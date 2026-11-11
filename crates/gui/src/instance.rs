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
    /// An app's page to show (`--app` / `--desktop-id`, `--tab`).
    pub open: Option<Open>,
}

/// An app's page to show, on one of its tabs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Open {
    /// The library id (not checked here; the page says when there is no such app).
    pub app: String,
    /// One of [`TABS`].
    pub tab: String,
}

impl Open {
    pub fn to_json(&self) -> Value {
        json!({"app": self.app, "tab": self.tab})
    }
}

/// The tabs of an app's page, in order.
pub const TABS: [&str; 4] = ["overview", "updates", "launch", "sandbox"];

/// Options that take a value: `--app ID`, `--desktop-id appofholding-ID.desktop`, `--tab TAB`
/// (also written `--tab=TAB`).
const VALUE_OPTIONS: [&str; 3] = ["--app", "--desktop-id", "--tab"];

/// Split the command line (without the program name) into the AppImages to add (every argument
/// that is not an option or an option's value) and the app page to show. A desktop id that
/// appOFholding did not write, or an unknown tab, is ignored.
pub fn parse_args(args: &[String]) -> (Vec<String>, Option<Open>) {
    let mut files = Vec::new();
    let (mut app, mut tab): (Option<String>, Option<String>) = (None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let (name, value) = match a.split_once('=') {
            Some((n, v)) if VALUE_OPTIONS.contains(&n) => (n, Some(v.to_string())),
            _ if VALUE_OPTIONS.contains(&a.as_str()) => (a.as_str(), it.next().cloned()),
            _ => {
                if !a.starts_with('-') {
                    files.push(a.clone());
                }
                continue;
            }
        };
        let Some(value) = value else { continue };
        match name {
            "--app" => app = Some(value),
            "--desktop-id" => {
                app = appofholding_core::paths::app_id_from_desktop_id(&value).map(str::to_string)
            }
            _ => tab = Some(value),
        }
    }
    let open = app.filter(|a| !a.is_empty()).map(|app| Open {
        app,
        tab: tab
            .filter(|t| TABS.contains(&t.as_str()))
            .unwrap_or_else(|| TABS[0].to_string()),
    });
    (files, open)
}

impl Request {
    pub fn encode(&self) -> String {
        json!({
            "files": self.files,
            "show": self.show,
            "open": self.open.as_ref().map(Open::to_json),
        })
        .to_string()
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
            open: v.get("open").and_then(|o| {
                Some(Open {
                    app: o.get("app")?.as_str()?.to_string(),
                    tab: o.get("tab")?.as_str()?.to_string(),
                })
            }),
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
            open: None,
        };
        assert_eq!(Request::decode(&r.encode()), Some(r));
        assert_eq!(Request::decode("garbage"), None);
        assert_eq!(
            Request::decode(r#"{"files":[]}"#),
            Some(Request {
                files: vec![],
                show: true,
                open: None,
            })
        );
    }

    #[test]
    fn requests_carry_the_page_to_open() {
        let r = Request {
            files: vec![],
            show: true,
            open: Some(Open {
                app: "krita".into(),
                tab: "launch".into(),
            }),
        };
        assert_eq!(Request::decode(&r.encode()), Some(r));
        // A request from an older version has no "open".
        assert_eq!(
            Request::decode(r#"{"files":[],"show":true}"#).unwrap().open,
            None
        );
    }

    #[test]
    fn arguments_split_into_files_and_a_page() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let open = |app: &str, tab: &str| {
            Some(Open {
                app: app.into(),
                tab: tab.into(),
            })
        };
        assert_eq!(
            parse_args(&a(&["/x/A.AppImage", "--tray", "b.AppImage"])),
            (a(&["/x/A.AppImage", "b.AppImage"]), None)
        );
        assert_eq!(
            parse_args(&a(&[
                "--tab",
                "launch",
                "--desktop-id",
                "appofholding-krita.desktop"
            ])),
            (vec![], open("krita", "launch"))
        );
        assert_eq!(
            parse_args(&a(&["--app=krita", "--tab=sandbox", "c.AppImage"])),
            (a(&["c.AppImage"]), open("krita", "sandbox"))
        );
        // An unknown tab shows the overview; a foreign desktop id opens nothing.
        assert_eq!(
            parse_args(&a(&["--app", "krita", "--tab", "nope"])).1,
            open("krita", "overview")
        );
        assert_eq!(
            parse_args(&a(&["--desktop-id", "firefox.desktop"])),
            (vec![], None)
        );
        // An option's value is never taken for a file, even when it is missing.
        assert_eq!(parse_args(&a(&["--tab"])), (vec![], None));
    }

    #[test]
    fn a_second_start_reaches_the_first() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("s.socket");
        let req = Request {
            files: vec!["/a.AppImage".into()],
            show: true,
            open: None,
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
            open: None,
        };
        assert!(forward(&path, &req));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), req);
    }
}
