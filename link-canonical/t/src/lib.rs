#[cfg(test)]
#[macro_use]
extern crate link_canonical;

#[cfg(any(test, feature = "test"))]
pub mod gen;

#[cfg(test)]
mod properties;
#[cfg(test)]
mod tests;
