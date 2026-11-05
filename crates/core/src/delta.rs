//! Delta updates through the external `zsync` tool, when it is installed. The result is always
//! verified by the caller exactly like a normal download, so a missing or failing `zsync` just
//! means a full download instead.

use std::path::Path;
use std::process::{Command, Stdio};

pub fn zsync_available() -> bool {
    Command::new("zsync")
        .arg("-V")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// Arguments for `zsync`: seed from `old`, write to `out`, fetch what changed from `control_url`.
pub fn zsync_args(control_url: &str, old: &Path, out: &Path) -> Vec<String> {
    vec![
        "-q".into(),
        "-i".into(),
        old.to_string_lossy().into_owned(),
        "-o".into(),
        out.to_string_lossy().into_owned(),
        control_url.into(),
    ]
}

/// Try to produce `out` from `old` plus a small download. `false` on any problem.
pub fn try_delta(control_url: &str, old: &Path, out: &Path) -> bool {
    if !zsync_available() {
        return false;
    }
    let ok = Command::new("zsync")
        .args(zsync_args(control_url, old, out))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        let _ = std::fs::remove_file(out);
    }
    ok && out.exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_expected_command() {
        let a = zsync_args("https://h/a.zsync", Path::new("/old"), Path::new("/new"));
        assert_eq!(a, ["-q", "-i", "/old", "-o", "/new", "https://h/a.zsync"]);
    }

    #[test]
    fn missing_tool_or_bad_url_is_just_false() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        assert!(!try_delta(
            "http://127.0.0.1:1/none.zsync",
            &dir.path().join("old"),
            &out
        ));
        assert!(!out.exists());
    }
}
