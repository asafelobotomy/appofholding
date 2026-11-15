//! Delta updates through the external `zsync` tool, when it is installed. The caller checks the
//! result with [`verify`] (the size and SHA-256 a normal download is held to, where known, and
//! the SHA-1 the zsync control file names) and then inspects it like any download; a missing or
//! failing `zsync`, or a result that does not verify, just means a full download instead.

use std::path::Path;
use std::process::{Command, Stdio};

use crate::assets::Asset;
use crate::error::{Error, Result};
use crate::image;

/// Check a file `zsync` produced against what the release says it should be: `asset`'s size
/// and SHA-256 when known, and the SHA-1 in `release_tag` when it is one (`sha1:<hex>`, as
/// zsync sources identify builds). Returns the file's SHA-256.
pub fn verify(out: &Path, asset: &Asset, release_tag: &str) -> Result<String> {
    let (sha256, size) = image::hash_file(out)?;
    if let Some(want) = asset.size
        && want != size
    {
        return Err(Error::Source(format!(
            "rebuilt file has {size} bytes, expected {want}"
        )));
    }
    if let Some(want) = &asset.sha256
        && !want.eq_ignore_ascii_case(&sha256)
    {
        return Err(Error::Source("rebuilt file has the wrong SHA-256".into()));
    }
    if let Some(want) = release_tag.strip_prefix("sha1:")
        && !want.eq_ignore_ascii_case(&image::sha1_file(out)?)
    {
        return Err(Error::Source("rebuilt file has the wrong SHA-1".into()));
    }
    Ok(sha256)
}

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
    fn rebuilt_files_are_held_to_size_and_checksums() {
        use sha1::Digest as _;
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        std::fs::write(&out, b"hello").unwrap();
        let sha256 = hex::encode(sha2::Sha256::digest(b"hello"));
        let sha1 = hex::encode(sha1::Sha1::digest(b"hello"));
        let asset = |size, sha: Option<&str>| Asset {
            name: "a.AppImage".into(),
            url: "https://h/a.AppImage".into(),
            size,
            sha256: sha.map(str::to_string),
        };
        let tag = format!("sha1:{}", sha1.to_uppercase());
        assert_eq!(verify(&out, &asset(Some(5), None), &tag).unwrap(), sha256);
        assert!(verify(&out, &asset(None, Some(&sha256)), "etag").is_ok());
        assert!(verify(&out, &asset(Some(6), None), &tag).is_err());
        assert!(verify(&out, &asset(None, Some("00")), &tag).is_err());
        assert!(verify(&out, &asset(Some(5), None), "sha1:00").is_err());
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
