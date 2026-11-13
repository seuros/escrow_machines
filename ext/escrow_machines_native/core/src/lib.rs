#![cfg_attr(not(test), no_std)]
//! Bounded-counter escrow: a fixed set of replicas shares a fixed supply of
//! rights, and each replica may spend only the rights it holds locally.
//!
//! The state is the bounded-counter CRDT of Balegas et al.: a matrix of
//! cumulative grants between replicas plus per-replica spent and refunded
//! counters, all grow-only and joined cell by cell. Unlike an unrestricted
//! PN-counter, a spend is checked against the local share before it is
//! recorded, so the sum of everything spent can never exceed the supply, no
//! matter how the replicas are partitioned. Unlike a rate limiter, nothing
//! refills: rights move, they are never minted.
//!
//! Each replica writes only its own cells: its spent and refunded counters and
//! its row of outgoing grants. Everything else it learns through
//! [`Escrow::merge`] or [`Escrow::accept`]. That ownership is the whole safety
//! argument, and it makes two things the caller's contract:
//!
//! * **Durable replica identities.** A replica must persist its state (see
//!   [`Escrow::encode`]) before acting on a successful spend, refund or
//!   transfer. A replica that restarts from an older state under the same
//!   identity can spend the same rights twice. [`Escrow::merge`] refuses with
//!   [`Refusal::Diverged`] when a peer proves that happened.
//! * **Transfer replay protection.** A transfer carries a caller-chosen id that
//!   strictly increases per `(from, to)` pair. Retrying the latest id returns
//!   the original [`Receipt`]; an older id is refused. Receipts are cumulative,
//!   so delivering one twice, late, or out of order is harmless.
//!
//! Nothing here reads a clock, talks to a network, stores anything, or
//! allocates.

mod frame;
mod merge;
mod ops;
#[cfg(test)]
mod tests;

pub use frame::FrameError;
use merge_machines::GCounter;
pub use merge_machines::{Merge, ReplicaId};

/// The latest transfer one replica made to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Mark {
    /// The caller-chosen id of that transfer.
    pub id: u64,
    /// How many rights it moved.
    pub amount: u64,
}

/// Proof of one transfer, safe to deliver any number of times in any order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Receipt {
    /// The replica that gave the rights away.
    pub from: ReplicaId,
    /// The replica that received them.
    pub to: ReplicaId,
    /// The caller-chosen id of the transfer.
    pub id: u64,
    /// How many rights this transfer moved.
    pub amount: u64,
    /// Everything `from` has ever granted `to`, this transfer included.
    pub total: u64,
}

/// What a transfer request did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transfer {
    /// The rights moved now.
    Applied(Receipt),
    /// This id already moved these rights; nothing moved again.
    Replayed(Receipt),
}

impl Transfer {
    /// The receipt to deliver, the same one whether applied or replayed.
    #[must_use]
    pub const fn receipt(&self) -> Receipt {
        match self {
            Self::Applied(receipt) | Self::Replayed(receipt) => *receipt,
        }
    }
}

/// What a delivered receipt did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accepted {
    /// The receipt taught this replica about a grant it had not seen.
    Applied,
    /// This replica already knew everything the receipt says.
    Replayed,
}

/// Why a call changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The amount was zero.
    ZeroAmount,
    /// The local replica holds fewer rights than the call needs.
    Insufficient {
        /// The rights the local replica holds.
        available: u64,
    },
    /// A refund asked for more than the local replica has outstanding.
    ExceedsSpent {
        /// Local spends not yet refunded.
        outstanding: u64,
    },
    /// A replica id is outside the configured set.
    UnknownReplica,
    /// A transfer named the same replica on both ends.
    SelfTransfer,
    /// The transfer id is older than the latest one for this pair.
    StaleId {
        /// The latest id recorded for the pair.
        latest: u64,
    },
    /// The id was already used for this pair with a different amount.
    IdConflict {
        /// The amount recorded under that id.
        amount: u64,
    },
    /// A receipt contradicts itself or the history it claims to extend.
    Malformed,
    /// A receipt claims a grant from the local replica that it never made.
    Forged,
    /// The other state was built for a different replica set or allotment.
    ConfigMismatch,
    /// The other state knows more about the local replica's own cells than the
    /// local replica does, or two histories disagree about one transfer id.
    Diverged,
    /// Adopting the other state would leave the local replica below zero.
    Overdrawn,
    /// A cumulative counter or the local balance would pass [`u64::MAX`].
    Overflow,
}

