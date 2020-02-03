use std::{fmt::Debug, path::PathBuf};

use librad::paths::Paths;
use radicle_keystore as keystore;
use structopt::StructOpt;

use crate::error::Error;

/// Common options
#[derive(StructOpt)]
pub struct CommonOpts {
    /// Verbose output
    #[structopt(short, long)]
    pub verbose: bool,

    /// Override the default, platform-specific configuration and state
    /// directory root
    ///
    /// Most useful for local testing with multiple identities.
    #[structopt(long, env = "RAD_ROOT", parse(from_os_str))]
    pub paths: Option<PathBuf>,
}

/// Stateful configuration, derived from [`CommonOpts`] and passed around to
/// commands.
pub struct Config<K>
where
    K: keystore::Storage,
{
    pub verbose: bool,
    pub paths: Paths,
    pub keystore: K,
}

impl CommonOpts {
    pub fn into_config<F, K>(self, init_keystore: F) -> Result<Config<K>, Error<K::Error>>
    where
        F: FnOnce(&Paths) -> K,
        K: keystore::Storage,
        K::Error: Debug + Send + Sync,
    {
        let verbose = self.verbose;
        let paths = if let Some(root) = self.paths {
            Paths::from_root(&root)
        } else {
            Paths::new()
        }?;
        let keystore = init_keystore(&paths);

        Ok(Config {
            verbose,
            paths,
            keystore,
        })
    }
}
