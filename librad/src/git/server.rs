use std::{io, process::Command, thread};

use libc;
use signal_hook::{iterator::Signals, SIGABRT, SIGCONT, SIGHUP, SIGINT, SIGQUIT, SIGSTOP, SIGTERM};

use crate::paths::Paths;

pub struct Daemon {
    paths: Paths,
    port: u16,
}

impl Daemon {
    pub fn new(paths: Paths, port: u16) -> Self {
        Self { paths, port }
    }

    pub fn run(self) -> Result<(), io::Error> {
        let t = thread::spawn(move || {
            let signals =
                Signals::new(&[SIGABRT, SIGCONT, SIGHUP, SIGINT, SIGQUIT, SIGSTOP, SIGTERM])?;
            let mut child = Command::new("git")
                .arg("daemon")
                .arg("--reuseaddr")
                .arg(format!("--port={}", self.port))
                .arg("--export-all")
                .arg("--log-destination=stderr")
                .arg(format!(
                    "--base-path={}",
                    self.paths.projects_dir().display()
                ))
                .arg(self.paths.projects_dir())
                .spawn()?;

            loop {
                for signal in signals.pending() {
                    unsafe { libc::kill(child.id() as i32, signal) };
                }

                if child.try_wait()?.is_some() {
                    return Ok(());
                }

                thread::yield_now()
            }
        });

        t.join().expect("Panic in child thread")
    }
}
