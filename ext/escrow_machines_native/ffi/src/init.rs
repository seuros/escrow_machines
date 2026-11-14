//! Private native entry point, and the `Data` classes the binding builds and
//! checks, resolved once here so no call looks a constant up by name.
use crate::{CAPACITY, Core, Wrapper, checked_mark, checked_receipt, checked_transfer};
use magnus::rb_sys::AsRawValue;
use magnus::value::Opaque;
use magnus::{Error, RClass, RModule, Ruby, Value, function, method, prelude::*};
use std::sync::OnceLock;

/// The value classes `lib/escrow_machines.rb` defines before this loads.
#[derive(Clone, Copy)]
pub struct Classes {
    pub receipt: RClass,
    pub applied: RClass,
    pub replayed: RClass,
    pub mark: RClass,
}

static CLASSES: OnceLock<[Opaque<RClass>; 4]> = OnceLock::new();

/// The classes resolved at init.
pub fn classes(ruby: &Ruby) -> Result<Classes, Error> {
    let [receipt, applied, replayed, mark] = CLASSES.get().ok_or_else(|| {
        Error::new(
            ruby.exception_load_error(),
            "the escrow_machines extension is not initialised",
        )
    })?;
    Ok(Classes {
        receipt: ruby.get_inner(*receipt),
        applied: ruby.get_inner(*applied),
        replayed: ruby.get_inner(*replayed),
        mark: ruby.get_inner(*mark),
    })
}

/// Whether `value` is an instance of exactly `class`: the classes compared by
/// identity, which no Ruby method can be redefined to fake. A subclass (whose
/// constant leaks past the seal) is refused, and so is a value given a
/// singleton class.
pub fn instance_of(value: Value, class: RClass) -> bool {
    value.class().as_raw() == class.as_raw()
}

fn resolve(ruby: &Ruby, module: RModule) -> Result<[Opaque<RClass>; 4], Error> {
    let transfer: RModule = module.const_get("Transfer")?;
    let receipt: RClass = module.const_get("Receipt")?;
    let applied: RClass = transfer.const_get("Applied")?;
    let replayed: RClass = transfer.const_get("Replayed")?;
    let mark: RClass = module.const_get("Mark")?;
    // Held in a static, so pinned: `GC.compact` must not move them.
    for class in [receipt, applied, replayed, mark] {
        ruby.gc_register_mark_object(class);
    }
    Ok([receipt.into(), applied.into(), replayed.into(), mark.into()])
}

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("EscrowMachines")?;
    let resolved = resolve(ruby, module)?;
    // A second load keeps the first classes: they are the same constants.
    let _ = CLASSES.get_or_init(|| resolved);
    module.const_set("CAPACITY", CAPACITY)?;
    module.define_singleton_method("__receipt", function!(checked_receipt, 5))?;
    module.define_singleton_method("__mark", function!(checked_mark, 2))?;
    module.define_singleton_method("__transfer", function!(checked_transfer, 1))?;
    let class = module.define_class("Escrow", ruby.class_object())?;
    class.undef_default_alloc_func();
    class.const_set("ENCODED_LEN", Core::ENCODED_LEN)?;
    class.define_singleton_method("new", function!(Wrapper::new, 2))?;
    class.define_singleton_method("decode", function!(Wrapper::decode, 1))?;
    class.define_method("encode", method!(Wrapper::encode, 0))?;
    class.define_method("spend", method!(Wrapper::spend, 1))?;
    class.define_method("refund", method!(Wrapper::refund, 1))?;
    class.define_method("transfer", method!(Wrapper::transfer, 3))?;
    class.define_method("accept", method!(Wrapper::accept, 1))?;
    class.define_method("merge", method!(Wrapper::merge, 1))?;
    class.define_method("recover", method!(Wrapper::recover, 1))?;
    class.define_method("local", method!(Wrapper::local, 0))?;
    class.define_method("replicas", method!(Wrapper::replicas, 0))?;
    class.define_method("capacity", method!(Wrapper::capacity, 0))?;
    class.define_method("supply", method!(Wrapper::supply, 0))?;
    class.define_method("available", method!(Wrapper::available, 0))?;
    class.define_method("allotted", method!(Wrapper::allotted, 1))?;
    class.define_method("balance", method!(Wrapper::balance, 1))?;
    class.define_method("spent", method!(Wrapper::spent, 1))?;
    class.define_method("refunded", method!(Wrapper::refunded, 1))?;
    class.define_method("outstanding", method!(Wrapper::outstanding, 1))?;
    class.define_method("granted", method!(Wrapper::granted, 2))?;
    class.define_method("last_transfer", method!(Wrapper::last_transfer, 2))?;
    Ok(())
}
