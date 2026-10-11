use std::{io, path::PathBuf};

use url::Url;

/// Synchronous test loads never access the network or files outside this root.
/// Canonicalization also prevents encoded traversal and symlink escapes.
#[derive(Clone, Debug, Default)]
pub(crate) enum ResourcePolicy {
    #[default]
    Browser,
    Local(PathBuf),
}

impl ResourcePolicy {
    pub fn check(&self, url: &Url) -> io::Result<()> {
        let Self::Local(root) = self else {
            return Ok(());
        };
        if url.scheme() != "file" || url.query().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("headless local-only URL required (no query): {url}"),
            ));
        }
        let path = url
            .to_file_path()
            .map_err(|_| io::Error::other("invalid local file URL"))?
            .canonicalize()?;
        if !path.starts_with(root) || !path.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("headless resource escapes root or is not a file: {url}"),
            ));
        }
        Ok(())
    }

    pub fn local(&self) -> bool {
        matches!(self, Self::Local(_))
    }
}
