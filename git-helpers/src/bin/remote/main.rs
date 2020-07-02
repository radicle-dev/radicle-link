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

#![feature(str_strip)]

use std::{
    env,
    io::{self, BufRead, BufReader, Write},
    path::PathBuf,
    thread,
};

use librad::{
    git::local::transport::{LocalTransport, Mode::Stateful, Settings},
    keys::PublicKey,
    paths::Paths,
};
use radicle_git_helpers::credential;
use radicle_keystore::{crypto::Pwhash, FileStorage, Keystore};

fn main() -> anyhow::Result<()> {
    let url = {
        let args = env::args().skip(1).take(2).collect::<Vec<_>>();
        args[0]
            .parse()
            .or_else(|_| args[1].parse())
            .or_else(|_| Err(anyhow::anyhow!("invalid args: {:?}", args)))
    }?;

    let git_dir = env::var("GIT_DIR").map(PathBuf::from)?;

    let transport = {
        let pass = credential::Git::new(&git_dir).get(&url)?;
        let paths = Paths::from_env()?;
        let keystore = FileStorage::<_, PublicKey, _, _>::new(
            &paths.keys_dir().join("librad.key"),
            Pwhash::new(pass),
        );
        let key = keystore.get_key().map(|keypair| keypair.secret_key)?;

        LocalTransport::new(Settings { paths, signer: key })
    }?;

    loop {
        let mut buf = String::with_capacity(32);
        io::stdin().read_line(&mut buf)?;
        let line = buf.trim();

        if line == "capabilities" {
            println!("connect\n\n");
            continue;
        }

        if let Some(service) = line.strip_prefix("connect ") {
            let service = match service {
                "git-upload-pack" => Ok(git2::transport::Service::UploadPack),
                "git-receive-pack" => Ok(git2::transport::Service::ReceivePack),
                unknown => Err(anyhow::anyhow!("unknown service: {}", unknown)),
            }?;

            let (read, mut write) = transport.connect(url, service, Stateful)?.split();

            // ack ok
            println!();

            thread::spawn(move || {
                // For the ways of the IOs are inscrutable, thou shallt not
                // simply `io::copy` from the child's stdout to stdout. Instead,
                // a `pkt-line` flush packet shallt conjure a flush on the
                // handle -- or else thou shallt block in eternity!
                let t1: thread::JoinHandle<io::Result<()>> = thread::spawn(move || {
                    let mut read = BufReader::new(read);
                    let mut stdout = io::stdout();
                    loop {
                        let bytes = read.fill_buf()?;
                        let len = bytes.len();
                        let flush = bytes.ends_with(b"0000");

                        stdout.write_all(bytes)?;
                        read.consume(len);

                        if flush {
                            stdout.flush()?;
                        }

                        if len == 0 {
                            stdout.flush()?;
                            break;
                        }
                    }

                    Ok(())
                });
                let t2: thread::JoinHandle<io::Result<()>> =
                    thread::spawn(move || io::copy(&mut io::stdin(), &mut write).and(Ok(())));

                t1.join()
                    .expect("child read->stdout panicked")
                    .and_then(|()| t2.join().expect("stdin->child write panicked"))
            })
            .join()
            .expect("IO pipe panicked")?;

            break;
        }

        return Err(anyhow::anyhow!("unexpected command: {}", line));
    }

    Ok(())
}
