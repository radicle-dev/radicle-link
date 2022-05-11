// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use clap::Parser;
use librad::git::Urn;

use crate::Mode;

#[derive(Clone, Debug, Parser)]
pub struct Args {
    #[clap(long)]
    pub urn: Urn,
    #[clap(long, default_value_t)]
    pub mode: Mode,
}
