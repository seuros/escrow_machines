//! Thin binding to the bounded-counter escrow.
mod init;

use init::classes;

use bytes::Bytes;
use escrow_machines::{Accepted, ConfigError, Escrow, Mark, Receipt, Refusal, Transfer};
use magnus::{Error, RString, Ruby, Symbol, TryConvert, Value, prelude::*, typed_data::Obj};
use matryona_machines::ruby::{exact_array, exact_fields, exact_typed, integer, keep, symbol};
use std::cell::RefCell;

/// Replicas one compiled escrow can hold.
const CAPACITY: usize = 8;

type Core = Escrow<CAPACITY>;

const fn refusal(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::ZeroAmount => "zero_amount",
        Refusal::Insufficient { .. } => "insufficient",
        Refusal::ExceedsSpent { .. } => "exceeds_spent",
        Refusal::UnknownReplica => "unknown_replica",
        Refusal::SelfTransfer => "self_transfer",
        Refusal::StaleId { .. } => "stale_id",
        Refusal::IdConflict { .. } => "id_conflict",
        Refusal::Malformed => "malformed",
        Refusal::Forged => "forged",
        Refusal::ConfigMismatch => "config_mismatch",
        Refusal::Diverged => "diverged",
        Refusal::Overdrawn => "overdrawn",
        Refusal::Overflow => "overflow",
    }
}

fn receipt_from(ruby: &Ruby, value: Value) -> Result<Receipt, Error> {
    // Exactly a Receipt: sealing cannot stop a subclass being bound, and a
    // value that skipped `initialize` (`Marshal.load`) is re-checked below.
    // The fields are read natively: no reader runs.
    let class = classes(ruby)?.receipt.get(ruby);
    let [from, to, id, amount, total] =
        exact_fields::<5>(ruby, value, class, "expected an EscrowMachines::Receipt")?;
    let (from, to, id) = (
        integer(from, "from")?,
        integer(to, "to")?,
        integer(id, "id")?,
    );
    let (amount, total) = (integer(amount, "amount")?, integer(total, "total")?);
    Receipt::new(from, to, id, amount, total).ok_or_else(|| inconsistent_receipt(ruby))
}

/// The `RangeError` for receipt fields `Receipt::new` turns down.
fn inconsistent_receipt(ruby: &Ruby) -> Error {
    Error::new(
        ruby.exception_range_error(),
        "a receipt names two replicas and moves 1..=total rights",
    )
}

/// `Receipt.new`'s private hook: the fields as `Receipt::new` accepts them.
fn checked_receipt(
    ruby: &Ruby,
    from: Value,
    to: Value,
    id: Value,
    amount: Value,
    total: Value,
) -> Result<(u64, u64, u64, u64, u64), Error> {
    let (from, to, id) = (
        integer(from, "from")?,
        integer(to, "to")?,
        integer(id, "id")?,
    );
    let (amount, total) = (integer(amount, "amount")?, integer(total, "total")?);
    let receipt =
        Receipt::new(from, to, id, amount, total).ok_or_else(|| inconsistent_receipt(ruby))?;
    Ok(fields(receipt))
}

/// A receipt's fields in `Receipt.new`'s order.
const fn fields(receipt: Receipt) -> (u64, u64, u64, u64, u64) {
    (
        receipt.from(),
        receipt.to(),
        receipt.id(),
        receipt.amount(),
        receipt.total(),
    )
}

/// `Mark.new`'s private hook: the fields as `Mark::new` accepts them.
fn checked_mark(ruby: &Ruby, id: Value, amount: Value) -> Result<(u64, u64), Error> {
    let (id, amount) = (integer(id, "id")?, integer(amount, "amount")?);
    let mark = Mark::new(id, amount).ok_or_else(|| {
        Error::new(
            ruby.exception_range_error(),
            "a mark records a transfer of at least 1 right",
        )
    })?;
    Ok((mark.id(), mark.amount()))
}

