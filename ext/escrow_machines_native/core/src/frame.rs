//! The fixed-length frame a replica persists to keep its identity durable.

use crate::{Escrow, Mark};
use merge_machines::{CodecError, GCounter, ReplicaId};

/// The byte that leads every escrow frame.
const TAG: u8 = 0xE5;
/// Bytes of a two-byte header field.
const SMALL: usize = 2;
/// Bytes of a 64-bit field.
const FIELD: usize = 8;
/// Bytes before the tables: tag, capacity, replica count, local replica.
const HEADER: usize = 1 + 3 * SMALL;
/// Bytes of one mark record: a presence byte, the id, the amount.
const MARK: usize = 1 + 2 * FIELD;

/// Why a frame could not be written or read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    /// The buffer is shorter than a frame.
    Truncated,
    /// The frame does not lead with the escrow tag.
    Tag,
    /// The frame was written for another capacity `N`.
    Shape,
    /// The frame has the right shape and describes an impossible state.
    Corrupt,
}

impl core::fmt::Display for FrameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Truncated => "buffer too short",
            Self::Tag => "not an escrow frame",
            Self::Shape => "capacity mismatch",
            Self::Corrupt => "impossible state",
        })
    }
}

impl core::error::Error for FrameError {}

/// The front of `bytes`, `len` long, leaving the rest in place.
fn take<'a>(bytes: &mut &'a [u8], len: usize) -> Result<&'a [u8], FrameError> {
    let (head, rest) = bytes.split_at_checked(len).ok_or(FrameError::Truncated)?;
    *bytes = rest;
    Ok(head)
}

fn take_u64(bytes: &mut &[u8]) -> Result<u64, FrameError> {
    let mut field = [0; FIELD];
    field.copy_from_slice(take(bytes, FIELD)?);
    Ok(u64::from_le_bytes(field))
}

fn take_small(bytes: &mut &[u8]) -> Result<usize, FrameError> {
    let mut field = [0; SMALL];
    field.copy_from_slice(take(bytes, SMALL)?);
    Ok(usize::from(u16::from_le_bytes(field)))
}

/// A frame being filled front to back.
struct Writer<'a> {
    out: &'a mut [u8],
    at: usize,
}

impl Writer<'_> {
    /// The next `len` bytes of the frame.
    fn slot(&mut self, len: usize) -> Result<&mut [u8], FrameError> {
        let end = self.at.checked_add(len).ok_or(FrameError::Truncated)?;
        let slot = self
            .out
            .get_mut(self.at..end)
            .ok_or(FrameError::Truncated)?;
        self.at = end;
        Ok(slot)
    }

    fn put(&mut self, field: &[u8]) -> Result<(), FrameError> {
        self.slot(field.len())?.copy_from_slice(field);
        Ok(())
    }

    fn put_small(&mut self, value: usize) -> Result<(), FrameError> {
        let value = u16::try_from(value).map_err(|_| FrameError::Shape)?;
        self.put(&value.to_le_bytes())
    }

    fn put_counter<const N: usize>(&mut self, counter: &GCounter<N>) -> Result<(), FrameError> {
        counter
            .encode(self.slot(GCounter::<N>::ENCODED_LEN)?)
            .map(|_| ())
            .map_err(|_| FrameError::Truncated)
    }
}

fn take_counter<const N: usize>(bytes: &mut &[u8]) -> Result<GCounter<N>, FrameError> {
    GCounter::decode(take(bytes, GCounter::<N>::ENCODED_LEN)?).map_err(|error| match error {
        CodecError::Truncated => FrameError::Truncated,
        CodecError::Tag | CodecError::Parameters => FrameError::Corrupt,
    })
}

/// Whether no replica holds two slots in `counter`, which only a forged frame
/// can arrange.
fn distinct<const N: usize>(counter: &GCounter<N>) -> bool {
    counter.entries().all(|entry| {
        let slots = counter.entries().filter(|o| o.replica == entry.replica);
        slots.count() == 1
    })
}

impl<const N: usize> Escrow<N> {
    /// Length of the [`encode`](Self::encode) frame.
    pub const ENCODED_LEN: usize =
        HEADER + N * FIELD + (N + 2) * GCounter::<N>::ENCODED_LEN + N * N * MARK;

