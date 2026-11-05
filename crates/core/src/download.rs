//! Downloading an asset to disk: resumable, checksum-verified, never leaves a bad final file.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
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

fn hash_prefix(path: &Path) -> Result<Sha256> {
    let mut h = Sha256::new();
    let mut f = File::open(path).map_err(Error::io(path))?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(Error::io(path))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h)
}

/// Download `asset` into `dir` (as its own file name). Returns the final path and the SHA-256 of
/// what was written.
///
/// * An interrupted download leaves `.<name>.part` behind and the next call resumes from it
///   (falling back to a full download if the server ignores the Range request).
/// * If the host published a digest (or the size is known), a mismatch deletes the partial file
///   and fails; the final path only ever appears once the file is complete and verified.
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

    let mut offset = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let actual = loop {
        let (file, hash) = if offset > 0 {
            let hash = hash_prefix(&part)?;
            (
                OpenOptions::new()
                    .append(true)
                    .open(&part)
                    .map_err(Error::io(&part))?,
                hash,
            )
        } else {
            (
                File::create(&part).map_err(Error::io(&part))?,
                Sha256::new(),
            )
        };
        let mut w = HashingWriter { inner: file, hash };
        let result = http.download(&asset.url, offset, &mut w, progress);
        let flushed = w.flush();
        match result {
            Ok(()) => {
                flushed.map_err(Error::io(&part))?;
                break hex::encode(w.hash.finalize());
            }
            Err(Error::RangeIgnored) if offset > 0 => {
                offset = 0; // start over from the beginning
            }
            // Keep the partial file so the next attempt can resume.
            Err(e) => return Err(e),
        }
    };

    let bad_size = asset
        .size
        .filter(|s| *s > 0)
        .is_some_and(|s| fs::metadata(&part).map(|m| m.len()).ok() != Some(s));
    if bad_size {
        let _ = fs::remove_file(&part);
        return Err(Error::Source(format!(
            "downloaded file for {} has the wrong size (expected {} bytes)",
            asset.name,
            asset.size.unwrap_or(0)
        )));
    }
    if let Some(expected) = &asset.sha256
        && !expected.eq_ignore_ascii_case(&actual)
    {
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

    #[test]
    fn interrupted_download_resumes_and_still_verifies() {
        let dir = tempfile::tempdir().unwrap();
        let body: Vec<u8> = (0..=255u8).cycle().take(10_000).collect();
        let sha = hex::encode(Sha256::digest(&body));
        let a = asset("Foo.AppImage", Some(&sha));

        // First attempt: the connection drops after 4000 bytes.
        let mut flaky = FakeHttp::default().bytes("https://dl/x", body.clone());
        flaky.fail_after = Some(4000);
        assert!(download_asset(&flaky, &a, dir.path(), &mut |_, _| {}).is_err());
        let part = dir.path().join(".Foo.AppImage.part");
        assert_eq!(
            fs::metadata(&part).unwrap().len(),
            4000,
            "partial data is kept"
        );
        assert!(!dir.path().join("Foo.AppImage").exists());

        // Second attempt resumes: only the remaining bytes are requested/written.
        let good = FakeHttp::default().bytes("https://dl/x", body.clone());
        let (path, got) = download_asset(&good, &a, dir.path(), &mut |_, _| {}).unwrap();
        assert_eq!(got, sha);
        assert_eq!(fs::read(path).unwrap(), body);
        assert!(!part.exists());
    }

    #[test]
    fn server_ignoring_range_restarts_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let body = b"0123456789".to_vec();
        let sha = hex::encode(Sha256::digest(&body));
        fs::write(
            dir.path().join(".Foo.AppImage.part"),
            b"garbage-from-another-version",
        )
        .unwrap();
        let mut http = FakeHttp::default().bytes("https://dl/x", body.clone());
        http.ignore_range = true;
        let (path, _) = download_asset(
            &http,
            &asset("Foo.AppImage", Some(&sha)),
            dir.path(),
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(fs::read(path).unwrap(), body);
    }

    #[test]
    fn wrong_size_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let http = FakeHttp::default().bytes("https://dl/x", b"short".to_vec());
        let mut a = asset("Foo.AppImage", None);
        a.size = Some(999);
        assert!(download_asset(&http, &a, dir.path(), &mut |_, _| {}).is_err());
        assert!(!dir.path().join("Foo.AppImage").exists());
    }
}
