#[macro_use]
extern crate failure_derive;
#[macro_use]
extern crate lazy_static;

pub use radicle_surf as surf;

pub mod git;
pub mod keys;
pub mod meta;
pub mod p2p;
pub mod paths;
pub mod peer;
pub mod project;

pub fn init() -> bool {
    sodiumoxide::init().is_ok()
}
