//! Scanning AppImages with oysterAV (`oyst-cli scan <file> --ndjson`).
//!
//! The contract is oysterAV's (`docs/cli/gui-contract.md`, "Apps that save files"): progress lines
//! `{"event":"progress",…}`, then one `{"event":"result","clean":…,"findings":[{"threat_name":…}],
//! "pack_errors":[{"error":…}]}`; exit 0 clean, 1 threats found, 3 another oysterAV job is running
//! (ask again later), anything else a failure. oysterAV only looks: a flagged file is never moved or
//! deleted by this module.
//!
//! The library setting `scan` chooses what appOFholding does with that:
//! `off` never scans, `warn` scans each AppImage as it is added or updated and keeps the verdict,
//! `block` also refuses to add or install one in which oysterAV found a threat (an app that could
//! not be scanned is only reported). A scan on request (`appofholding scan`) works in every mode.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::library::Library;

const DEFAULT_CLI: &str = "/usr/bin/oyst-cli";
const MODE: &str = "scan-mode";
/// oysterAV runs one job at a time; when it is busy, ask again this often, this many times.
const BUSY_DELAY: Duration = Duration::from_secs(5);
const BUSY_TRIES: u32 = 60;

/// What appOFholding does with oysterAV.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScanMode {
    #[default]
    Off,
    Warn,
    Block,
}

impl ScanMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ScanMode::Off => "off",
            ScanMode::Warn => "warn",
            ScanMode::Block => "block",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "off" => ScanMode::Off,
            "warn" => ScanMode::Warn,
            "block" => ScanMode::Block,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanState {
    Clean,
    /// oysterAV found something; `detail` names it.
    Threat,
    /// The scan could not run or finish; `detail` says why.
    Error,
}

impl ScanState {
    pub fn as_str(self) -> &'static str {
        match self {
            ScanState::Clean => "clean",
            ScanState::Threat => "threat",
            ScanState::Error => "error",
        }
    }
}

/// What oysterAV said about one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    pub state: ScanState,
    /// The threat names, or why the scan failed; empty when clean.
    #[serde(default)]
    pub detail: String,
    /// Unix seconds.
    pub at: i64,
    /// The file the verdict is about: a verdict never applies to another build.
    pub sha256: String,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// `$APPOFHOLDING_OYST_CLI` (for tests and odd installs), else `/usr/bin/oyst-cli`.
pub fn oyst_cli() -> PathBuf {
    std::env::var_os("APPOFHOLDING_OYST_CLI").map_or_else(|| DEFAULT_CLI.into(), PathBuf::from)
}

/// One run of `oyst-cli`, read.
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    Done(ScanState, String),
    /// Another oysterAV job is running.
    Busy,
}

fn read_answer(code: Option<i32>, stdout: &str, stderr: &str) -> Answer {
    if code == Some(3) {
        return Answer::Busy;
    }
    let result = stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .rfind(|v| v.get("event").and_then(Value::as_str) == Some("result"));
    let Some(result) = result else {
        let why = stderr
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("oysterAV gave no result");
        return Answer::Done(ScanState::Error, why.trim().chars().take(300).collect());
    };
    let mut threats: Vec<&str> = Vec::new();
    for f in result
        .get("findings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = f
            .get("threat_name")
            .and_then(Value::as_str)
            .unwrap_or("unknown threat");
        if !threats.contains(&name) {
            threats.push(name);
        }
    }
    if !threats.is_empty() {
        return Answer::Done(ScanState::Threat, threats.join(", "));
    }
    let problem = result
        .get("pack_errors")
        .and_then(Value::as_array)
        .and_then(|e| e.first())
        .and_then(|e| e.get("error").and_then(Value::as_str));
    match (problem, result.get("clean").and_then(Value::as_bool), code) {
        (Some(p), _, _) => Answer::Done(ScanState::Error, p.chars().take(300).collect()),
        (None, Some(true), Some(0)) => Answer::Done(ScanState::Clean, String::new()),
        _ => Answer::Done(
            ScanState::Error,
            "oysterAV could not finish the scan".into(),
        ),
    }
}

/// Scan one file with oysterAV, waiting (up to about five minutes) while it is busy with
/// another job. The answer is `(state, detail)`; this never fails: a scan that could not run is
/// the `Error` state.
pub fn scan_file(path: &Path) -> (ScanState, String) {
    scan_with(&oyst_cli(), path, BUSY_DELAY, BUSY_TRIES)
}

pub fn scan_with(cli: &Path, path: &Path, busy_delay: Duration, tries: u32) -> (ScanState, String) {
    let err = |m: &str| (ScanState::Error, m.to_string());
    if !path.is_absolute() {
        return err("the file's place is not known");
    }
    if !path.is_file() {
        return err("the file is not there");
    }
    if !cli.exists() {
        return err("oysterAV is not installed (oyst-cli not found)");
    }
    for attempt in 0..tries.max(1) {
        let out = match Command::new(cli)
            .args(["scan", "--ndjson"])
            .arg(path)
            .stdin(Stdio::null())
            .output()
        {
            Ok(o) => o,
            Err(e) => return (ScanState::Error, format!("could not start oysterAV: {e}")),
        };
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        match read_answer(out.status.code(), &stdout, &stderr) {
            Answer::Done(state, detail) => return (state, detail),
            Answer::Busy if attempt + 1 < tries.max(1) => std::thread::sleep(busy_delay),
            Answer::Busy => {}
        }
    }
    err("oysterAV stayed busy with another job")
}

