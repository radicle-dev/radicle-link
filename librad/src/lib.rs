#![feature(str_strip)]

extern crate radicle_keystore as keystore;
extern crate sequoia_openpgp as pgp;
extern crate sodiumoxide;
#[macro_use]
extern crate failure_derive;
#[macro_use]
extern crate lazy_static;

pub use radicle_surf as surf;

pub mod git;
pub mod keys;
pub mod meta;
pub mod net;
pub mod paths;
pub mod peer;
pub mod project;

pub(crate) mod internal;

pub fn init() -> bool {
    sodiumoxide::init().is_ok()
}
