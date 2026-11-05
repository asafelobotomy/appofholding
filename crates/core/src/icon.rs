//! Locating the icon inside an AppImage.

use crate::error::Result;
use crate::image::AppImage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IconFormat {
    Png,
    Svg,
}

#[derive(Debug, Clone)]
pub struct Icon {
    pub data: Vec<u8>,
    pub format: IconFormat,
    /// Pixel size (max of width/height) for PNGs; `None` for scalable icons.
    pub size: Option<u32>,
}

impl Icon {
    pub fn detect(data: Vec<u8>) -> Option<Icon> {
        if data.len() >= 24 && data.starts_with(b"\x89PNG\r\n\x1a\n") {
            let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
            let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
            return Some(Icon {
                format: IconFormat::Png,
                size: Some(w.max(h)),
                data,
            });
        }
        let head = String::from_utf8_lossy(&data[..data.len().min(512)]).to_lowercase();
        if head.contains("<svg") || (head.starts_with("<?xml") && head.contains("svg")) {
            return Some(Icon {
                format: IconFormat::Svg,
                size: None,
                data,
            });
        }
        None // XPM and others are ignored for now
    }
}

/// Finds the best icon for `icon_name` (the `Icon=` value of the chosen .desktop entry).
pub fn find_icon(img: &AppImage, icon_name: Option<&str>) -> Result<Option<Icon>> {
    let mut candidates: Vec<String> = Vec::new();
    if let Some(n) = icon_name.filter(|n| !n.is_empty() && !n.contains('/')) {
        candidates.push(format!("/{n}.png"));
        candidates.push(format!("/{n}.svg"));
    }
    candidates.push("/.DirIcon".into());
    for c in &candidates {
        if let Some(data) = img.read_file(c)?
            && let Some(icon) = Icon::detect(data)
        {
            return Ok(Some(icon));
        }
    }
    // Fall back to the largest hicolor icon of that name.
    let Some(name) = icon_name.filter(|n| !n.is_empty() && !n.contains('/')) else {
        return Ok(None);
    };
    let base = "/usr/share/icons/hicolor";
    let mut best: Option<(u32, Icon)> = None;
    for (dir, is_dir) in img.list_dir(base) {
        if !is_dir {
            continue;
        }
        let rank = if dir == "scalable" {
            u32::MAX
        } else {
            dir.split('x')
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0)
        };
        for ext in ["png", "svg"] {
            if let Some(data) = img.read_file(&format!("{base}/{dir}/apps/{name}.{ext}"))?
                && let Some(icon) = Icon::detect(data)
                && best.as_ref().is_none_or(|(r, _)| rank > *r)
            {
                best = Some((rank, icon));
            }
        }
    }
    Ok(best.map(|(_, i)| i))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_png_size() {
        let mut d = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        d.extend(256u32.to_be_bytes());
        d.extend(128u32.to_be_bytes());
        let i = Icon::detect(d).unwrap();
        assert_eq!((i.format, i.size), (IconFormat::Png, Some(256)));
    }

    #[test]
    fn detects_svg() {
        let i = Icon::detect(b"<svg xmlns='x'></svg>".to_vec()).unwrap();
        assert_eq!(i.format, IconFormat::Svg);
        assert!(Icon::detect(b"garbage".to_vec()).is_none());
    }
}
