//! Downloading an asset to disk with checksum verification.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::assets::Asset;
use crate::error::{Error, Result};
use crate::http::Http;

struct HashingWriter<W: Write> {
    inner: W,
    hash: Sha256,
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hash.update(&buf[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Download `asset` into `dir` (as its own file name). Returns the final path and the SHA-256 of
/// what was written. If the host published a digest, a mismatch deletes the file and fails.
pub fn download_asset(
    http: &dyn Http,
    asset: &Asset,
    dir: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(PathBuf, String)> {
    fs::create_dir_all(dir).map_err(Error::io(dir))?;
    // Never trust a server-supplied name as a path.
    let name = Path::new(&asset.name)
        .file_name()
        .ok_or_else(|| Error::Source(format!("bad asset name '{}'", asset.name)))?;
    let dest = dir.join(name);
    let part = dir.join(format!(".{}.part", name.to_string_lossy()));

    let file = File::create(&part).map_err(Error::io(&part))?;
    let mut w = HashingWriter {
        inner: file,
        hash: Sha256::new(),
    };
    let result = http.download(&asset.url, &mut w, progress);
    let flushed = w.flush();
    let actual = hex::encode(w.hash.finalize());
    if let Err(e) = result {
        let _ = fs::remove_file(&part);
        return Err(e);
    }
    if let Err(e) = flushed {
        let _ = fs::remove_file(&part);
        return Err(Error::io(&part)(e));
    }
    if let Some(expected) = &asset.sha256
        && !expected.eq_ignore_ascii_case(&actual) {
            let _ = fs::remove_file(&part);
            return Err(Error::Checksum {
                expected: expected.clone(),
                actual,
            });
        }
    fs::rename(&part, &dest).map_err(Error::io(&dest))?;
    Ok((dest, actual))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::fake::FakeHttp;

    fn asset(name: &str, sha: Option<&str>) -> Asset {
        Asset {
            name: name.into(),
            url: "https://dl/x".into(),
            size: None,
            sha256: sha.map(String::from),
        }
    }

    #[test]
    fn downloads_and_verifies() {
        let dir = tempfile::tempdir().unwrap();
        let body = b"hello appimage".to_vec();
        let sha = hex::encode(Sha256::digest(&body));
        let http = FakeHttp::default().bytes("https://dl/x", body.clone());
        let (path, got) = download_asset(
            &http,
            &asset("Foo.AppImage", Some(&sha)),
            dir.path(),
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(got, sha);
        assert_eq!(fs::read(path).unwrap(), body);
    }

    #[test]
    fn mismatch_deletes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let http = FakeHttp::default().bytes("https://dl/x", b"tampered".to_vec());
        let err = download_asset(
            &http,
            &asset("Foo.AppImage", Some("00")),
            dir.path(),
            &mut |_, _| {},
        )
        .unwrap_err();
        assert!(matches!(err, Error::Checksum { .. }));
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            0,
            "no partial or final file is left behind"
        );
    }

    #[test]
    fn server_supplied_names_cannot_escape() {
        let dir = tempfile::tempdir().unwrap();
        let http = FakeHttp::default().bytes("https://dl/x", b"x".to_vec());
        let (path, _) = download_asset(
            &http,
            &asset("../../evil.AppImage", None),
            dir.path(),
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(path.parent().unwrap(), dir.path());
    }
}
