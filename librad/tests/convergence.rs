// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use std::time::Duration;

use tokio::time::timeout;

use librad_test::{logging, rad::testnet};

#[tokio::test]
async fn converge() -> Result<(), Box<dyn std::error::Error>> {
    logging::init();

    for num_peers in 1..10 {
        let peers = testnet::setup(num_peers).await?;
        timeout(
            Duration::from_secs(10),
            testnet::run_on_testnet(peers, num_peers, |mut _apis| async move {
                Ok::<(), Box<dyn std::error::Error>>(())
            }),
        )
        .await??;
    }

    Ok(())
}
