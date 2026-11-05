use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error on {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("not an AppImage: {0}")]
    NotAppImage(String),
    #[error("unsupported AppImage: {0}")]
    Unsupported(String),
    #[error("squashfs error: {0}")]
    Squashfs(String),
    #[error("no .desktop file found in the AppImage")]
    NoDesktopEntry,
    #[error("invalid desktop entry: {0}")]
    DesktopEntry(String),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("'{0}' is already in the library")]
    AlreadyAdded(String),
    #[error("no app with id '{0}' in the library")]
    NotFound(String),
    #[error("network error: {0}")]
    Http(String),
    #[error("{0}")]
    Source(String),
    #[error("GitHub API rate limit reached{}; set GITHUB_TOKEN to raise the limit", .0.as_deref().map(|r| format!(" (resets {r})")).unwrap_or_default())]
    RateLimited(Option<String>),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    Checksum { expected: String, actual: String },
    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn io(path: impl AsRef<Path>) -> impl FnOnce(std::io::Error) -> Error {
        let path = path.as_ref().to_path_buf();
        move |source| Error::Io { path, source }
    }
}
