//! What a replica does with its own rights: spend, refund, transfer, accept.

use crate::{Accepted, Escrow, Mark, Receipt, Refusal, Transfer};
use merge_machines::ReplicaId;

impl<const N: usize> Escrow<N> {
    /// Spends `amount` of the local replica's rights and returns what it has
    /// left.
    ///
    /// # Errors
    ///
    /// [`Refusal::ZeroAmount`], [`Refusal::Insufficient`] when the local
    /// replica holds less than `amount` (rights held elsewhere do not count),
    /// or [`Refusal::Overflow`] when the lifetime spent counter is exhausted.
    /// The state is untouched on refusal.
    pub fn spend(&mut self, amount: u64) -> Result<u64, Refusal> {
        if amount == 0 {
            return Err(Refusal::ZeroAmount);
        }
        let available = self.available();
        if amount > available {
            return Err(Refusal::Insufficient { available });
        }
        let local = self.local();
        self.spent
            .count(local)
            .checked_add(amount)
            .ok_or(Refusal::Overflow)?;
        self.spent
            .increment(local, amount)
            .map_err(|_| Refusal::Overflow)?;
        Ok(available - amount)
    }

    /// Returns `amount` of the local replica's own spends to its balance and
    /// returns what it now holds.
    ///
    /// # Errors
    ///
    /// [`Refusal::ZeroAmount`], [`Refusal::ExceedsSpent`] when `amount` is more
    /// than the local replica has spent and not yet refunded (a refund never
    /// mints), or [`Refusal::Overflow`] when the lifetime refunded counter is
    /// exhausted. The state is untouched on refusal.
    pub fn refund(&mut self, amount: u64) -> Result<u64, Refusal> {
        if amount == 0 {
            return Err(Refusal::ZeroAmount);
        }
        let local = self.local();
        let outstanding = self
            .spent
            .count(local)
            .saturating_sub(self.refunded.count(local));
        if amount > outstanding {
            return Err(Refusal::ExceedsSpent { outstanding });
        }
        self.refunded
            .count(local)
            .checked_add(amount)
            .ok_or(Refusal::Overflow)?;
        self.refunded
            .increment(local, amount)
            .map_err(|_| Refusal::Overflow)?;
        Ok(self.available())
    }

    /// Moves `amount` of the local replica's rights to `to` under transfer
    /// `id`.
    ///
    /// Ids are chosen by the caller and must strictly increase per
    /// destination. Asking again with the latest id and the same amount moves
    /// nothing and returns the original receipt as [`Transfer::Replayed`].
    ///
    /// # Errors
    ///
    /// [`Refusal::ZeroAmount`], [`Refusal::UnknownReplica`],
    /// [`Refusal::SelfTransfer`], [`Refusal::StaleId`] for an id older than the
    /// latest, [`Refusal::IdConflict`] for the latest id with another amount,
    /// [`Refusal::Insufficient`], or [`Refusal::Overflow`] when the lifetime
    /// grant counter for the pair is exhausted. The state is untouched on
    /// refusal.
    pub fn transfer(&mut self, to: ReplicaId, id: u64, amount: u64) -> Result<Transfer, Refusal> {
        if amount == 0 {
            return Err(Refusal::ZeroAmount);
        }
        let slot = self.index(to).ok_or(Refusal::UnknownReplica)?;
        if slot == self.local {
            return Err(Refusal::SelfTransfer);
        }
        let from = self.local();
        let granted = self.grants[self.local].count(to);
        if let Some(mark) = self.marks[self.local][slot] {
            if id < mark.id {
                return Err(Refusal::StaleId { latest: mark.id });
            }
            if id == mark.id {
                if amount != mark.amount {
                    return Err(Refusal::IdConflict {
                        amount: mark.amount,
                    });
                }
                return Ok(Transfer::Replayed(Receipt {
                    from,
                    to,
                    id,
                    amount,
                    total: granted,
                }));
            }
        }
        let available = self.available();
        if amount > available {
            return Err(Refusal::Insufficient { available });
        }
        let total = granted.checked_add(amount).ok_or(Refusal::Overflow)?;
        self.grants[self.local]
            .increment(to, amount)
            .map_err(|_| Refusal::Overflow)?;
        self.marks[self.local][slot] = Some(Mark { id, amount });
        Ok(Transfer::Applied(Receipt {
            from,
            to,
            id,
            amount,
            total,
        }))
    }

    /// Learns of a transfer from its receipt, without a full merge.
    ///
    /// Receipts are cumulative: one that is older than what this view already
    /// knows, or the same, is [`Accepted::Replayed`] and changes nothing.
    ///
    /// # Errors
    ///
    /// [`Refusal::UnknownReplica`], [`Refusal::SelfTransfer`],
    /// [`Refusal::Malformed`] for a receipt that contradicts itself or the
    /// grants already seen for the pair, [`Refusal::IdConflict`] for a known id
    /// with another amount, [`Refusal::Forged`] for a grant the local replica
    /// would have had to make and did not, or [`Refusal::Overflow`] when the
    /// local balance would pass [`u64::MAX`]. The state is untouched on
    /// refusal.
    pub fn accept(&mut self, receipt: &Receipt) -> Result<Accepted, Refusal> {
        let from = self.index(receipt.from).ok_or(Refusal::UnknownReplica)?;
        let to = self.index(receipt.to).ok_or(Refusal::UnknownReplica)?;
        if from == to {
            return Err(Refusal::SelfTransfer);
        }
        if receipt.amount == 0 || receipt.amount > receipt.total {
            return Err(Refusal::Malformed);
        }
        let held = self.grants[from].count(receipt.to);
        let known = self.marks[from][to];
        let fresh = match known {
            Some(mark) if receipt.id == mark.id => {
                if receipt.amount != mark.amount {
                    return Err(Refusal::IdConflict {
                        amount: mark.amount,
                    });
                }
                if receipt.total != held {
                    return Err(Refusal::Malformed);
                }
                false
            }
            Some(mark) if receipt.id < mark.id => {
                if receipt.total > held.saturating_sub(mark.amount) {
                    return Err(Refusal::Malformed);
                }
                false
            }
            None | Some(_) => true,
        };
        if !fresh {
            return Ok(Accepted::Replayed);
        }
        if from == self.local {
            return Err(Refusal::Forged);
        }
        let raise = receipt
            .total
            .checked_sub(held)
            .filter(|raise| *raise >= receipt.amount)
            .ok_or(Refusal::Malformed)?;
        if to == self.local && self.available().checked_add(raise).is_none() {
            return Err(Refusal::Overflow);
        }
        self.grants[from]
            .increment(receipt.to, raise)
            .map_err(|_| Refusal::Overflow)?;
        self.marks[from][to] = Some(Mark {
            id: receipt.id,
            amount: receipt.amount,
        });
        Ok(Accepted::Applied)
    }
}