impl core::fmt::Display for Refusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ZeroAmount => f.write_str("the amount is zero"),
            Self::Insufficient { available } => {
                write!(f, "the local replica holds only {available}")
            }
            Self::ExceedsSpent { outstanding } => {
                write!(f, "only {outstanding} is spent and not refunded")
            }
            Self::UnknownReplica => f.write_str("the replica is outside the configured set"),
            Self::SelfTransfer => f.write_str("the transfer names one replica on both ends"),
            Self::StaleId { latest } => {
                write!(f, "the latest transfer id for the pair is {latest}")
            }
            Self::IdConflict { amount } => write!(f, "the id already moved {amount}"),
            Self::Malformed => f.write_str("the receipt contradicts itself or its history"),
            Self::Forged => f.write_str("the receipt claims a grant the local replica never made"),
            Self::ConfigMismatch => f.write_str("the other state has another allotment"),
            Self::Diverged => {
                f.write_str("the other state knows more of the local replica's cells")
            }
            Self::Overdrawn => {
                f.write_str("the adopted history leaves the local replica below zero")
            }
            Self::Overflow => f.write_str("a counter or the balance would pass u64::MAX"),
        }
    }
}

impl core::error::Error for Refusal {}

/// Why an escrow could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// The allotment names no replica.
    NoReplicas,
    /// The allotment names more replicas than the capacity `N`.
    TooManyReplicas,
    /// The local replica is not in the allotment.
    UnknownReplica,
    /// The allotment adds up to more than [`u64::MAX`].
    SupplyOverflow,
}

/// One replica's view of a supply of rights shared by up to `N` replicas.
///
/// ```
/// use escrow_machines::{Escrow, Refusal};
///
/// let mut left = Escrow::<4>::new(0, &[10, 0]).unwrap();
/// let mut right = Escrow::<4>::new(1, &[10, 0]).unwrap();
///
/// assert_eq!(right.spend(1), Err(Refusal::Insufficient { available: 0 }));
///
/// let moved = left.transfer(1, 1, 4).unwrap();
/// assert_eq!(left.available(), 6);
/// right.accept(&moved.receipt()).unwrap();
/// assert_eq!(right.spend(4), Ok(0));
///
/// // A retry of the same id moves nothing more.
/// assert_eq!(left.transfer(1, 1, 4).unwrap().receipt(), moved.receipt());
/// assert_eq!(left.available(), 6);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Escrow<const N: usize> {
    local: usize,
    replicas: usize,
    allotted: [u64; N],
    spent: GCounter<N>,
    refunded: GCounter<N>,
    /// `grants[from]` counts, per receiving replica, what `from` gave away.
    grants: [GCounter<N>; N],
    /// `marks[from][to]` is the latest transfer `from` made to `to`.
    marks: [[Option<Mark>; N]; N],
}

impl<const N: usize> Escrow<N> {
    /// How many replicas this shape can hold.
    pub const CAPACITY: usize = N;

