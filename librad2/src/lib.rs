extern crate sequoia_openpgp as pgp;
extern crate sodiumoxide;

pub mod keys;
pub mod paths;

pub fn init() -> bool {
    sodiumoxide::init().is_ok()
}
