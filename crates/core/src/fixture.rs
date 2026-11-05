//! Builds tiny fake AppImages entirely in memory, for tests. Enabled with the `fixtures`
//! feature (and always under `cfg(test)`).

use std::io::Cursor;

use backhand::{FilesystemWriter, NodeHeader};

/// A file to place inside the fake AppImage's squashfs.
pub struct Entry {
    pub path: &'static str,
    pub data: Vec<u8>,
}

pub enum Item {
    File(&'static str, Vec<u8>),
    Symlink {
        path: &'static str,
        target: &'static str,
    },
}

pub fn png(size: u32) -> Vec<u8> {
    let mut d = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    d.extend(size.to_be_bytes());
    d.extend(size.to_be_bytes());
    d.extend([8, 6, 0, 0, 0]);
    d
}

/// Minimal 64-bit little-endian ELF runtime with a section table containing `.shstrtab` and,
/// optionally, `.upd_info`. `AI\x02` magic is set at offset 8.
pub fn fake_runtime(upd_info: Option<&str>) -> Vec<u8> {
    let mut shstr = vec![0u8];
    let shstr_name = shstr.len() as u32;
    shstr.extend(b".shstrtab\0");
    let upd_name = shstr.len() as u32;
    shstr.extend(b".upd_info\0");

    let mut upd = vec![0u8; 1024];
    if let Some(s) = upd_info {
        upd[..s.len()].copy_from_slice(s.as_bytes());
    }

    let mut out = vec![0u8; 64];
    out[0..4].copy_from_slice(b"\x7fELF");
    out[4] = 2; // 64-bit
    out[5] = 1; // little endian
    out[6] = 1;
    out[8..11].copy_from_slice(b"AI\x02");
    let shstr_off = out.len() as u64;
    out.extend(&shstr);
    let upd_off = out.len() as u64;
    out.extend(&upd);
    let shoff = out.len() as u64;

    let sh = |name: u32, off: u64, size: u64| {
        let mut s = vec![0u8; 64];
        s[0..4].copy_from_slice(&name.to_le_bytes());
        s[4..8].copy_from_slice(&1u32.to_le_bytes());
        s[24..32].copy_from_slice(&off.to_le_bytes());
        s[32..40].copy_from_slice(&size.to_le_bytes());
        s
    };
    out.extend(vec![0u8; 64]); // null section
    out.extend(sh(shstr_name, shstr_off, shstr.len() as u64));
    out.extend(sh(upd_name, upd_off, upd.len() as u64));

    out[0x28..0x30].copy_from_slice(&shoff.to_le_bytes());
    out[0x3A..0x3C].copy_from_slice(&64u16.to_le_bytes());
    out[0x3C..0x3E].copy_from_slice(&3u16.to_le_bytes());
    out[0x3E..0x40].copy_from_slice(&1u16.to_le_bytes());
    out
}

/// Fake AppImage bytes: runtime + squashfs containing `items`.
pub fn build_appimage(upd_info: Option<&str>, items: Vec<Item>) -> Vec<u8> {
    let mut w = FilesystemWriter::default();
    let hdr = NodeHeader::new(0o755, 0, 0, 0);
    for item in items {
        let path = match &item {
            Item::File(p, _) | Item::Symlink { path: p, .. } => *p,
        };
        if let Some(parent) = std::path::Path::new(path).parent()
            && parent != std::path::Path::new("/")
        {
            // Already-existing directories are fine.
            let _ = w.push_dir_all(parent, hdr);
        }
        match item {
            Item::File(path, data) => w.push_file(Cursor::new(data), path, hdr).unwrap(),
            Item::Symlink { path, target } => w.push_symlink(target, path, hdr).unwrap(),
        }
    }
    let mut sq = Cursor::new(Vec::new());
    w.write(&mut sq).unwrap();
    let mut out = fake_runtime(upd_info);
    out.extend(sq.into_inner());
    out
}

/// A typical well-formed AppImage: root .desktop + icon + `.DirIcon` symlink + AppRun.
pub fn simple_appimage(name: &str, upd_info: Option<&str>) -> Vec<u8> {
    versioned_appimage(name, "1.2.3", upd_info)
}

/// Like [`simple_appimage`] but with a chosen version (and therefore different bytes).
pub fn versioned_appimage(name: &str, version: &str, upd_info: Option<&str>) -> Vec<u8> {
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nName={name}\nExec=AppRun --flag %U\nIcon=icon\nCategories=Utility;\nX-AppImage-Version={version}\n\n[Desktop Action new]\nName=New window\nExec=AppRun --new-window\n"
    );
    build_appimage(
        upd_info,
        vec![
            Item::File("/app.desktop", desktop.into_bytes()),
            Item::File("/icon.png", png(256)),
            Item::Symlink {
                path: "/.DirIcon",
                target: "icon.png",
            },
            Item::File("/AppRun", b"#!/bin/sh\nexit 0\n".to_vec()),
            Item::File("/VERSION", version.as_bytes().to_vec()),
        ],
    )
}