    /// Writes this view into `out` and returns the number of bytes written.
    /// Persist the frame before acting on the call that changed the state.
    ///
    /// # Errors
    ///
    /// [`FrameError::Truncated`] when `out` is shorter than
    /// [`ENCODED_LEN`](Self::ENCODED_LEN), [`FrameError::Shape`] when `N` does
    /// not fit the frame's two-byte capacity field.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let out = out
            .get_mut(..Self::ENCODED_LEN)
            .ok_or(FrameError::Truncated)?;
        let mut out = Writer { out, at: 0 };
        out.put(&[TAG])?;
        out.put_small(N)?;
        out.put_small(self.replicas)?;
        out.put_small(self.local)?;
        for share in &self.allotted {
            out.put(&share.to_le_bytes())?;
        }
        out.put_counter(&self.spent)?;
        out.put_counter(&self.refunded)?;
        for row in &self.grants {
            out.put_counter(row)?;
        }
        for mark in self.marks.iter().flatten() {
            let Mark { id, amount } = mark.unwrap_or(Mark { id: 0, amount: 0 });
            out.put(&[u8::from(mark.is_some())])?;
            out.put(&id.to_le_bytes())?;
            out.put(&amount.to_le_bytes())?;
        }
        Ok(Self::ENCODED_LEN)
    }

    /// Reads a view written by [`encode`](Self::encode).
    ///
    /// # Errors
    ///
    /// [`FrameError::Truncated`], [`FrameError::Tag`], [`FrameError::Shape`]
    /// for a frame of another capacity, or [`FrameError::Corrupt`] for a frame
    /// whose contents no sequence of calls could have produced.
    pub fn decode(bytes: &[u8]) -> Result<Self, FrameError> {
        let mut bytes = bytes
            .get(..Self::ENCODED_LEN)
            .ok_or(FrameError::Truncated)?;
        if take(&mut bytes, 1)? != [TAG] {
            return Err(FrameError::Tag);
        }
        if take_small(&mut bytes)? != N {
            return Err(FrameError::Shape);
        }
        let replicas = take_small(&mut bytes)?;
        let local = take_small(&mut bytes)?;
        let mut allotted = [0; N];
        for share in &mut allotted {
            *share = take_u64(&mut bytes)?;
        }
        let spent = take_counter(&mut bytes)?;
        let refunded = take_counter(&mut bytes)?;
        let mut grants = [GCounter::new(); N];
        for row in &mut grants {
            *row = take_counter(&mut bytes)?;
        }
        let mut marks = [[None; N]; N];
        for mark in marks.iter_mut().flatten() {
            let present = take(&mut bytes, 1)?;
            let (id, amount) = (take_u64(&mut bytes)?, take_u64(&mut bytes)?);
            *mark = match present {
                [0] if id == 0 && amount == 0 => None,
                [1] => Some(Mark { id, amount }),
                _ => return Err(FrameError::Corrupt),
            };
        }
        let escrow = Self {
            local,
            replicas,
            allotted,
            spent,
            refunded,
            grants,
            marks,
        };
        if escrow.sound() {
            Ok(escrow)
        } else {
            Err(FrameError::Corrupt)
        }
    }

    /// Whether some sequence of calls could have produced this state.
    fn sound(&self) -> bool {
        let known = |replica: ReplicaId| self.index(replica).is_some();
        let members = (1..=N).contains(&self.replicas) && self.local < self.replicas;
        let supply = self
            .allotted
            .iter()
            .try_fold(0_u64, |sum, share| sum.checked_add(*share));
        members
            && supply.is_some()
            && self.allotted.iter().skip(self.replicas).all(|s| *s == 0)
            && distinct(&self.spent)
            && distinct(&self.refunded)
            && self.grants.iter().all(distinct)
            && self.spent.entries().all(|entry| known(entry.replica))
            && self
                .refunded
                .entries()
                .all(|entry| known(entry.replica) && entry.count <= self.spent.count(entry.replica))
            && (0..N).all(|from| self.sound_row(from))
            && (0..=i128::from(u64::MAX)).contains(&self.signed_balance(self.local))
    }

    /// Whether row `from` grants only to other known replicas, and every
    /// grant has the mark of the transfer that last raised it.
    fn sound_row(&self, from: usize) -> bool {
        let row = &self.grants[from];
        let cells_known = row.entries().all(|entry| {
            entry.count == 0 || (from < self.replicas && self.index(entry.replica).is_some())
        });
        cells_known
            && (0..N).all(|to| {
                let granted = row.count(to as ReplicaId);
                self.marks[from][to].map_or(granted == 0, |mark| {
                    from != to && mark.amount > 0 && mark.amount <= granted
                })
            })
    }
}
