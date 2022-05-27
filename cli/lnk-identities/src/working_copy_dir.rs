use std::path::{Path, PathBuf};

/// Where to checkout or create an identity
pub enum WorkingCopyDir {
    /// A directory within this directory named after the identity
    Within(PathBuf),
    /// Directly at the given path, which must be a directory
    At(PathBuf),
}

impl WorkingCopyDir {
    /// If `at` is `Some` then return `CheckoutPath::At(at)`, otherwise
    /// `CheckoutPath::Within(current directory)`.
    pub fn at_or_current_dir<P: AsRef<Path>>(
        at: Option<P>,
    ) -> Result<WorkingCopyDir, std::io::Error> {
        match at {
            Some(p) => Ok(WorkingCopyDir::At(p.as_ref().to_path_buf())),
            None => Ok(WorkingCopyDir::Within(std::env::current_dir()?)),
        }
    }
}

impl std::fmt::Display for WorkingCopyDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkingCopyDir::At(p) => p.display().fmt(f),
            WorkingCopyDir::Within(p) => write!(f, "{}/<name>", p.display()),
        }
    }
}

impl WorkingCopyDir {
    pub(crate) fn resolve(&self, identity_name: &str) -> PathBuf {
        match self {
            Self::At(p) => p.clone(),
            Self::Within(p) => p.join(identity_name),
        }
    }
}
