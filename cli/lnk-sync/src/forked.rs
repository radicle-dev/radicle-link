use std::collections::BTreeSet;

use librad::git::{identities::project::heads, storage::ReadOnlyStorage};

/// A nicely formatted error message describing the forks in a forked project
pub struct ForkError(Vec<ForkDescription>);

impl ForkError {
    pub(crate) fn from_forked<S>(storage: &S, forked: BTreeSet<heads::Fork>) -> Self
    where
        S: ReadOnlyStorage,
    {
        ForkError(
            forked
                .into_iter()
                .map(|f| ForkDescription::from_fork(storage, f))
                .collect(),
        )
    }
}

impl std::fmt::Display for ForkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "the delegates for this project have forked")?;
        writeln!(f, "you must choose a specific peer to clone")?;
        writeln!(f, "you can do this using the --peer <peer id> argument")?;
        writeln!(f, "and one of the peers listed below")?;
        writeln!(f)?;
        writeln!(f, "There are {} different forks", self.0.len())?;
        writeln!(f)?;
        for fork in &self.0 {
            fork.fmt(f)?;
            writeln!(f)?;
        }
        Ok(())
    }
}

struct ForkDescription {
    fork: heads::Fork,
    tip_commit_message: Option<String>,
}

impl ForkDescription {
    fn from_fork<S>(storage: &S, fork: heads::Fork) -> Self
    where
        S: ReadOnlyStorage,
    {
        let tip = std::rc::Rc::new(fork.tip);
        let tip_commit_message = storage
            .find_object(&tip)
            .ok()
            .and_then(|o| o.and_then(|o| o.as_commit().map(|c| c.summary().map(|m| m.to_string()))))
            .unwrap_or(None);
        ForkDescription {
            fork,
            tip_commit_message,
        }
    }
}

impl std::fmt::Display for ForkDescription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "{} peers pointing at {}",
            self.fork.peers.len(),
            self.fork.tip
        )?;
        match &self.tip_commit_message {
            Some(m) => {
                writeln!(f, "Commit message:")?;
                writeln!(f, "    {}", m)?;
            },
            None => {
                writeln!(f)?;
                writeln!(f, "unable to determine commit message")?;
                writeln!(f)?;
            },
        }
        writeln!(f, "Peers:")?;
        for peer in &self.fork.peers {
            writeln!(f, "    {}", peer)?;
        }
        Ok(())
    }
}
