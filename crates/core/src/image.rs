//! Read-only access to an AppImage. Nothing here executes the file: the squashfs payload is
//! parsed in-process with `backhand`.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

use backhand::{FilesystemReader, InnerNode};
use sha2::{Digest, Sha256};

use crate::elf;
use crate::error::{Error, Result};

const MAX_READ: u64 = 64 << 20;
const MAX_SYMLINK_HOPS: usize = 16;

pub struct AppImage {
    pub path: PathBuf,
    /// AppImage type (1 or 2). Only type 2 is readable for now.
    pub kind: u8,
    /// Byte offset of the squashfs payload.
    pub offset: u64,
    /// Raw update information embedded in the `.upd_info` ELF section.
    pub update_info: Option<String>,
    fs: FilesystemReader<'static>,
}

impl std::fmt::Debug for AppImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppImage")
            .field("path", &self.path)
            .field("kind", &self.kind)
            .field("offset", &self.offset)
            .field("update_info", &self.update_info)
            .finish_non_exhaustive()
    }
}

impl AppImage {
    pub fn open(path: &Path) -> Result<Self> {
        let f = File::open(path).map_err(Error::io(path))?;
        let mut r = BufReader::new(f);
        let elf_info = elf::parse(&mut r)?;

        let mut magic = [0u8; 3];
        r.seek(SeekFrom::Start(8)).map_err(Error::io(path))?;
        r.read_exact(&mut magic)
            .map_err(|_| Error::NotAppImage("file too short".into()))?;
        let kind = match magic {
            [b'A', b'I', 2] => 2,
            [b'A', b'I', 1] => {
                return Err(Error::Unsupported(
                    "type 1 (ISO 9660) AppImages are not supported yet".into(),
                ));
            }
            // Some runtimes omit the magic but are still type 2; accept if a squashfs follows.
            _ => 2,
        };

        let mut offset = None;
        if elf_info.end_offset > 0 {
            let mut m = [0u8; 8];
            r.seek(SeekFrom::Start(elf_info.end_offset))
                .map_err(Error::io(path))?;
            if r.read_exact(&mut m).is_ok() {
                if &m[0..4] == b"hsqs" {
                    offset = Some(elf_info.end_offset);
                } else if &m[0..6] == b"DWARFS" {
                    return Err(Error::Unsupported(
                        "DwarFS-based AppImages are not supported yet".into(),
                    ));
                }
            }
        }
        let offset = match offset {
            Some(o) => o,
            None => elf::scan_for_squashfs(&mut r)?.ok_or_else(|| {
                Error::NotAppImage("no squashfs filesystem found after the ELF runtime".into())
            })?,
        };

        r.seek(SeekFrom::Start(0)).map_err(Error::io(path))?;
        let fs = FilesystemReader::from_reader_with_offset(r, offset)
            .map_err(|e| Error::Squashfs(e.to_string()))?;
        Ok(Self {
            path: path.to_path_buf(),
            kind,
            offset,
            update_info: elf_info.upd_info,
            fs,
        })
    }

    /// Names of entries directly inside `dir` (e.g. `"/"`), with a directory flag.
    pub fn list_dir(&self, dir: &str) -> Vec<(String, bool)> {
        let dir = normalize(Path::new(dir));
        let mut out: Vec<(String, bool)> = self
            .fs
            .files()
            .filter(|n| n.fullpath != Path::new("/") && n.fullpath.parent() == Some(dir.as_path()))
            .filter_map(|n| {
                let name = n.fullpath.file_name()?.to_string_lossy().into_owned();
                Some((name, matches!(n.inner, InnerNode::Dir(_))))
            })
            .collect();
        out.sort();
        out
    }

    /// Reads a file, following symlinks inside the image. `None` if absent or not a file.
    pub fn read_file(&self, path: &str) -> Result<Option<Vec<u8>>> {
        let mut cur = normalize(Path::new(path));
        for _ in 0..MAX_SYMLINK_HOPS {
            let Some(node) = self.fs.files().find(|n| n.fullpath == cur) else {
                return Ok(None);
            };
            match &node.inner {
                InnerNode::File(f) => {
                    let mut buf = Vec::new();
                    self.fs
                        .file(f)
                        .reader()
                        .take(MAX_READ)
                        .read_to_end(&mut buf)
                        .map_err(|e| Error::Squashfs(e.to_string()))?;
                    return Ok(Some(buf));
                }
                InnerNode::Symlink(s) => {
                    cur = if s.link.is_absolute() {
                        normalize(&s.link)
                    } else {
                        normalize(&cur.parent().unwrap_or(Path::new("/")).join(&s.link))
                    };
                }
                _ => return Ok(None),
            }
        }
        Err(Error::Squashfs(format!(
            "too many symlinks resolving {path}"
        )))
    }

    pub fn exists(&self, path: &str) -> bool {
        let p = normalize(Path::new(path));
        self.fs.files().any(|n| n.fullpath == p)
    }
}

/// Make `p` absolute within the image and resolve `.`/`..` lexically.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::from("/");
    for c in p.components() {
        match c {
            Component::Normal(s) => out.push(s),
            Component::ParentDir => {
                out.pop();
            }
            _ => {}
        }
    }
    out
}

/// SHA-1 of a file on disk (zsync control files identify builds by SHA-1).
pub fn sha1_file(path: &Path) -> Result<String> {
    use sha1::Sha1;
    let mut f = File::open(path).map_err(Error::io(path))?;
    let mut h = Sha1::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(Error::io(path))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// SHA-256 and size of a file on disk.
pub fn hash_file(path: &Path) -> Result<(String, u64)> {
    let mut f = File::open(path).map_err(Error::io(path))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let n = f.read(&mut buf).map_err(Error::io(path))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        total += n as u64;
    }
    Ok((hex::encode(h.finalize()), total))
}
