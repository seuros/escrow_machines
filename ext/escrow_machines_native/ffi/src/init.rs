//! Private native entry point.
use crate::{CAPACITY, Core, Wrapper};
use magnus::{Error, Ruby, function, method, prelude::*};

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("EscrowMachines")?;
    module.const_set("CAPACITY", CAPACITY)?;
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
