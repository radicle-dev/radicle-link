use std::{
    env,
    fs,
    io::{self, Write as _},
};

use link_hooks::{hook::HookMessage, Data};
use radicle_git_ext::Oid;

type Message = HookMessage<Data<Oid>>;

fn main() {
    let mut args = env::args();
    let _ = args.next();
    let out = args.next().expect("expected output path");
    let mut file = fs::File::create(out).unwrap();

    let mut buffer = String::new();
    let stdin = io::stdin();
    let mut eot = false;

    while !eot {
        stdin.read_line(&mut buffer).unwrap();
        match buffer.parse::<Message>().unwrap() {
            HookMessage::EOT => {
                eot = true;
            },
            HookMessage::Payload(data) => file.write_all(format!("{}", data).as_bytes()).unwrap(),
        }
        buffer.clear();
    }
}
