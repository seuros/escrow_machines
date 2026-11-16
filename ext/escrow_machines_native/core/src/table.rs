//! The per-replica tables escrow keeps, and the index that reads them.
//!
//! Replica ids here are dense, `0..replicas` with `replicas <= N`, so every
//! table is a plain `[T; N]` and every lookup goes through a [`Member`], which
//! is below `N` by construction. A lookup therefore cannot miss, and no
//! caller has a "missing row" arm to answer.

use crate::Refusal;
use merge_machines::ReplicaId;

/// A position below `N` in a `[T; N]` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Member<const N: usize>(usize);

impl<const N: usize> Member<N> {
    /// The position `index`, or `None` at `N` or past it.
    pub const fn new(index: usize) -> Option<Self> {
        if index < N { Some(Self(index)) } else { None }
    }

    /// The position as a table index.
    pub const fn index(self) -> usize {
        self.0
    }

    /// The replica id this position holds the cells of.
    pub const fn id(self) -> ReplicaId {
        self.0 as ReplicaId
    }

    /// `table`'s cell for this member.
    // In bounds by construction: `new` is the only way in and answers only
    // below `N`, the table's length.
    #[allow(clippy::indexing_slicing)]
    pub const fn of<T>(self, table: &[T; N]) -> &T {
        &table[self.0]
    }

    /// `table`'s cell for this member, to write.
    // In bounds for the same reason as `of`.
    #[allow(clippy::indexing_slicing)]
    pub const fn of_mut<T>(self, table: &mut [T; N]) -> &mut T {
        &mut table[self.0]
    }
}

/// A grow-only count per member, joined cell by cell with `max`.
///
/// Unlike a slot-table counter, it holds a cell for every position up front,
/// so raising a member's count never needs a free slot: the only refusal is
/// passing [`u64::MAX`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tally<const N: usize>([u64; N]);

impl<const N: usize> Tally<N> {
    /// Zero for every member.
    pub const fn new() -> Self {
        Self([0; N])
    }

    /// A tally holding exactly `counts`, as a frame stores it.
    pub const fn from_counts(counts: [u64; N]) -> Self {
        Self(counts)
    }

    /// Every cell, member order.
    pub const fn counts(&self) -> &[u64; N] {
        &self.0
    }

    /// What `member` has counted.
    pub const fn count(&self, member: Member<N>) -> u64 {
        *member.of(&self.0)
    }

    /// Every cell added up, wide enough never to clamp: at most `N` terms of
    /// at most [`u64::MAX`].
    pub fn total(&self) -> i128 {
        self.0.iter().map(|count| i128::from(*count)).sum()
    }

    /// Raises `member`'s count by `amount` and answers the new count.
    ///
    /// # Errors
    ///
    /// [`Refusal::Overflow`] when the count would pass [`u64::MAX`]; the
    /// tally is unchanged.
    pub fn raise(&mut self, member: Member<N>, amount: u64) -> Result<u64, Refusal> {
        let cell = member.of_mut(&mut self.0);
        *cell = cell.checked_add(amount).ok_or(Refusal::Overflow)?;
        Ok(*cell)
    }

    /// Folds `other` in: each cell becomes the larger of the two.
    pub fn join(&mut self, other: &Self) {
        for (mine, theirs) in self.0.iter_mut().zip(&other.0) {
            *mine = (*mine).max(*theirs);
        }
    }

    /// Whether every cell at or past position `len` is zero.
    pub fn zero_from(&self, len: usize) -> bool {
        self.0.iter().skip(len).all(|count| *count == 0)
    }
}

#[cfg(test)]
mod tests;
