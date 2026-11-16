//! The fixed-length frame a replica persists to keep its identity durable.

use crate::table::{Member, Tally};
use crate::{Escrow, Mark, Refusal};

/// The byte that leads every escrow frame. `0xE5` led the frames that
/// embedded `merge-machines` counter frames; those are another structure now.
const TAG: u8 = 0xE6;
/// Bytes of a two-byte header field.
const SMALL: usize = 2;
/// Bytes of a 64-bit field.
const FIELD: usize = 8;
/// Bytes before the tables: tag, capacity, replica count, local replica.
const HEADER: usize = 1 + 3 * SMALL;
/// Bytes of one mark record: a presence byte, the id, the amount.
const MARK: usize = 1 + 2 * FIELD;

/// The front of `bytes`, `len` long, leaving the rest in place.
fn take<'a>(bytes: &mut &'a [u8], len: usize) -> Result<&'a [u8], Refusal> {
    let (head, rest) = bytes.split_at_checked(len).ok_or(Refusal::Truncated)?;
    *bytes = rest;
    Ok(head)
}

fn take_u64(bytes: &mut &[u8]) -> Result<u64, Refusal> {
    let mut field = [0; FIELD];
    field.copy_from_slice(take(bytes, FIELD)?);
    Ok(u64::from_le_bytes(field))
}

fn take_small(bytes: &mut &[u8]) -> Result<usize, Refusal> {
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
    fn slot(&mut self, len: usize) -> Result<&mut [u8], Refusal> {
        let end = self.at.checked_add(len).ok_or(Refusal::Truncated)?;
        let slot = self.out.get_mut(self.at..end).ok_or(Refusal::Truncated)?;
        self.at = end;
        Ok(slot)
    }

    fn put(&mut self, field: &[u8]) -> Result<(), Refusal> {
        self.slot(field.len())?.copy_from_slice(field);
        Ok(())
    }

    /// The low two bytes of `value`, which is at most `N`, and `encode`
    /// rejects at compile time any `N` above [`u16::MAX`].
    fn put_small(&mut self, value: usize) -> Result<(), Refusal> {
        let [low, high, ..] = value.to_le_bytes();
        self.put(&[low, high])
    }

    /// Every cell of `tally`, member order, one 64-bit field each.
    fn put_tally<const N: usize>(&mut self, tally: &Tally<N>) -> Result<(), Refusal> {
        for count in tally.counts() {
            self.put(&count.to_le_bytes())?;
        }
        Ok(())
    }
}

/// `N` 64-bit fields read as one tally.
fn take_tally<const N: usize>(bytes: &mut &[u8]) -> Result<Tally<N>, Refusal> {
    let mut counts = [0; N];
    for count in &mut counts {
        *count = take_u64(bytes)?;
    }
    Ok(Tally::from_counts(counts))
}

impl<const N: usize> Escrow<N> {
    /// Length of the [`encode`](Self::encode) frame.
    /// The allotment, then the spent, refunded and `N` grant tallies, each
    /// `N` fields, then the `N * N` marks.
    pub const ENCODED_LEN: usize = HEADER + N * FIELD + (N + 2) * N * FIELD + N * N * MARK;

    /// Writes this view into `out` and returns the number of bytes written.
    /// Persist the frame before acting on the call that changed the state.
    ///
    /// # Errors
    ///
    /// [`Refusal::Truncated`] when `out` is shorter than
    /// [`ENCODED_LEN`](Self::ENCODED_LEN), and nothing else: an `N` that does
    /// not fit the frame's two-byte capacity field does not compile.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, Refusal> {
        const { assert!(N <= u16::MAX as usize, "the capacity must fit two bytes") };
        let out = out.get_mut(..Self::ENCODED_LEN).ok_or(Refusal::Truncated)?;
        let mut out = Writer { out, at: 0 };
        out.put(&[TAG])?;
        out.put_small(N)?;
        out.put_small(self.replicas)?;
        out.put_small(self.local.index())?;
        for share in &self.allotted {
            out.put(&share.to_le_bytes())?;
        }
        out.put_tally(&self.spent)?;
        out.put_tally(&self.refunded)?;
        for row in &self.grants {
            out.put_tally(row)?;
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
    /// [`Refusal::Truncated`], [`Refusal::WrongStructure`], [`Refusal::WrongShape`]
    /// for a frame of another capacity, or [`Refusal::Malformed`] for a frame
    /// whose contents no sequence of calls could have produced.
    pub fn decode(bytes: &[u8]) -> Result<Self, Refusal> {
        let mut bytes = bytes.get(..Self::ENCODED_LEN).ok_or(Refusal::Truncated)?;
        if take(&mut bytes, 1)? != [TAG] {
            return Err(Refusal::WrongStructure);
        }
        if take_small(&mut bytes)? != N {
            return Err(Refusal::WrongShape);
        }
        let replicas = take_small(&mut bytes)?;
        // A local position past the capacity is not one `sound` can even
        // look up: the frame is impossible.
        let local = Member::new(take_small(&mut bytes)?).ok_or(Refusal::Malformed)?;
        let mut allotted = [0; N];
        for share in &mut allotted {
            *share = take_u64(&mut bytes)?;
        }
        let spent = take_tally(&mut bytes)?;
        let refunded = take_tally(&mut bytes)?;
        let mut grants = [Tally::new(); N];
        for row in &mut grants {
            *row = take_tally(&mut bytes)?;
        }
        let mut marks = [[None; N]; N];
        for mark in marks.iter_mut().flatten() {
            let present = take(&mut bytes, 1)?;
            let (id, amount) = (take_u64(&mut bytes)?, take_u64(&mut bytes)?);
            *mark = match present {
                [0] if id == 0 && amount == 0 => None,
                [1] => Some(Mark { id, amount }),
                _ => return Err(Refusal::Malformed),
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
            Err(Refusal::Malformed)
        }
    }

    /// Whether some sequence of calls could have produced this state.
    fn sound(&self) -> bool {
        let members = (1..=N).contains(&self.replicas) && self.local.index() < self.replicas;
        let supply = self
            .allotted
            .iter()
            .try_fold(0_u64, |sum, share| sum.checked_add(*share));
        members
            && supply.is_some()
            && self.allotted.iter().skip(self.replicas).all(|s| *s == 0)
            && self.spent.zero_from(self.replicas)
            && self.refunded.zero_from(self.replicas)
            && self
                .refunded
                .counts()
                .iter()
                .zip(self.spent.counts())
                .all(|(refunded, spent)| refunded <= spent)
            && self
                .grants
                .iter()
                .zip(&self.marks)
                .enumerate()
                .all(|(from, (row, marks))| self.sound_row(from, row, marks))
            && (0..=i128::from(u64::MAX)).contains(&self.signed_balance(self.local))
    }

    /// Whether row `from` grants only to other known replicas, and every
    /// grant has the mark of the transfer that last raised it.
    fn sound_row(&self, from: usize, row: &Tally<N>, marks: &[Option<Mark>; N]) -> bool {
        let cells_known = row.zero_from(if from < self.replicas {
            self.replicas
        } else {
            0
        });
        cells_known
            && marks
                .iter()
                .zip(row.counts())
                .enumerate()
                .all(|(to, (mark, granted))| {
                    mark.map_or(*granted == 0, |mark| {
                        from != to && mark.amount > 0 && mark.amount <= *granted
                    })
                })
    }
}
