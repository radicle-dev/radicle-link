// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use tokio::{select, sync::mpsc};
use tracing::{info, instrument};

#[cfg(unix)]
#[instrument(name = "signals subroutine", skip(shutdown_tx))]
pub async fn routine(shutdown_tx: mpsc::Sender<()>) -> anyhow::Result<()> {
    use tokio::signal::unix::*;

    let mut int = signal(SignalKind::interrupt())?;
    let mut quit = signal(SignalKind::quit())?;
    let mut term = signal(SignalKind::terminate())?;

    let signal = select! {
        _ = int.recv() => SignalKind::interrupt(),
        _ = quit.recv() => SignalKind::quit(),
        _ = term.recv() => SignalKind::terminate(),
    };

    info!(?signal, "received termination signal");
    let _ = shutdown_tx.try_send(());

    Ok(())
}

#[cfg(windows)]
#[instrument(name = "signals subroutine", skip(shutdown_tx))]
pub async fn routine(shutdown_tx: mpsc::Sender<()>) -> anyhow::Result<()> {
    use tokio::signal::windows::*;

    let mut br = ctrl_break()?;
    let mut c = ctrl_c()?;

    select! {
        _ = br.recv() => info!("received Break signal"),
        _ = c.recv() => info!("recieved CtrlC signal"),
    };

    let _ = shutdown_tx.try_send(());

    Ok(())
}
