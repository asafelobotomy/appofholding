//! Releases that are not on a forge: a plain download link, and AppImageUpdate's zsync files.

use super::*;

pub(crate) fn plain_url(http: &dyn Http, url: &str) -> Result<Release> {
    let r = http.head(url, &[])?;
    if !(200..300).contains(&r.status) {
        return Err(Error::Http(format!("HTTP {} from {url}", r.status)));
    }
    // Whatever the server offers to tell a changed file from an unchanged one. When it offers
    // nothing the tag stays empty, which `version::decide` reports as "can't tell", never as
    // an update.
    let nonempty = |s: &str| {
        let s = s.trim();
        (!s.is_empty()).then(|| s.to_string())
    };
    let ident = r
        .header("etag")
        .and_then(|e| nonempty(e.trim().trim_start_matches("W/").trim_matches('"')))
        .or_else(|| r.header("last-modified").and_then(nonempty))
        .or_else(|| {
            r.header("content-length")
                .and_then(nonempty)
                .map(|l| format!("size-{l}"))
        })
        .unwrap_or_default();
    // A plain link *is* the AppImage, whatever its URL looks like, so it is named after what
    // the server calls it, else the URL's last path segment without query or fragment.
    let file = r
        .header("content-disposition")
        .and_then(disposition_filename)
        .unwrap_or_else(|| url_file_name(url));
    Ok(Release {
        tag: ident,
        name: None,
        prerelease: false,
        published_at: r.header("last-modified").map(str::to_string),
        notes: None,
        opaque: true,
        assets: vec![Asset {
            name: appimage_file_name(&file),
            url: url.to_string(),
            size: r
                .header("content-length")
                .and_then(|l| l.trim().parse().ok()),
            sha256: None,
        }],
    })
}

/// The last path segment of `url`, without query or fragment, percent-decoded; empty if the
/// URL has no path.
pub(crate) fn url_file_name(url: &str) -> String {
    let no_query = url.split(['?', '#']).next().unwrap_or("");
    let after_scheme = no_query.split_once("://").map_or(no_query, |x| x.1);
    let path = after_scheme.split_once('/').map_or("", |x| x.1);
    percent_decode(path.rsplit('/').next().unwrap_or(""))
}

/// The file name in a `Content-Disposition` header: `filename*=UTF-8''…` (RFC 6266) first, else
/// `filename="…"`.
pub(crate) fn disposition_filename(h: &str) -> Option<String> {
    let params: Vec<(String, String)> = h
        .split(';')
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((k.trim().to_ascii_lowercase(), v.trim().to_string()))
        })
        .collect();
    let get = |key: &str| {
        params
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };
    let name = get("filename*")
        .map(|v| {
            let v = v.trim_matches('"');
            // charset'language'value
            let value = v.splitn(3, '\'').nth(2).unwrap_or(v);
            percent_decode(value)
        })
        .or_else(|| get("filename").map(|v| v.trim_matches('"').to_string()))?;
    // Never a path: only the last component counts.
    let name = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    (!name.is_empty()).then_some(name)
}

pub(crate) fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(h) = s.get(i + 1..i + 3)
            && let Ok(v) = u8::from_str_radix(h, 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A file name that is safe to install under: no path separators, control characters, `?`,
/// `*` or leading dots, and ending in `.AppImage`.
pub(crate) fn appimage_file_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | '?' | '*' | '"' | '<' | '>' | '|' | ':') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let clean = clean.trim().trim_start_matches('.').trim();
    let stem = if clean.is_empty() { "download" } else { clean };
    if stem.to_ascii_lowercase().ends_with(".appimage") {
        stem.to_string()
    } else {
        format!("{stem}.AppImage")
    }
}

/// Read a `.zsync` control file. Its text header names the real download URL, file name,
/// length and SHA-1 of the current build, which is all we need to detect an update without
/// downloading anything large (and is how AppImage authors publish "latest" files).
pub(crate) fn zsync(http: &dyn Http, url: &str) -> Result<Release> {
    let r = http.get(url, &[])?;
    if !(200..300).contains(&r.status) {
        return Err(Error::Http(format!("HTTP {} from {url}", r.status)));
    }
    let end = r
        .body
        .windows(2)
        .position(|w| w == b"\n\n")
        .unwrap_or(r.body.len().min(8192));
    let header = String::from_utf8_lossy(&r.body[..end]).into_owned();
    let field = |key: &str| {
        header.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            (k.eq_ignore_ascii_case(key)).then(|| v.trim().to_string())
        })
    };
    if !header.starts_with("zsync:") {
        return Err(Error::Source(format!("{url} is not a zsync control file")));
    }
    let base = url.strip_suffix(".zsync").unwrap_or(url);
    let asset_url = match field("URL") {
        Some(u) if u.starts_with("http://") || u.starts_with("https://") => u,
        // Relative URLs are relative to the control file's directory.
        Some(u) => format!(
            "{}/{}",
            url.rsplit_once('/').map_or("", |x| x.0),
            u.trim_start_matches("./")
        ),
        None => base.to_string(),
    };
    let name = field("Filename")
        .or_else(|| asset_url.split('/').next_back().map(str::to_string))
        .unwrap_or_else(|| "download.AppImage".into());
    let tag = match field("SHA-1") {
        Some(h) => format!("sha1:{}", h.to_ascii_lowercase()),
        None => field("MTime").unwrap_or_default(),
    };
    Ok(Release {
        tag,
        name: Some(name.clone()),
        prerelease: false,
        published_at: field("MTime"),
        notes: None,
        opaque: true,
        assets: vec![Asset {
            name,
            url: asset_url,
            size: field("Length").and_then(|l| l.parse().ok()),
            sha256: None,
        }],
    })
}
