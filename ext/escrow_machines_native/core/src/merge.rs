//! Joining two replicas' views.

use crate::{Escrow, Refusal};
use merge_machines::{CapacityError, Merge, ReplicaId};

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
    /// or when the two views hold one transfer id with different amounts.
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
    /// hold one transfer id with different amounts, [`Refusal::Overdrawn`]
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
        let mut next = *self;
        next.spent
            .merge(&other.spent)
            .map_err(|CapacityError| Refusal::Overflow)?;
        next.refunded
            .merge(&other.refunded)
            .map_err(|CapacityError| Refusal::Overflow)?;
        for (mine, theirs) in next.grants.iter_mut().zip(&other.grants) {
            mine.merge(theirs)
                .map_err(|CapacityError| Refusal::Overflow)?;
        }
        let pairs = next.marks.iter_mut().zip(&other.marks);
        for (mine, theirs) in pairs.flat_map(|(mine, theirs)| mine.iter_mut().zip(theirs)) {
            if let (Some(left), Some(right)) = (*mine, *theirs)
                && left.id == right.id
                && left.amount != right.amount
            {
                return Err(Refusal::Diverged);
            }
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
        let local = self.local();
        let row = self.local;
        other.spent.count(local) > self.spent.count(local)
            || other.refunded.count(local) > self.refunded.count(local)
            || (0..self.replicas).any(|to| {
                other.grants[row].count(to as ReplicaId) > self.grants[row].count(to as ReplicaId)
                    || other.marks[row][to] > self.marks[row][to]
            })
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
