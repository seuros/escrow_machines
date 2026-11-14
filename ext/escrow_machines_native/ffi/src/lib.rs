//! Thin binding to the bounded-counter escrow.
mod init;

use init::{classes, instance_of};

use bytes::Bytes;
use escrow_machines::{Accepted, ConfigError, Escrow, Mark, Receipt, Refusal, Transfer};
use magnus::{
    Error, Integer, RArray, RString, Ruby, Symbol, TryConvert, Value, prelude::*, typed_data::Obj,
};
use std::cell::RefCell;

/// Replicas one compiled escrow can hold.
const CAPACITY: usize = 8;

type Core = Escrow<CAPACITY>;

#[derive(Clone, Copy)]
struct Number(u64);

impl TryConvert for Number {
    fn try_convert(value: Value) -> Result<Self, Error> {
        if Integer::from_value(value).is_none() {
            return Err(Error::new(
                Ruby::get_with(value).exception_type_error(),
                "expected an Integer (unsigned 64-bit)",
            ));
        }
        u64::try_convert(value).map(Self)
    }
}

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
    if !instance_of(value, classes(ruby)?.receipt) {
        return Err(Error::new(
            ruby.exception_type_error(),
            "expected an EscrowMachines::Receipt",
        ));
    }
    let field = |name: &str| value.funcall::<_, _, Number>(name, ()).map(|n| n.0);
    let (from, to, id) = (field("from")?, field("to")?, field("id")?);
    let (amount, total) = (field("amount")?, field("total")?);
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
    from: Number,
    to: Number,
    id: Number,
    amount: Number,
    total: Number,
) -> Result<(u64, u64, u64, u64, u64), Error> {
    let receipt = Receipt::new(from.0, to.0, id.0, amount.0, total.0)
        .ok_or_else(|| inconsistent_receipt(ruby))?;
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
fn checked_mark(ruby: &Ruby, id: Number, amount: Number) -> Result<(u64, u64), Error> {
    let mark = Mark::new(id.0, amount.0).ok_or_else(|| {
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

/// Runs `act` on a copy of the escrow in `cell` and keeps the copy only once
/// `act`, which builds the answer and may run Ruby, has returned. A raise
/// while building leaves the escrow as it was, so no work is lost. Right
/// before the store the receiver is checked again: frozen meanwhile raises
/// `FrozenError`, changed meanwhile (a redefined `new` calling back in)
/// raises `RuntimeError`, and in both cases the copy is not applied.
fn on_copy<R>(
    ruby: &Ruby,
    this: Obj<Wrapper>,
    act: impl FnOnce(&mut Core) -> Result<R, Error>,
) -> Result<R, Error> {
    this.check_frozen()?;
    let before = *this.core.borrow();
    let mut after = before;
    let answer = act(&mut after)?;
    this.check_frozen()?;
    let mut held = this.core.borrow_mut();
    if *held != before {
        return Err(Error::new(
            ruby.exception_runtime_error(),
            "the escrow changed while the answer was built; the call was not applied",
        ));
    }
    *held = after;
    Ok(answer)
}

#[magnus::wrap(class = "EscrowMachines::Escrow", free_immediately, size)]
struct Wrapper {
    core: RefCell<Core>,
}

impl Wrapper {
    fn new(ruby: &Ruby, local: Number, allotted: RArray) -> Result<Self, Error> {
        let range = |message: &'static str| Error::new(ruby.exception_range_error(), message);
        if allotted.len() > CAPACITY {
            return Err(range("more replicas than EscrowMachines::CAPACITY"));
        }
        let mut shares = [0_u64; CAPACITY];
        let shares = &mut shares[..allotted.len()];
        for (index, share) in (0_isize..).zip(shares.iter_mut()) {
            *share = allotted.entry::<Number>(index)?.0;
        }
        let core = Core::new(local.0, shares).map_err(|error| {
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
            Err(refused) => ruby.to_symbol(refusal(refused)).as_value(),
        }
    }

    fn spend(ruby: &Ruby, this: Obj<Self>, amount: Number) -> Result<Value, Error> {
        this.check_frozen()?;
        let outcome = this.core.borrow_mut().spend(amount.0);
        Ok(Self::amount_or_refusal(ruby, outcome))
    }

    fn refund(ruby: &Ruby, this: Obj<Self>, amount: Number) -> Result<Value, Error> {
        this.check_frozen()?;
        let outcome = this.core.borrow_mut().refund(amount.0);
        Ok(Self::amount_or_refusal(ruby, outcome))
    }

    fn transfer(
        ruby: &Ruby,
        this: Obj<Self>,
        to: Number,
        id: Number,
        amount: Number,
    ) -> Result<Value, Error> {
        this.check_frozen()?;
        // The classes were resolved at init, and the answer is built from a
        // copy kept only once it exists.
        let classes = classes(ruby)?;
        on_copy(ruby, this, |core| {
            let (variant, receipt) = match core.transfer(to.0, id.0, amount.0) {
                Ok(Transfer::Applied(receipt)) => (classes.applied, receipt),
                Ok(Transfer::Replayed(receipt)) => (classes.replayed, receipt),
                Err(refused) => return Ok(ruby.to_symbol(refusal(refused)).as_value()),
            };
            let receipt: Value = classes.receipt.funcall("new", fields(receipt))?;
            variant.funcall("new", (receipt,))
        })
    }

    fn accept(ruby: &Ruby, this: Obj<Self>, receipt: Value) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let receipt = receipt_from(ruby, receipt)?;
        // Reading the receipt ran its readers, which may have frozen us.
        this.check_frozen()?;
        let outcome = this.core.borrow_mut().accept(&receipt);
        Ok(ruby.to_symbol(match outcome {
            Ok(Accepted::Applied) => "applied",
            Ok(Accepted::Replayed) => "replayed",
            Err(refused) => refusal(refused),
        }))
    }

    fn joined(ruby: &Ruby, outcome: Result<(), Refusal>, honoured: &'static str) -> Symbol {
        ruby.to_symbol(outcome.map_or_else(refusal, |()| honoured))
    }

    fn merge(ruby: &Ruby, this: Obj<Self>, other: Obj<Self>) -> Result<Symbol, Error> {
        this.check_frozen()?;
        // Copied out first: `other` may be `this`.
        let other = *other.core.borrow();
        let outcome = this.core.borrow_mut().merge(&other);
        Ok(Self::joined(ruby, outcome, "merged"))
    }

    fn recover(ruby: &Ruby, this: Obj<Self>, other: Obj<Self>) -> Result<Symbol, Error> {
        this.check_frozen()?;
        let other = *other.core.borrow();
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

    fn allotted(&self, replica: Number) -> Option<u64> {
        self.core.borrow().allotted(replica.0)
    }

    fn balance(&self, replica: Number) -> Option<i128> {
        self.core.borrow().balance(replica.0)
    }

    fn spent(&self, replica: Number) -> Option<u64> {
        self.core.borrow().spent(replica.0)
    }

    fn refunded(&self, replica: Number) -> Option<u64> {
        self.core.borrow().refunded(replica.0)
    }

    fn outstanding(&self, replica: Number) -> Option<u64> {
        self.core.borrow().outstanding(replica.0)
    }

    fn granted(&self, from: Number, to: Number) -> Option<u64> {
        self.core.borrow().granted(from.0, to.0)
    }

    fn last_transfer(ruby: &Ruby, this: &Self, from: Number, to: Number) -> Result<Value, Error> {
        let class = classes(ruby)?.mark;
        let Some(mark) = this.core.borrow().last_transfer(from.0, to.0) else {
            return Ok(ruby.qnil().as_value());
        };
        class.funcall("new", (mark.id(), mark.amount()))
    }
}
