use std::{marker::PhantomData, path::PathBuf};

use librad::{
    keys::{self, storage::Pinentry},
    paths::Paths,
};
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
pub struct Config<K, P>
where
    K: keys::Storage<P>,
    P: Pinentry,
{
    pub verbose: bool,
    pub paths: Paths,
    pub keystore: K,

    _marker: PhantomData<P>,
}

impl CommonOpts {
    pub fn into_config<F, K, P>(self, init_keystore: F) -> Result<Config<K, P>, Error<P::Error>>
    where
        F: FnOnce(&Paths) -> K,
        K: keys::Storage<P>,
        P: Pinentry,
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
            _marker: PhantomData,
        })
    }
}
