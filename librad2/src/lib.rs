extern crate sequoia_openpgp as pgp;
extern crate sodiumoxide;
#[macro_use]
extern crate failure_derive;

pub mod keys;
pub mod meta;
pub mod paths;
pub mod peer;

pub fn init() -> bool {
    sodiumoxide::init().is_ok()
}
