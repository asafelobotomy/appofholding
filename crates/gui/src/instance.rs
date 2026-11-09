//! One window per session. A second start (the menu entry clicked while appOFholding sits in
//! the tray, or an AppImage opened with it) hands its files to the running copy, which shows
//! itself, and then exits.
//!
//! The running copy listens on a Unix socket in `$XDG_RUNTIME_DIR`, which only this user can
//! reach. Without that directory every start simply gets its own window, as before.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

/// What a second start asks the running copy to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// AppImages to add (absolute paths).
    pub files: Vec<String>,
    /// Bring the window up. A start from the login autostart with `--tray` does not.
    pub show: bool,
}

impl Request {
    pub fn encode(&self) -> String {
        json!({"files": self.files, "show": self.show}).to_string()
    }

    pub fn decode(line: &str) -> Option<Request> {
        let v: Value = serde_json::from_str(line.trim()).ok()?;
        Some(Request {
            files: v
                .get("files")?
                .as_array()?
                .iter()
                .filter_map(|f| f.as_str().map(str::to_string))
                .collect(),
            show: v.get("show").and_then(Value::as_bool).unwrap_or(true),
        })
    }
}

pub fn socket_path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|d| d.is_absolute() && d.is_dir())
        .map(|d| d.join("appofholding-gui.socket"))
}

/// Pass `req` to a copy already running at `path`. True when one took it.
pub fn forward(path: &Path, req: &Request) -> bool {
    let Ok(mut stream) = UnixStream::connect(path) else {
        return false;
    };
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    if writeln!(stream, "{}", req.encode()).is_err() {
        return false;
    }
    // The running copy answers once it has the request, so a copy that is shutting down (and
    // never reads it) does not swallow this start.
    let mut answer = String::new();
    BufReader::new(stream).read_line(&mut answer).is_ok() && answer.trim() == "ok"
}

/// Become the running copy: answer later starts at `path` by calling `handle` (on the
/// listener's own thread). False when the socket cannot be made; the program then just runs
/// without the single-window behaviour.
pub fn listen(path: &Path, handle: impl Fn(Request) + Send + 'static) -> bool {
    // A socket file left by a copy that crashed: nobody answers it, so it can go.
    if path.exists() && UnixStream::connect(path).is_err() {
        let _ = std::fs::remove_file(path);
    }
    let Ok(listener) = UnixListener::bind(path) else {
        return false;
    };
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut line = String::new();
            let Ok(mut reader) = stream.try_clone().map(BufReader::new) else {
                continue;
            };
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            if let Some(req) = Request::decode(&line) {
                let mut s = stream;
                let _ = writeln!(s, "ok");
                handle(req);
            }
        }
    });
    true
}

/// Stop answering later starts (before handing over to a new version of the program).
pub fn release(path: &Path) {
    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn requests_round_trip() {
        let r = Request {
            files: vec!["/home/u/My Apps/x.AppImage".into()],
            show: false,
        };
        assert_eq!(Request::decode(&r.encode()), Some(r));
        assert_eq!(Request::decode("garbage"), None);
        assert_eq!(
            Request::decode(r#"{"files":[]}"#),
            Some(Request {
                files: vec![],
                show: true
            })
        );
    }

    #[test]
    fn a_second_start_reaches_the_first() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("s.socket");
        let req = Request {
            files: vec!["/a.AppImage".into()],
            show: true,
        };
        assert!(!forward(&path, &req), "nobody is listening yet");

        let (tx, rx) = mpsc::channel();
        assert!(listen(&path, move |r| tx.send(r).unwrap()));
        assert!(forward(&path, &req));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), req);

        release(&path);
        assert!(!forward(&path, &req), "a released copy no longer answers");
    }

    #[test]
    fn a_stale_socket_is_replaced() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("s.socket");
        drop(UnixListener::bind(&path).unwrap()); // left behind, nobody listening
        let (tx, rx) = mpsc::channel();
        assert!(listen(&path, move |r| tx.send(r).unwrap()));
        let req = Request {
            files: vec![],
            show: true,
        };
        assert!(forward(&path, &req));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), req);
    }
}