fn verdict_of(path: &Path, sha256: &str) -> Verdict {
    let (state, detail) = scan_file(path);
    Verdict {
        state,
        detail,
        at: now(),
        sha256: sha256.to_string(),
    }
}

impl Library {
    /// What to do with oysterAV: `off` unless chosen.
    pub fn scan_mode(&self) -> Result<ScanMode> {
        Ok(self
            .db
            .get_library_setting(MODE)?
            .and_then(|v| ScanMode::parse(&v))
            .unwrap_or_default())
    }

    pub fn set_scan_mode(&self, mode: ScanMode) -> Result<()> {
        self.db.set_library_setting(MODE, Some(mode.as_str()))
    }

    /// What oysterAV last said about this app's current file; none when it was never scanned or
    /// the file has changed since.
    pub fn scan_verdict(&self, id: &str) -> Result<Option<Verdict>> {
        let app = self.get(id)?;
        Ok(self
            .db
            .get_setting(id, "scan")?
            .and_then(|j| serde_json::from_str::<Verdict>(&j).ok())
            .filter(|v| v.sha256 == app.sha256))
    }

    pub(crate) fn record_verdict(&self, id: &str, v: &Verdict) -> Result<()> {
        let json = serde_json::to_string(v).map_err(|e| Error::Other(e.to_string()))?;
        self.db.set_setting(id, "scan", Some(&json))
    }

    /// Scan an app's current AppImage and keep the verdict.
    pub fn scan_app(&self, id: &str) -> Result<Verdict> {
        let app = self.get(id)?;
        let v = verdict_of(&app.appimage_path, &app.sha256);
        self.record_verdict(id, &v)?;
        Ok(v)
    }

    /// Before a file becomes (part of) the library: scan it when the mode asks, and refuse a
    /// threat in `block` mode. `Ok(None)` when scanning is off.
    pub(crate) fn scan_before_install(&self, path: &Path, sha256: &str) -> Result<Option<Verdict>> {
        let mode = self.scan_mode()?;
        if mode == ScanMode::Off {
            return Ok(None);
        }
        let v = verdict_of(path, sha256);
        if mode == ScanMode::Block && v.state == ScanState::Threat {
            return Err(Error::Other(format!(
                "oysterAV found {} in {}; nothing was installed and your library is untouched (see `appofholding config set scan warn` to allow it)",
                v.detail,
                path.file_name().map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into_owned()
                )
            )));
        }
        Ok(Some(v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLEAN: &str = "{\"event\":\"progress\",\"stage\":\"clamav\",\"percent\":100.0}\n{\"event\":\"result\",\"clean\":true,\"findings\":[],\"pack_errors\":[]}\n";

    #[test]
    fn clean_threat_error_and_busy() {
        assert_eq!(
            read_answer(Some(0), CLEAN, ""),
            Answer::Done(ScanState::Clean, String::new())
        );
        let threat = "{\"event\":\"result\",\"clean\":false,\"findings\":[{\"threat_name\":\"Win.Test\"},{\"threat_name\":\"Win.Test\"},{\"threat_name\":\"Other\"}],\"pack_errors\":[]}\n";
        assert_eq!(
            read_answer(Some(1), threat, ""),
            Answer::Done(ScanState::Threat, "Win.Test, Other".into())
        );
        let broken = "{\"event\":\"result\",\"clean\":false,\"findings\":[],\"pack_errors\":[{\"pack\":\"clamav\",\"error\":\"no signatures\"}]}\n";
        assert_eq!(
            read_answer(Some(2), broken, ""),
            Answer::Done(ScanState::Error, "no signatures".into())
        );
        assert_eq!(read_answer(Some(3), "", "busy"), Answer::Busy);
        assert_eq!(
            read_answer(Some(2), "", "\nboom\n"),
            Answer::Done(ScanState::Error, "boom".into())
        );
        // A clean result with a failing exit code is not a clean file.
        assert!(matches!(
            read_answer(Some(2), CLEAN, ""),
            Answer::Done(ScanState::Error, _)
        ));
    }

    #[test]
    fn a_missing_scanner_or_file_is_an_error_not_a_panic() {
        // A file that is always there, also in a clean build chroot (no /etc/hostname).
        let present = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let (s, d) = scan_with(
            Path::new("/nonexistent/oyst-cli"),
            &present,
            Duration::ZERO,
            1,
        );
        assert_eq!(s, ScanState::Error);
        assert!(d.contains("not installed"), "{d}");
        let (_, d) = scan_with(
            Path::new("/bin/true"),
            Path::new("/nonexistent/file"),
            Duration::ZERO,
            1,
        );
        assert!(d.contains("not there"), "{d}");
    }

    #[test]
    fn modes_round_trip() {
        for m in [ScanMode::Off, ScanMode::Warn, ScanMode::Block] {
            assert_eq!(ScanMode::parse(m.as_str()), Some(m));
        }
        assert_eq!(ScanMode::parse("on"), None);
    }
}
