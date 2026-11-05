//! Minimal ELF reader: just enough to locate the end of the AppImage runtime (where the
//! squashfs payload starts) and to read the `.upd_info` section. We never execute the file.

use std::io::{Read, Seek, SeekFrom};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Default)]
pub struct ElfInfo {
    /// First byte after the section header table; the squashfs image normally starts here.
    pub end_offset: u64,
    /// Raw contents of the `.upd_info` section, NUL padding trimmed.
    pub upd_info: Option<String>,
}

struct Rd {
    big: bool,
}

impl Rd {
    fn u16(&self, b: &[u8]) -> u16 {
        let a = [b[0], b[1]];
        if self.big {
            u16::from_be_bytes(a)
        } else {
            u16::from_le_bytes(a)
        }
    }
    fn u32(&self, b: &[u8]) -> u32 {
        let a = [b[0], b[1], b[2], b[3]];
        if self.big {
            u32::from_be_bytes(a)
        } else {
            u32::from_le_bytes(a)
        }
    }
    fn u64(&self, b: &[u8]) -> u64 {
        let a = [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
        if self.big {
            u64::from_be_bytes(a)
        } else {
            u64::from_le_bytes(a)
        }
    }
}

const MAX_SECTIONS: u64 = 4096;
const MAX_STRTAB: u64 = 1 << 20;

pub fn parse<R: Read + Seek>(r: &mut R) -> Result<ElfInfo> {
    let not_elf = || Error::NotAppImage("missing ELF header".into());
    let mut h = [0u8; 64];
    r.seek(SeekFrom::Start(0)).map_err(Error::io("<image>"))?;
    r.read_exact(&mut h).map_err(|_| not_elf())?;
    if &h[0..4] != b"\x7fELF" {
        return Err(not_elf());
    }
    let is64 = match h[4] {
        1 => false,
        2 => true,
        _ => return Err(Error::NotAppImage("bad ELF class".into())),
    };
    let rd = Rd {
        big: match h[5] {
            1 => false,
            2 => true,
            _ => return Err(Error::NotAppImage("bad ELF endianness".into())),
        },
    };
    let (shoff, shentsize, shnum, shstrndx) = if is64 {
        (
            rd.u64(&h[0x28..]),
            rd.u16(&h[0x3A..]) as u64,
            rd.u16(&h[0x3C..]) as u64,
            rd.u16(&h[0x3E..]) as u64,
        )
    } else {
        (
            rd.u32(&h[0x20..]) as u64,
            rd.u16(&h[0x2E..]) as u64,
            rd.u16(&h[0x30..]) as u64,
            rd.u16(&h[0x32..]) as u64,
        )
    };
    let min_entry = if is64 { 64 } else { 40 };
    if shoff == 0 || shnum == 0 || shentsize < min_entry || shnum > MAX_SECTIONS {
        // Stripped / unusual runtime: caller falls back to scanning for the squashfs magic.
        return Ok(ElfInfo::default());
    }
    let mut info = ElfInfo {
        end_offset: shoff + shentsize * shnum,
        upd_info: None,
    };

    // Section headers.
    let mut table = vec![0u8; (shentsize * shnum) as usize];
    r.seek(SeekFrom::Start(shoff))
        .map_err(Error::io("<image>"))?;
    if r.read_exact(&mut table).is_err() {
        return Ok(ElfInfo::default());
    }
    let section = |i: u64| -> (u32, u64, u64) {
        let s = &table[(i * shentsize) as usize..];
        if is64 {
            (rd.u32(s), rd.u64(&s[24..]), rd.u64(&s[32..]))
        } else {
            (rd.u32(s), rd.u32(&s[16..]) as u64, rd.u32(&s[20..]) as u64)
        }
    };
    if shstrndx >= shnum {
        return Ok(info);
    }
    let (_, stroff, strsize) = section(shstrndx);
    if strsize > MAX_STRTAB {
        return Ok(info);
    }
    let mut strtab = vec![0u8; strsize as usize];
    r.seek(SeekFrom::Start(stroff))
        .map_err(Error::io("<image>"))?;
    if r.read_exact(&mut strtab).is_err() {
        return Ok(info);
    }
    for i in 0..shnum {
        let (name_off, off, size) = section(i);
        let name = strtab
            .get(name_off as usize..)
            .and_then(|s| s.split(|&b| b == 0).next());
        if name == Some(b".upd_info") && size > 0 && size <= 64 * 1024 {
            let mut buf = vec![0u8; size as usize];
            r.seek(SeekFrom::Start(off)).map_err(Error::io("<image>"))?;
            if r.read_exact(&mut buf).is_ok() {
                let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                let s = String::from_utf8_lossy(&buf[..end]).trim().to_string();
                if !s.is_empty() {
                    info.upd_info = Some(s);
                }
            }
            break;
        }
    }
    Ok(info)
}

/// Fallback when the section table is unusable: look for a plausible squashfs superblock in
/// the first few MiB.
pub fn scan_for_squashfs<R: Read + Seek>(r: &mut R) -> Result<Option<u64>> {
    const LIMIT: usize = 8 << 20;
    let mut buf = Vec::new();
    r.seek(SeekFrom::Start(0)).map_err(Error::io("<image>"))?;
    r.take(LIMIT as u64)
        .read_to_end(&mut buf)
        .map_err(Error::io("<image>"))?;
    let mut i = 0;
    while let Some(p) = buf[i..].windows(4).position(|w| w == b"hsqs") {
        let at = i + p;
        // superblock: version_major (u16) at +28 must be 4, block_size (u32) at +12 a power of two.
        if buf.len() >= at + 32 {
            let major = u16::from_le_bytes([buf[at + 28], buf[at + 29]]);
            let bs = u32::from_le_bytes([buf[at + 12], buf[at + 13], buf[at + 14], buf[at + 15]]);
            if major == 4 && bs.is_power_of_two() && (4096..=1 << 20).contains(&bs) {
                return Ok(Some(at as u64));
            }
        }
        i = at + 1;
    }
    Ok(None)
}
