use std::{thread, time::Duration};

fn main() {
    // simulate a hook hanging
    thread::sleep(Duration::from_secs(10));
}
