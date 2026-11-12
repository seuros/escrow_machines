//! Thin binding to the bounded-counter escrow.
mod init;

use bytes::Bytes;
use escrow_machines::{Accepted, ConfigError, Escrow, Receipt, Refusal};
use magnus::scan_args::{get_kwargs, scan_args};
use magnus::{
    Error, Integer, RArray, RClass, RHash, RModule, RString, Ruby, Symbol, TryConvert, Value,
    prelude::*, typed_data::Obj,
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

fn receipt_class(ruby: &Ruby) -> Result<RClass, Error> {
    let module: RModule = ruby.class_object().const_get("EscrowMachines")?;
    module.const_get("Receipt")
}

fn receipt_from(ruby: &Ruby, value: Value) -> Result<Receipt, Error> {
    if !value.is_kind_of(receipt_class(ruby)?) {
        return Err(Error::new(
            ruby.exception_type_error(),
            "expected an EscrowMachines::Receipt",
        ));
    }
    let field = |name: &str| value.funcall::<_, _, Number>(name, ()).map(|n| n.0);
    Ok(Receipt {
        from: field("from")?,
        to: field("to")?,
        id: field("id")?,
        amount: field("amount")?,
        total: field("total")?,
    })
}

#[magnus::wrap(class = "EscrowMachines::Escrow", free_immediately, size)]
struct Wrapper {
    core: RefCell<Core>,
}

impl Wrapper {
    fn new(ruby: &Ruby, args: &[Value]) -> Result<Self, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let (replica, rights) =
            get_kwargs::<_, (Number, RArray), (), ()>(args.keywords, &["replica", "rights"], &[])?
                .required;
        let range = |message: &'static str| Error::new(ruby.exception_range_error(), message);
        if rights.len() > CAPACITY {
            return Err(range("more replicas than EscrowMachines::CAPACITY"));
        }
        let mut allotted = [0_u64; CAPACITY];
        let allotted = &mut allotted[..rights.len()];
        for (index, share) in (0_isize..).zip(allotted.iter_mut()) {
            *share = rights.entry::<Number>(index)?.0;
        }
        let core = Core::new(replica.0, allotted).map_err(|error| {
            range(match error {
                ConfigError::NoReplicas => "rights must name at least one replica",
                ConfigError::TooManyReplicas => "more replicas than EscrowMachines::CAPACITY",
                ConfigError::UnknownReplica => "replica must index into rights",
                ConfigError::SupplyOverflow => "rights must add up to at most 2**64-1",
            })
        })?;
        Ok(Self {
            core: RefCell::new(core),
        })
    }

    fn load(ruby: &Ruby, frame: RString) -> Result<Self, Error> {
        let frame = Bytes::try_convert(frame.as_value())?;
        let core = Core::decode(&frame).map_err(|error| {
            Error::new(
                ruby.exception_arg_error(),
                format!("not a loadable escrow frame: {error}"),
            )
        })?;
        Ok(Self {
            core: RefCell::new(core),
        })
    }

    fn dump(ruby: &Ruby, this: &Self) -> Result<RString, Error> {
        let mut frame = [0_u8; Core::ENCODED_LEN];
        let written = this.core.borrow().encode(&mut frame).map_err(|error| {
            Error::new(ruby.exception_runtime_error(), format!("encode: {error}"))
        })?;
        Ok(ruby.str_from_slice(&frame[..written]))
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

    fn transfer(ruby: &Ruby, this: Obj<Self>, args: &[Value]) -> Result<Value, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let (to, id, amount) = get_kwargs::<_, (Number, Number, Number), (), ()>(
            args.keywords,
            &["to", "id", "amount"],
            &[],
        )?
        .required;
        this.check_frozen()?;
        let outcome = this.core.borrow_mut().transfer(to.0, id.0, amount.0);
        match outcome {
            Ok(transfer) => {
                let Receipt {
                    from,
                    to,
                    id,
                    amount,
                    total,
                } = transfer.receipt();
                receipt_class(ruby)?.funcall("new", (from, to, id, amount, total))
            }
            Err(refused) => Ok(ruby.to_symbol(refusal(refused)).as_value()),
        }
    }

    fn accept(ruby: &Ruby, this: Obj<Self>, receipt: Value) -> Result<Symbol, Error> {
        let receipt = receipt_from(ruby, receipt)?;
        this.check_frozen()?;
        let outcome = this.core.borrow_mut().accept(&receipt);
        Ok(ruby.to_symbol(match outcome {
            Ok(Accepted::Applied) => "applied",
            Ok(Accepted::Replayed) => "replayed",
            Err(refused) => refusal(refused),
        }))
    }

    fn joined(ruby: &Ruby, outcome: Result<(), Refusal>) -> Value {
        match outcome {
            Ok(()) => ruby.qtrue().as_value(),
            Err(refused) => ruby.to_symbol(refusal(refused)).as_value(),
        }
    }

    fn merge(ruby: &Ruby, this: Obj<Self>, other: Obj<Self>) -> Result<Value, Error> {
        this.check_frozen()?;
        // Copied out first: `other` may be `this`.
        let other = *other.core.borrow();
        let outcome = this.core.borrow_mut().merge(&other);
        Ok(Self::joined(ruby, outcome))
    }

    fn recover(ruby: &Ruby, this: Obj<Self>, other: Obj<Self>) -> Result<Value, Error> {
        this.check_frozen()?;
        let other = *other.core.borrow();
        let outcome = this.core.borrow_mut().recover(&other);
        Ok(Self::joined(ruby, outcome))
    }

    fn replica(&self) -> u64 {
        self.core.borrow().local()
    }

    fn replicas(&self) -> usize {
        self.core.borrow().replicas()
    }

    #[allow(clippy::unused_self)]
    const fn capacity(&self) -> usize {
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

    fn last_transfer(&self, from: Number, to: Number) -> Option<(u64, u64)> {
        let mark = self.core.borrow().last_transfer(from.0, to.0)?;
        Some((mark.id, mark.amount))
    }
}
