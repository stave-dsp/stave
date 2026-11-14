//! The built-in commands, one module per family.

pub mod bench;
pub mod characterise;
pub mod corpus;
pub mod fit;
pub mod instance;
pub mod listen;
pub mod validate;

use crate::registry::Registry;

/// Register every built-in command, in the order help lists them.
pub fn register(r: &mut Registry) {
    r.register_fn(instance::DESCRIBE, instance::describe)
        .register_fn(instance::PARAMS, instance::params)
        .register_fn(instance::PRESETS, instance::presets)
        .register_fn(instance::STAGES, instance::stages)
        .register_fn(instance::OPEN, instance::open)
        .register_fn(instance::SET_PARAMS, instance::set)
        .register_fn(instance::CLOSE, instance::close)
        .register_fn(bench::PROCESS, bench::process)
        .register_fn(bench::BENCH, bench::bench)
        .register_fn(validate::VALIDATE, validate::validate)
        .register_fn(validate::RESPONSE, validate::response)
        .register_fn(listen::ADD, listen::add)
        .register_fn(listen::LIST, listen::list)
        .register_fn(listen::NEXT, listen::next)
        .register_fn(listen::ANSWER, listen::answer)
        .register_fn(characterise::CHARACTERISE, characterise::characterise)
        .register_fn(fit::STIMULUS, fit::stimulus)
        .register_fn(fit::FIT, fit::fit)
        .register_fn(corpus::LIST, corpus::list)
        .register_fn(corpus::CHECK, corpus::check);
}

#[cfg(test)]
mod tests;
