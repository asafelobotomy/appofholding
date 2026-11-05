//! Update sources: where an app's new versions come from, and how users describe them.

use std::fmt;

use crate::assets;
use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provider {
    GitHub {
        owner: String,
        repo: String,
    },
    GitLab {
        host: String,
        project: String,
    },
    Forgejo {
        host: String,
        owner: String,
        repo: String,
    },
    /// A fixed download URL; changes are detected through ETag / Last-Modified.
    Url {
        url: String,
    },
    /// A `.zsync` control file, which names the real download URL and the file's SHA-1.
    Zsync {
        url: String,
    },
}

/// Which release to follow (the AppImageSpec `latest` / `latest-pre` / `latest-all` / tag values).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Channel {
    /// Newest non-prerelease.
    Stable,
    /// Newest prerelease.
    Prerelease,
    /// Newest release of any kind.
    Any,
    /// One specific (possibly moving) tag such as `nightly`.
    Tag(String),
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Channel::Stable => f.write_str("latest"),
            Channel::Prerelease => f.write_str("latest-pre"),
            Channel::Any => f.write_str("latest-all"),
            Channel::Tag(t) => f.write_str(t),
        }
    }
}

impl Channel {
    pub fn parse(s: &str) -> Channel {
        match s {
            "latest" | "stable" | "" => Channel::Stable,
            "latest-pre" | "pre" | "prerelease" => Channel::Prerelease,
            "latest-all" | "all" | "any" => Channel::Any,
            tag => Channel::Tag(tag.to_string()),
        }
    }
}

/// What to do when a new version is found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Policy {
    /// Download and install automatically (the scheduled updater).
    Auto,
    /// Tell the user once per version, install only on request.
    #[default]
    Notify,
    /// Check, but stay silent; install only on request.
    Manual,
    /// Never touch this app's version (no checks, no updates).
    Pinned,
}

