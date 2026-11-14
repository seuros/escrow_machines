//! Private native entry point, and the `Data` classes the binding builds and
//! checks, resolved once here so no call looks a constant up by name.
use crate::{CAPACITY, Core, Wrapper, checked_mark, checked_receipt, checked_transfer};
use magnus::{Error, RClass, RModule, Ruby, function, method, prelude::*};
use matryona_machines::ruby::{Class, Registry};

/// The value classes `lib/escrow_machines.rb` defines before this loads.
pub struct Classes {
    pub receipt: Class,
    pub applied: Class,
    pub replayed: Class,
    pub mark: Class,
    pub escrow: Class,
}

static CLASSES: Registry<Classes> = Registry::new("escrow_machines");

/// The classes resolved at init.
pub fn classes(ruby: &Ruby) -> Result<&'static Classes, Error> {
    CLASSES.get(ruby)
}

/// Resolves and pins the value classes, and pins `escrow`, which init
/// defined. Called once, from init.
fn resolve(ruby: &Ruby, module: RModule, escrow: RClass) -> Result<(), Error> {
    let transfer: RModule = module.const_get("Transfer")?;
    let classes = Classes {
        receipt: Class::resolve(ruby, module, "Receipt")?,
        applied: Class::resolve(ruby, transfer, "Applied")?,
        replayed: Class::resolve(ruby, transfer, "Replayed")?,
        mark: Class::resolve(ruby, module, "Mark")?,
        escrow: Class::pin(ruby, escrow),
    };
    CLASSES.set(ruby, classes).map(drop)
}

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("EscrowMachines")?;
    module.const_set("CAPACITY", CAPACITY)?;
    module.define_singleton_method("__receipt", function!(checked_receipt, 5))?;
    module.define_singleton_method("__mark", function!(checked_mark, 2))?;
    module.define_singleton_method("__transfer", function!(checked_transfer, 1))?;
    let class = module.define_class("Escrow", ruby.class_object())?;
    class.undef_default_alloc_func();
    resolve(ruby, module, class)?;
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
