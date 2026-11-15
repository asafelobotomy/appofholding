//! Just enough of AppStream metadata (`usr/share/metainfo/*.xml`) for appOFholding: the
//! project links, the component id and the newest release's version. A string scan, not an XML
//! parser: these files are small, flat and machine-written.

use std::cmp::Ordering;

use crate::image::AppImage;
use crate::version;

/// The text of every AppStream file in the image.
pub fn files(img: &AppImage) -> Vec<String> {
    ["/usr/share/metainfo", "/usr/share/appdata"]
        .iter()
        .flat_map(|dir| {
            img.list_dir(dir)
                .into_iter()
                .filter(|(n, is_dir)| !is_dir && n.ends_with(".xml"))
                .filter_map(move |(n, _)| img.read_file(&format!("{dir}/{n}")).ok().flatten())
        })
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .collect()
}

/// The file describing the app whose desktop entry is `desktop_id` (with or without
/// `.desktop`): the one whose `<id>` or `<launchable>` names it, or the only file there is.
pub fn for_app(img: &AppImage, desktop_id: &str) -> Option<String> {
    let want = desktop_id.trim_end_matches(".desktop");
    let mut all = files(img);
    let named = all.iter().position(|t| {
        [element(t, "id"), element(t, "launchable")]
            .into_iter()
            .flatten()
            .any(|v| v.trim_end_matches(".desktop") == want)
    });
    match named {
        Some(i) => Some(all.swap_remove(i)),
        None if all.len() == 1 => all.pop(),
        None => None,
    }
}

/// The `<url>` links that may name the project's repository, the likeliest first. `help`,
/// `contribute` and the like often point to someone else's project, so they are left out.
pub fn project_urls(text: &str) -> Vec<String> {
    const ORDER: [&str; 3] = ["vcs-browser", "homepage", "bugtracker"];
    let mut found: Vec<(usize, String)> = tags(text, "url")
        .into_iter()
        .filter_map(|(attrs, body)| {
            let kind = attr(&attrs, "type")?;
            let rank = ORDER.iter().position(|k| *k == kind)?;
            Some((rank, body))
        })
        .collect();
    found.sort_by_key(|(rank, _)| *rank);
    found.into_iter().map(|(_, u)| u).collect()
}

/// The highest `<release version="…">`. AppStream lists the newest first, but not every tool
/// keeps to that, so they are compared; the first one wins when they cannot be.
pub fn newest_release(text: &str) -> Option<String> {
    let mut rest = text;
    let mut best: Option<String> = None;
    while let Some(at) = rest.find("<release") {
        rest = &rest[at + "<release".len()..];
        // Any whitespace may follow the tag name; `<releases>` is a different element.
        if !rest.starts_with(|c: char| c.is_ascii_whitespace()) {
            continue;
        }
        let Some(end) = rest.find('>') else { break };
        let found = attr(&rest[..end], "version");
        if let Some(v) = found.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            let newer = best
                .as_deref()
                .is_none_or(|b| version::compare(v, b) == Some(Ordering::Greater));
            if newer {
                best = Some(v.to_string());
            }
        }
    }
    best
}

/// The text of the first `<name>…</name>` element (also with attributes, `<name type="…">`).
pub fn element(text: &str, name: &str) -> Option<String> {
    tags(text, name)
        .into_iter()
        .next()
        .map(|(_, body)| body)
        .filter(|s| !s.is_empty())
}

/// Every `<name …>body</name>`, as (attributes, unescaped trimmed body).
fn tags(text: &str, name: &str) -> Vec<(String, String)> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(&open) {
        rest = &rest[start + open.len()..];
        // `<url` must not match `<urlx`.
        if !rest.starts_with(['>', ' ', '\t', '\n', '\r']) {
            continue;
        }
        let Some(gt) = rest.find('>') else { break };
        let attrs = rest[..gt].to_string();
        let body = &rest[gt + 1..];
        let Some(end) = body.find(&close) else { break };
        out.push((attrs, unescape(body[..end].trim())));
        rest = &body[end..];
    }
    out
}

fn attr(attrs: &str, name: &str) -> Option<String> {
    let at = attrs.find(&format!("{name}="))? + name.len() + 1;
    let rest = &attrs[at..];
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let rest = &rest[1..];
    Some(unescape(&rest[..rest.find(quote)?]))
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<component type="desktop">
      <id>io.github.o.r.desktop</id>
      <launchable type="desktop-id">io.github.o.r.desktop</launchable>
      <url type="homepage">https://example.org/</url>
      <url type="help">https://github.com/someone/else</url>
      <url type="vcs-browser">https://github.com/o/r?a=1&amp;b=2</url>
      <url type="bugtracker">https://github.com/o/r/issues</url>
      <releases>
        <release version="1.9.0" date="2026-01-01"/>
        <release version="1.10.0" date="2026-02-01">
          <description><p>x</p></description>
        </release>
        <release version="1.2" date="2025-01-01"/>
      </releases>
    </component>"#;

    #[test]
    fn project_links_skip_help_and_come_repository_first() {
        assert_eq!(
            project_urls(XML),
            [
                "https://github.com/o/r?a=1&b=2",
                "https://example.org/",
                "https://github.com/o/r/issues"
            ]
        );
    }

    #[test]
    fn the_newest_release_is_compared_not_just_taken_first() {
        assert_eq!(newest_release(XML).as_deref(), Some("1.10.0"));
        assert_eq!(newest_release("<releases/>"), None);
        assert_eq!(
            newest_release(r#"<release version="nightly"/><release version="x"/>"#).as_deref(),
            Some("nightly")
        );
    }

    #[test]
    fn release_tags_split_over_lines_and_padded_versions_are_read() {
        let xml = "<releases>\n<release\n  version=\"1.2.0\"/>\n<release\tversion=\" 1.10.0 \" date=\"x\"/>\n<release version=\"1.9.0\"/>\n</releases>";
        assert_eq!(newest_release(xml).as_deref(), Some("1.10.0"));
        // A padded first entry is compared trimmed, so a smaller later one doesn't replace it.
        assert_eq!(
            newest_release(r#"<release version=" 2.0 "/><release version="1.5"/>"#).as_deref(),
            Some("2.0")
        );
    }

    #[test]
    fn elements_are_read_with_or_without_attributes() {
        assert_eq!(element(XML, "id").as_deref(), Some("io.github.o.r.desktop"));
        assert_eq!(
            element(XML, "launchable").as_deref(),
            Some("io.github.o.r.desktop")
        );
        assert_eq!(element("<ids>x</ids>", "id"), None);
    }
}
