//! Joining two replicas' views.

use crate::{Escrow, Mark, Refusal};
use core::cmp::Ordering;
use merge_machines::Merge;

impl<const N: usize> Escrow<N> {
    /// Folds another replica's view into this one. Commutative, associative
    /// and idempotent across the views of one deployment.
    ///
    /// # Errors
    ///
    /// [`Refusal::ConfigMismatch`] when `other` was built from a different
    /// allotment. [`Refusal::Diverged`] when `other` records more spends,
    /// refunds or outgoing transfers for the local replica than the local
    /// replica remembers making (it lost state it had already shown to a peer),
    /// or when the two views disagree about a transfer: one id with another
    /// amount or cumulative total, or a newer id whose total does not cover
    /// the older one.
    /// [`Refusal::Overflow`] when the local balance would pass [`u64::MAX`].
    /// The state is untouched on refusal.
    pub fn merge(&mut self, other: &Self) -> Result<(), Refusal> {
        self.join(other, true)
    }

    /// Folds `other` in like [`merge`](Self::merge), but adopts what `other`
    /// knows about the local replica's own cells instead of refusing.
    ///
    /// This is the explicit way back for a replica that lost state. It
    /// restores only what some peer had already seen: a spend or transfer that
    /// was acted on but never left the lost replica is gone, and the caller
    /// owns the consequences of that.
    ///
    /// # Errors
    ///
    /// [`Refusal::ConfigMismatch`], [`Refusal::Diverged`] when the two views
    /// disagree about a transfer as in [`merge`](Self::merge), [`Refusal::Overdrawn`]
    /// when the adopted history shows the local replica gave away or spent
    /// more than it held, or [`Refusal::Overflow`]. The state is untouched on
    /// refusal.
    pub fn recover(&mut self, other: &Self) -> Result<(), Refusal> {
        self.join(other, false)
    }

    fn join(&mut self, other: &Self, strict: bool) -> Result<(), Refusal> {
        if self.replicas != other.replicas || self.allotted != other.allotted {
            return Err(Refusal::ConfigMismatch);
        }
        if strict && self.outrun_by(other) {
            return Err(Refusal::Diverged);
        }
        let cells = (self.grants.iter().zip(&other.grants))
            .flat_map(|(mine, theirs)| mine.counts().iter().zip(theirs.counts()));
        let marks = (self.marks.iter().zip(&other.marks))
            .flat_map(|(mine, theirs)| mine.iter().zip(theirs));
        if !cells
            .zip(marks)
            .all(|((held, seen), (mine, theirs))| agree((*mine, *held), (*theirs, *seen)))
        {
            return Err(Refusal::Diverged);
        }
        let mut next = *self;
        next.spent.join(&other.spent);
        next.refunded.join(&other.refunded);
        for (mine, theirs) in next.grants.iter_mut().zip(&other.grants) {
            mine.join(theirs);
        }
        let pairs = next.marks.iter_mut().zip(&other.marks);
        for (mine, theirs) in pairs.flat_map(|(mine, theirs)| mine.iter_mut().zip(theirs)) {
            *mine = (*mine).max(*theirs);
        }
        let balance = next.signed_balance(next.local);
        if balance < 0 {
            return Err(Refusal::Overdrawn);
        }
        if balance > i128::from(u64::MAX) {
            return Err(Refusal::Overflow);
        }
        *self = next;
        Ok(())
    }

    /// Whether `other` holds more in the cells only the local replica writes.
    fn outrun_by(&self, other: &Self) -> bool {
        let local = self.local;
        let ahead = |mine: &[u64], theirs: &[u64]| theirs.iter().zip(mine).any(|(t, m)| t > m);
        other.spent.count(local) > self.spent.count(local)
            || other.refunded.count(local) > self.refunded.count(local)
            || ahead(
                local.of(&self.grants).counts(),
                local.of(&other.grants).counts(),
            )
            || local
                .of(&self.marks)
                .iter()
                .zip(local.of(&other.marks))
                .any(|(mine, theirs)| theirs > mine)
    }
}

/// Whether two views' knowledge of one `(from, to)` pair, each a latest mark
/// and the cumulative grant it left, could come from one history.
///
/// A mark's grant cell is the cumulative total at that transfer. One id must
/// carry one amount and one total; a newer id's total must cover the older
/// total plus the newer amount, because ids strictly increase per pair.
fn agree(left: (Option<Mark>, u64), right: (Option<Mark>, u64)) -> bool {
    // A pair with no mark has no grant (`decode` checks it): nothing to
    // contradict.
    let ((Some(mine), held), (Some(theirs), seen)) = (left, right) else {
        return true;
    };
    match mine.id.cmp(&theirs.id) {
        Ordering::Equal => mine.amount == theirs.amount && held == seen,
        Ordering::Less => seen
            .checked_sub(theirs.amount)
            .is_some_and(|before| held <= before),
        Ordering::Greater => held
            .checked_sub(mine.amount)
            .is_some_and(|before| seen <= before),
    }
}

impl<const N: usize> Merge for Escrow<N> {
    type Error = Refusal;

    /// The same join as [`Escrow::merge`], which leaves the state untouched
    /// when it refuses.
    fn merge(&mut self, other: &Self) -> Result<(), Self::Error> {
        Self::merge(self, other)
    }
}