impl Policy {
    pub fn as_str(self) -> &'static str {
        match self {
            Policy::Auto => "auto",
            Policy::Notify => "notify",
            Policy::Manual => "manual",
            Policy::Pinned => "pinned",
        }
    }
    pub fn parse(s: &str) -> Option<Policy> {
        Some(match s {
            "auto" => Policy::Auto,
            "notify" => Policy::Notify,
            "manual" => Policy::Manual,
            "pinned" | "lock" | "locked" => Policy::Pinned,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Read from the AppImage's own `.upd_info`.
    Embedded,
    /// Set by the user.
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub provider: Provider,
    pub channel: Channel,
    /// Glob (`*`, `?`) selecting the asset, e.g. `Foo-*-x86_64.AppImage`.
    pub asset_pattern: Option<String>,
    pub origin: Origin,
}

impl Source {
    pub fn new(provider: Provider) -> Self {
        Source {
            provider,
            channel: Channel::Stable,
            asset_pattern: None,
            origin: Origin::User,
        }
    }

    pub fn describe(&self) -> String {
        let place = match &self.provider {
            Provider::GitHub { owner, repo } => format!("github.com/{owner}/{repo}"),
            Provider::GitLab { host, project } => format!("{host}/{project}"),
            Provider::Forgejo { host, owner, repo } => format!("{host}/{owner}/{repo}"),
            Provider::Url { url } => url.clone(),
            Provider::Zsync { url } => format!("zsync {url}"),
        };
        match self.provider {
            Provider::Url { .. } | Provider::Zsync { .. } => place,
            _ => format!("{place} [{}]", self.channel),
        }
    }
}

/// The result of parsing something a user pasted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub source: Source,
    /// Set when the input pointed at one specific file (a release download link).
    pub asset_url: Option<String>,
}

fn parsed(source: Source) -> Parsed {
    Parsed {
        source,
        asset_url: None,
    }
}

/// Understands: embedded update info (`gh-releases-zsync|…`, `zsync|…`), GitHub / GitLab /
/// Forgejo repo, release, tag and download URLs (with or without scheme, trailing slash,
/// `.git`), `owner/repo` shorthand, and plain download URLs.
pub fn parse_input(input: &str) -> Result<Parsed> {
    let s = input.trim();
    if s.is_empty() {
        return Err(Error::Source("empty source".into()));
    }
    if let Some(rest) = s.strip_prefix("gh-releases-zsync|") {
        let p: Vec<&str> = rest.split('|').collect();
        if p.len() != 4 || p.iter().any(|x| x.is_empty()) {
            return Err(Error::Source(format!("malformed update info: {s}")));
        }
        let mut src = Source::new(Provider::GitHub {
            owner: p[0].into(),
            repo: p[1].into(),
        });
        src.channel = Channel::parse(p[2]);
        src.asset_pattern = Some(p[3].strip_suffix(".zsync").unwrap_or(p[3]).to_string());
        src.origin = Origin::Embedded;
        return Ok(parsed(src));
    }
    if let Some(rest) = s.strip_prefix("zsync|") {
        check_http(rest)?;
        let mut src = Source::new(Provider::Zsync {
            url: rest.to_string(),
        });
        src.origin = Origin::Embedded;
        return Ok(parsed(src));
    }
    if s.starts_with("bintray-zsync|") {
        return Err(Error::Source(
            "Bintray update info is obsolete (Bintray was shut down)".into(),
        ));
    }
    let (forced, rest) = match s.split_once(':') {
        Some(("gh", r)) => (Some("github"), r),
        Some(("github", r)) if !r.starts_with("//") => (Some("github"), r),
        Some(("gitlab", r)) if !r.starts_with("//") => (Some("gitlab"), r),
        Some(("forgejo" | "gitea" | "codeberg", r)) if !r.starts_with("//") => (Some("forgejo"), r),
        _ => (None, s),
    };

    let (host, segs, full) = split_url(rest, forced == Some("github") && !rest.contains('.'))?;
    let kind = forced.unwrap_or_else(|| match host.as_str() {
        "github.com" => "github",
        h if h == "gitlab.com" || h.starts_with("gitlab.") => "gitlab",
        h if matches!(h, "codeberg.org" | "gitea.com")
            || h.starts_with("gitea.")
            || h.starts_with("forgejo.") =>
        {
            "forgejo"
        }
        _ => "url",
    });
    match kind {
        "github" => parse_github_like(
            &segs,
            &full,
            |o, r| Provider::GitHub { owner: o, repo: r },
            "github.com",
        ),
        "forgejo" => {
            let h = host.clone();
            parse_github_like(
                &segs,
                &full,
                move |o, r| Provider::Forgejo {
                    host: h.clone(),
                    owner: o,
                    repo: r,
                },
                &host,
            )
        }
        "gitlab" => parse_gitlab(&host, &segs),
        _ => {
            // A plain download link is used exactly as given (query string, `www.` and all);
            // only a `#fragment`, which never reaches the server, is dropped.
            let given = rest.trim();
            let given = given.split('#').next().unwrap_or(given);
            let url = if given.contains("://") {
                given.to_string()
            } else {
                format!("https://{given}")
            };
            check_http(&url)?;
            let path_only = url.split('?').next().unwrap_or(&url);
            let provider = if path_only.ends_with(".zsync") {
                Provider::Zsync { url }
            } else {
                Provider::Url { url }
            };
            Ok(parsed(Source::new(provider)))
        }
    }
}

fn check_http(url: &str) -> Result<()> {
    if url.starts_with("https://") || url.starts_with("http://") {
        Ok(())
    } else {
        Err(Error::Source(format!("not an http(s) URL: {url}")))
    }
}

/// -> (lowercased host without `www.`, non-empty path segments, URL with scheme and no query/fragment)
fn split_url(input: &str, shorthand: bool) -> Result<(String, Vec<String>, String)> {
    let mut s = input.trim().to_string();
    if shorthand {
        s = format!("github.com/{}", s.trim_start_matches('/'));
    }
    if !s.contains("://") {
        // `owner/repo` with no dot in the first segment is GitHub shorthand.
        let first = s.split('/').next().unwrap_or("");
        if !first.contains('.') && s.matches('/').count() == 1 {
            s = format!("github.com/{s}");
        }
        s = format!("https://{s}");
    }
    let (scheme, after) = s.split_once("://").unwrap();
    if scheme != "https" && scheme != "http" {
        return Err(Error::Source(format!("unsupported URL scheme '{scheme}'")));
    }
    let after = after.split(['?', '#']).next().unwrap_or("");
    let (host, path) = after.split_once('/').unwrap_or((after, ""));
    if host.is_empty() {
        return Err(Error::Source(format!("no host in '{input}'")));
    }
    let host = host.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    let segs: Vec<String> = path
        .split('/')
        .filter(|x| !x.is_empty())
        .map(String::from)
        .collect();
    let full = format!("{scheme}://{host}/{}", segs.join("/"));
    Ok((host, segs, full))
}

fn strip_git(s: &str) -> &str {
    s.strip_suffix(".git").unwrap_or(s)
}

fn parse_github_like(
    segs: &[String],
    full: &str,
    mk: impl Fn(String, String) -> Provider,
    host: &str,
) -> Result<Parsed> {
    if segs.len() < 2 {
        return Err(Error::Source(format!("expected {host}/<owner>/<repo>")));
    }
    let mut src = Source::new(mk(segs[0].clone(), strip_git(&segs[1]).to_string()));
    let mut asset_url = None;
    match segs[2..]
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["releases", "tag", tag @ ..] if !tag.is_empty() => {
            src.channel = Channel::Tag(tag.join("/"))
        }
        ["releases", "latest", "download", file] => {
            src.asset_pattern = Some(assets::suggest_pattern(file));
            asset_url = Some(full.to_string());
        }
        ["releases", "download", tag, file] => {
            src.channel = Channel::Tag((*tag).to_string());
            src.asset_pattern = Some(assets::suggest_pattern(file));
            asset_url = Some(full.to_string());
        }
        _ => {}
    }
    Ok(Parsed {
        source: src,
        asset_url,
    })
}