/// `Transfer::Applied.new` and `Transfer::Replayed.new`'s private hook: the
/// receipt must be an `EscrowMachines::Receipt`, already valid by its own
/// constructor.
fn checked_transfer(ruby: &Ruby, receipt: Value) -> Result<Value, Error> {
    receipt_from(ruby, receipt)?;
    Ok(receipt)
}

#[magnus::wrap(class = "EscrowMachines::Escrow", free_immediately, size)]
struct Wrapper {
    core: RefCell<Core>,
}

impl Wrapper {
    /// The escrow `merge` and `recover` read: exactly an
    /// `EscrowMachines::Escrow`, a subclass instance refused with `TypeError`.
    fn partner(ruby: &Ruby, other: Value) -> Result<Obj<Self>, Error> {
        let class = classes(ruby)?.escrow.get(ruby);
        exact_typed(ruby, other, class, "expected an EscrowMachines::Escrow")
    }

    fn new(ruby: &Ruby, local: Value, allotted: Value) -> Result<Self, Error> {
        let local = integer(local, "local")?;
        let allotted = exact_array(ruby, allotted, "allotted must be an Array of Integers")?;
        let range = |message: &'static str| Error::new(ruby.exception_range_error(), message);
        if allotted.len() > CAPACITY {
            return Err(range("more replicas than EscrowMachines::CAPACITY"));
        }
        let mut shares = [0_u64; CAPACITY];
        let shares = &mut shares[..allotted.len()];
        for (index, share) in (0_isize..).zip(shares.iter_mut()) {
            *share = integer(allotted.entry(index)?, "allotted")?;
        }
        let core = Core::new(local, shares).map_err(|error| {
            range(match error {
                ConfigError::NoReplicas => "allotted must name at least one replica",
                ConfigError::TooManyReplicas => "more replicas than EscrowMachines::CAPACITY",
                ConfigError::UnknownReplica => "local must index into allotted",
                ConfigError::SupplyOverflow => "allotted must add up to at most 2**64-1",
            })
        })?;
        Ok(Self {
            core: RefCell::new(core),
        })
    }

    fn decode(ruby: &Ruby, bytes: RString) -> Result<Self, Error> {
        let bytes = Bytes::try_convert(bytes.as_value())?;
        let core = Core::decode(&bytes).map_err(|error| {
            Error::new(
                ruby.exception_range_error(),
                format!("not a decodable escrow frame: {error}"),
            )
        })?;
        Ok(Self {
            core: RefCell::new(core),
        })
    }

    /// The frame `Escrow::encode` writes. Its buffer is `ENCODED_LEN` long,
    /// which every escrow fits; an encode that still refused would answer the
    /// empty string, a frame `decode` raises on rather than one that restores
    /// the wrong state.
    fn encode(ruby: &Ruby, this: &Self) -> RString {
        let mut frame = [0_u8; Core::ENCODED_LEN];
        let written = this
            .core
            .borrow()
            .encode(&mut frame)
            .unwrap_or_else(|error| {
                debug_assert!(false, "a frame of ENCODED_LEN takes every escrow: {error}");
                0
            });
        ruby.str_from_slice(&frame[..written])
    }

    fn amount_or_refusal(ruby: &Ruby, outcome: Result<u64, Refusal>) -> Value {
        match outcome {
            Ok(left) => ruby.into_value(left),
            Err(refused) => symbol(ruby, refusal(refused)).as_value(),
        }
    }

    fn spend(ruby: &Ruby, this: Obj<Self>, amount: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let amount = integer(amount, "amount")?;
        let outcome = this.core.borrow_mut().spend(amount);
        Ok(Self::amount_or_refusal(ruby, outcome))
    }

    fn refund(ruby: &Ruby, this: Obj<Self>, amount: Value) -> Result<Value, Error> {
        this.check_frozen()?;
        let amount = integer(amount, "amount")?;
        let outcome = this.core.borrow_mut().refund(amount);
        Ok(Self::amount_or_refusal(ruby, outcome))
    }