    /// The view of replica `local`, where replica `i` starts with
    /// `allotted[i]` rights. Every replica must be built from the same
    /// allotment.
    ///
    /// # Errors
    ///
    /// [`ConfigError`] when the allotment is empty, longer than `N`, does not
    /// include `local`, or adds up to more than [`u64::MAX`].
    pub fn new(local: ReplicaId, allotted: &[u64]) -> Result<Self, ConfigError> {
        if allotted.is_empty() {
            return Err(ConfigError::NoReplicas);
        }
        if allotted.len() > N {
            return Err(ConfigError::TooManyReplicas);
        }
        let local = usize::try_from(local)
            .ok()
            .filter(|index| *index < allotted.len())
            .ok_or(ConfigError::UnknownReplica)?;
        allotted
            .iter()
            .try_fold(0_u64, |sum, share| sum.checked_add(*share))
            .ok_or(ConfigError::SupplyOverflow)?;
        let mut shares = [0; N];
        for (slot, share) in shares.iter_mut().zip(allotted) {
            *slot = *share;
        }
        Ok(Self {
            local,
            replicas: allotted.len(),
            allotted: shares,
            spent: GCounter::new(),
            refunded: GCounter::new(),
            grants: [GCounter::new(); N],
            marks: [[None; N]; N],
        })
    }

    /// The replica this view belongs to.
    #[must_use]
    pub const fn local(&self) -> ReplicaId {
        self.local as ReplicaId
    }

    /// How many replicas share the supply.
    #[must_use]
    pub const fn replicas(&self) -> usize {
        self.replicas
    }

    /// Every right that exists, held or spent.
    #[must_use]
    pub fn supply(&self) -> u64 {
        self.allotted
            .iter()
            .fold(0_u64, |sum, share| sum.saturating_add(*share))
    }

    /// The rights `replica` started with.
    #[must_use]
    pub fn allotted(&self, replica: ReplicaId) -> Option<u64> {
        self.index(replica).map(|index| self.allotted[index])
    }

    /// The rights the local replica holds and may spend or transfer now.
    #[must_use]
    pub fn available(&self) -> u64 {
        u64::try_from(self.signed_balance(self.local)).unwrap_or(0)
    }

    /// The rights `replica` holds as far as this view knows. For another
    /// replica this is an estimate, and it is negative when this view has seen
    /// that replica pass on rights whose arrival it has not seen yet.
    #[must_use]
    pub fn balance(&self, replica: ReplicaId) -> Option<i128> {
        self.index(replica).map(|index| self.signed_balance(index))
    }

    /// Everything `replica` has spent, as far as this view knows.
    #[must_use]
    pub fn spent(&self, replica: ReplicaId) -> Option<u64> {
        self.index(replica).map(|_| self.spent.count(replica))
    }

    /// Everything `replica` has refunded, as far as this view knows.
    #[must_use]
    pub fn refunded(&self, replica: ReplicaId) -> Option<u64> {
        self.index(replica).map(|_| self.refunded.count(replica))
    }

    /// What `replica` has spent and not refunded, as far as this view knows.
    #[must_use]
    pub fn outstanding(&self, replica: ReplicaId) -> Option<u64> {
        self.index(replica).map(|_| {
            self.spent
                .count(replica)
                .saturating_sub(self.refunded.count(replica))
        })
    }

    /// Everything `from` has granted `to`, as far as this view knows.
    #[must_use]
    pub fn granted(&self, from: ReplicaId, to: ReplicaId) -> Option<u64> {
        let from = self.index(from)?;
        self.index(to).map(|_| self.grants[from].count(to))
    }

    /// The latest transfer `from` made to `to`, as far as this view knows.
    #[must_use]
    pub fn last_transfer(&self, from: ReplicaId, to: ReplicaId) -> Option<Mark> {
        self.marks[self.index(from)?][self.index(to)?]
    }

    /// Where `replica` sits in the tables, if it is one of the replicas.
    fn index(&self, replica: ReplicaId) -> Option<usize> {
        usize::try_from(replica)
            .ok()
            .filter(|index| *index < self.replicas)
    }

    /// Allotment plus grants received, minus grants made and net spends.
    fn signed_balance(&self, index: usize) -> i128 {
        let replica = index as ReplicaId;
        let received: i128 = self
            .grants
            .iter()
            .map(|row| i128::from(row.count(replica)))
            .sum();
        let given: i128 = self.grants[index]
            .entries()
            .map(|entry| i128::from(entry.count))
            .sum();
        i128::from(self.allotted[index]) + received - given - i128::from(self.spent.count(replica))
            + i128::from(self.refunded.count(replica))
    }
}
