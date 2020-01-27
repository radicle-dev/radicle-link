use std::{
    future::Future,
    io,
    pin::Pin,
    process::{Child, Command, Stdio},
    task::{Context, Poll},
};

use libc;
use log::{debug, trace};
use signal_hook::{iterator::Signals, SIGCONT, SIGHUP, SIGINT, SIGKILL, SIGQUIT, SIGTERM};

use crate::paths::Paths;

pub struct Daemon {
    paths: Paths,
    port: u16,
    signals: Signals,
    child: Option<Child>,
}

// Belts + Suspenders
impl Drop for Daemon {
    fn drop(&mut self) {
        if let Some(child) = &self.child {
            kill(&child, SIGKILL)
        }
    }
}

impl Daemon {
    pub fn new(paths: &Paths, port: u16) -> Result<Self, io::Error> {
        let signals = Signals::new(&[SIGCONT, SIGHUP, SIGINT, SIGQUIT, SIGTERM])?;

        Ok(Self {
            paths: paths.clone(),
            port,
            signals,
            child: None,
        })
    }

    pub fn run(mut self) -> Result<Self, io::Error> {
        let child = Command::new("git")
            .arg("daemon")
            .arg(format!("--port={}", self.port))
            .arg("--reuseaddr")
            .arg("--export-all")
            .arg("--log-destination=stderr")
            .arg("--verbose")
            .arg(format!(
                "--base-path={}",
                self.paths.projects_dir().display()
            ))
            .arg(self.paths.projects_dir())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;

        debug!("spawned git daemon");

        self.child = Some(child);
        Ok(self)
    }
}

impl Future for Daemon {
    type Output = Result<(), io::Error>;

    fn poll(self: Pin<&mut Self>, _: &mut Context) -> Poll<Self::Output> {
        trace!("polling git daemon");

        let unpin = Pin::into_inner(self);
        let signals = &unpin.signals;
        if let Some(child) = &mut unpin.child {
            // FIXME: There's an async version of the signals iterator, but it
            // uses futures 0.1. Figure out that compat layer.
            for signal in signals.forever() {
                debug!("got signal {}", signal);
                kill(&child, signal);
                break;
            }

            match child.wait() {
                Err(e) => Poll::Ready(Err(e)),
                Ok(status) => {
                    debug!("git daemon exited with {}", status);
                    unpin.child = None;
                    Poll::Ready(Ok(()))
                },
            }
        } else {
            trace!("poll without run");
            Poll::Ready(Ok(()))
        }
    }
}

fn kill(child: &Child, signal: i32) {
    trace!("sending signal {} to child", signal);
    unsafe { libc::kill(child.id() as i32, signal) };
}
