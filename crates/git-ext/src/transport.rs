// Copyright © 2019-2020 The Radicle Foundation <hello@radicle.foundation>
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

pub const UPLOAD_PACK_HEADER: &[u8] = b"001e# service=git-upload-pack\n0000";
pub const RECEIVE_PACK_HEADER: &[u8] = b"001f# service=git-receive-pack\n0000";