    fn transfer(
        ruby: &Ruby,
        this: Obj<Self>,
        to: Value,
        id: Value,
        amount: Value,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        let (to, id, amount) = (
            integer(to, "to")?,
            integer(id, "id")?,
            integer(amount, "amount")?,
        );
        // The classes were resolved at init, and the answer is built from a
        // copy kept only once it exists.
        let classes = classes(ruby)?;
        keep(
            ruby,
            this,
            &this.core,
            |core| core.transfer(to, id, amount),
            |outcome| {
                let (variant, receipt) = match outcome {
                    Ok(Transfer::Applied(receipt)) => (classes.applied, receipt),
                    Ok(Transfer::Replayed(receipt)) => (classes.replayed, receipt),
                    Err(refused) => return Ok(symbol(ruby, refusal(refused)).as_value()),
                };
                let receipt: Value = classes.receipt.get(ruby).funcall("new", fields(receipt))?;
                variant.get(ruby).funcall("new", (receipt,))
            },
        )
    }

    fn accept(ruby: &Ruby, this: Obj<Self>, receipt: Value) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let receipt = receipt_from(ruby, receipt)?;
        let outcome = this.core.borrow_mut().accept(&receipt);
        Ok(symbol(
            ruby,
            match outcome {
                Ok(Accepted::Applied) => "applied",
                Ok(Accepted::Replayed) => "replayed",
                Err(refused) => refusal(refused),
            },
        ))
    }

    fn joined(ruby: &Ruby, outcome: Result<(), Refusal>, honoured: &'static str) -> Symbol {
        symbol(ruby, outcome.map_or_else(refusal, |()| honoured))
    }

    fn merge(ruby: &Ruby, this: Obj<Self>, other: Value) -> Result<Symbol, Error> {
        this.check_frozen()?;
        // Copied out first: `other` may be `this`.
        let other = *Self::partner(ruby, other)?.core.borrow();
        let outcome = this.core.borrow_mut().merge(&other);
        Ok(Self::joined(ruby, outcome, "merged"))
    }

    fn recover(ruby: &Ruby, this: Obj<Self>, other: Value) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let other = *Self::partner(ruby, other)?.core.borrow();
        let outcome = this.core.borrow_mut().recover(&other);
        Ok(Self::joined(ruby, outcome, "recovered"))
    }

    fn local(&self) -> u64 {
        self.core.borrow().local()
    }

    fn replicas(&self) -> usize {
        self.core.borrow().replicas()
    }

    const fn capacity(_: &Self) -> usize {
        CAPACITY
    }

    fn supply(&self) -> u64 {
        self.core.borrow().supply()
    }

    fn available(&self) -> u64 {
        self.core.borrow().available()
    }

    fn allotted(&self, replica: Value) -> Result<Option<u64>, Error> {
        let replica = integer(replica, "replica")?;
        Ok(self.core.borrow().allotted(replica))
    }

    fn balance(&self, replica: Value) -> Result<Option<i128>, Error> {
        let replica = integer(replica, "replica")?;
        Ok(self.core.borrow().balance(replica))
    }

    fn spent(&self, replica: Value) -> Result<Option<u64>, Error> {
        let replica = integer(replica, "replica")?;
        Ok(self.core.borrow().spent(replica))
    }

    fn refunded(&self, replica: Value) -> Result<Option<u64>, Error> {
        let replica = integer(replica, "replica")?;
        Ok(self.core.borrow().refunded(replica))
    }

    fn outstanding(&self, replica: Value) -> Result<Option<u64>, Error> {
        let replica = integer(replica, "replica")?;
        Ok(self.core.borrow().outstanding(replica))
    }

    fn granted(&self, from: Value, to: Value) -> Result<Option<u64>, Error> {
        let (from, to) = (integer(from, "from")?, integer(to, "to")?);
        Ok(self.core.borrow().granted(from, to))
    }

    fn last_transfer(ruby: &Ruby, this: &Self, from: Value, to: Value) -> Result<Value, Error> {
        let (from, to) = (integer(from, "from")?, integer(to, "to")?);
        let class = classes(ruby)?.mark.get(ruby);
        let Some(mark) = this.core.borrow().last_transfer(from, to) else {
            return Ok(ruby.qnil().as_value());
        };
        class.funcall("new", (mark.id(), mark.amount()))
    }
}
