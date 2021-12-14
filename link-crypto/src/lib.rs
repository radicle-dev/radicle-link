// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

#![allow(rustdoc::private_intra_doc_links)]
#![warn(clippy::extra_unused_lifetimes)]
#![deny(rustdoc::broken_intra_doc_links)]

#[macro_use]
extern crate async_trait;

pub extern crate radicle_git_ext as git_ext;
pub extern crate radicle_keystore as keystore;

mod keys;
pub use keys::{
    IntoSecretKeyError,
    PublicKey,
    SecStr,
    SecretKey,
    SignError,
    Signature,
    PUBLICKEYBYTES,
};

pub mod peer;
pub use peer::PeerId;

mod signer;
pub use signer::{BoxedSignError, BoxedSigner, Signer, SomeSigner};
