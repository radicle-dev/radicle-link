// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use super::io::recv::interrogation::Cache;

#[derive(Clone, Default)]
pub struct Caches {
    pub interrogation: Cache,
}