fn parse_gitlab(host: &str, segs: &[String]) -> Result<Parsed> {
    let cut = segs
        .iter()
        .position(|s| s == "-" || s == "releases")
        .unwrap_or(segs.len());
    if cut < 2 {
        return Err(Error::Source(format!("expected {host}/<group>/<project>")));
    }
    let mut project: Vec<&str> = segs[..cut].iter().map(String::as_str).collect();
    if let Some(last) = project.last_mut() {
        *last = strip_git(last);
    }
    let mut src = Source::new(Provider::GitLab {
        host: host.to_string(),
        project: project.join("/"),
    });
    let tail: Vec<&str> = segs[cut..]
        .iter()
        .map(String::as_str)
        .filter(|s| *s != "-")
        .collect();
    match tail.as_slice() {
        ["releases", tag] if *tag != "permalink" => src.channel = Channel::Tag((*tag).to_string()),
        _ => {}
    }
    Ok(parsed(src))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gh(owner: &str, repo: &str) -> Provider {
        Provider::GitHub {
            owner: owner.into(),
            repo: repo.into(),
        }
    }

    #[test]
    fn embedded_github_update_info() {
        let p =
            parse_input("gh-releases-zsync|lizzieshinkicker|Cacoco|nightly|*x86_64.AppImage.zsync")
                .unwrap();
        assert_eq!(p.source.provider, gh("lizzieshinkicker", "Cacoco"));
        assert_eq!(p.source.channel, Channel::Tag("nightly".into()));
        assert_eq!(p.source.asset_pattern.as_deref(), Some("*x86_64.AppImage"));
        assert_eq!(p.source.origin, Origin::Embedded);
        for (rel, ch) in [
            ("latest", Channel::Stable),
            ("latest-pre", Channel::Prerelease),
            ("latest-all", Channel::Any),
        ] {
            let p =
                parse_input(&format!("gh-releases-zsync|o|r|{rel}|a-*.AppImage.zsync")).unwrap();
            assert_eq!(p.source.channel, ch);
        }
        assert!(parse_input("gh-releases-zsync|o|r").is_err());
    }

    #[test]
    fn embedded_zsync_url() {
        let p = parse_input("zsync|https://downloads.cursor.com/x/cursor-latest.appimage.zsync")
            .unwrap();
        assert_eq!(
            p.source.provider,
            Provider::Zsync {
                url: "https://downloads.cursor.com/x/cursor-latest.appimage.zsync".into()
            }
        );
        let p = parse_input("https://example.com/a/Foo.AppImage.zsync").unwrap();
        assert!(matches!(p.source.provider, Provider::Zsync { .. }));
        assert!(parse_input("bintray-zsync|a|b|c|d").is_err());
    }

    #[test]
    fn github_forms_are_normalised() {
        // Gear Lever #436: a trailing slash must not break anything.
        for input in [
            "https://github.com/upscayl/upscayl/",
            "github.com/upscayl/upscayl",
            "http://www.github.com/upscayl/upscayl.git",
            "upscayl/upscayl",
            "gh:upscayl/upscayl",
            "https://github.com/upscayl/upscayl/releases",
            "https://github.com/upscayl/upscayl/releases/latest?x=1",
        ] {
            let p = parse_input(input).unwrap_or_else(|e| panic!("{input}: {e}"));
            assert_eq!(p.source.provider, gh("upscayl", "upscayl"), "{input}");
            assert_eq!(p.source.channel, Channel::Stable, "{input}");
        }
    }

    #[test]
    fn github_tag_and_download_links() {
        let p = parse_input("https://github.com/o/r/releases/tag/nightly").unwrap();
        assert_eq!(p.source.channel, Channel::Tag("nightly".into()));
        let p = parse_input(
            "https://github.com/o/r/releases/download/v1.2.3/Foo-1.2.3-x86_64.AppImage",
        )
        .unwrap();
        assert_eq!(p.source.channel, Channel::Tag("v1.2.3".into()));
        assert_eq!(
            p.source.asset_pattern.as_deref(),
            Some("Foo-*-x86_64.AppImage")
        );
        assert!(p.asset_url.is_some());
        let p = parse_input("https://github.com/o/r/releases/latest/download/Foo-x86_64.AppImage")
            .unwrap();
        assert_eq!(p.source.channel, Channel::Stable);
    }

    #[test]
    fn gitlab_and_forgejo() {
        let p = parse_input("https://gitlab.com/group/sub/proj/-/releases/v1").unwrap();
        assert_eq!(
            p.source.provider,
            Provider::GitLab {
                host: "gitlab.com".into(),
                project: "group/sub/proj".into()
            }
        );
        assert_eq!(p.source.channel, Channel::Tag("v1".into()));
        let p = parse_input("https://gitlab.com/group/proj.git").unwrap();
        assert_eq!(
            p.source.provider,
            Provider::GitLab {
                host: "gitlab.com".into(),
                project: "group/proj".into()
            }
        );
        let p = parse_input("https://codeberg.org/o/r/").unwrap();
        assert_eq!(
            p.source.provider,
            Provider::Forgejo {
                host: "codeberg.org".into(),
                owner: "o".into(),
                repo: "r".into()
            }
        );
        let p = parse_input("forgejo:https://git.example.org/o/r").unwrap();
        assert_eq!(
            p.source.provider,
            Provider::Forgejo {
                host: "git.example.org".into(),
                owner: "o".into(),
                repo: "r".into()
            }
        );
    }

    #[test]
    fn plain_urls_and_errors() {
        let p = parse_input("https://example.com/dl/Foo.AppImage?x=1").unwrap();
        assert_eq!(
            p.source.provider,
            Provider::Url {
                url: "https://example.com/dl/Foo.AppImage?x=1".into()
            }
        );
        // Download links are kept exactly: the query picks the file and `www.` may matter.
        let p = parse_input("https://www.vendor.com/dl?os=linux&fmt=appimage#top").unwrap();
        assert_eq!(
            p.source.provider,
            Provider::Url {
                url: "https://www.vendor.com/dl?os=linux&fmt=appimage".into()
            }
        );
        assert!(parse_input("").is_err());
        assert!(parse_input("ftp://example.com/x").is_err());
        assert!(parse_input("https://github.com/onlyowner").is_err());
    }

    #[test]
    fn describe() {
        let p = parse_input("o/r").unwrap();
        assert_eq!(p.source.describe(), "github.com/o/r [latest]");
    }
}
