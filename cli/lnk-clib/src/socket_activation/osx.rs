use std::collections::HashMap;

use super::Socket;

#[derive(thiserror::Error, Debug)]
pub enum Error {}

pub fn env_sockets() -> Result<Option<HashMap<String, Socket>>, Error> {
    todo!()
}
