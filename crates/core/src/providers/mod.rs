//! Fetching release information from GitHub, GitLab, Forgejo/Gitea and plain URLs.

use serde::Deserialize;

use crate::assets::Asset;
use crate::error::{Error, Result};
use crate::http::{Http, Response};
use crate::source::{Channel, Provider, Source};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Tag name, or for plain URLs a change identifier (ETag / Last-Modified / length).
    pub tag: String,
    pub name: Option<String>,
    pub prerelease: bool,
    pub published_at: Option<String>,
    pub notes: Option<String>,
    pub assets: Vec<Asset>,
    /// `tag` is an opaque identifier (ETag, SHA-1) that can only be compared for equality, not
    /// ordered like a version number.
    pub opaque: bool,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    name: Option<String>,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    published_at: Option<String>,
    body: Option<String>,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    size: Option<u64>,
    digest: Option<String>,
}

#[derive(Deserialize)]
struct GlRelease {
    tag_name: String,
    name: Option<String>,
    description: Option<String>,
    released_at: Option<String>,
    #[serde(default)]
    upcoming_release: bool,
    #[serde(default)]
    assets: GlAssets,
}

#[derive(Deserialize, Default)]
struct GlAssets {
    #[serde(default)]
    links: Vec<GlLink>,
}

#[derive(Deserialize)]
struct GlLink {
    name: String,
    url: String,
    direct_asset_url: Option<String>,
}

/// `sha256:ABC…` -> `abc…`; other algorithms are ignored.
fn parse_digest(d: Option<String>) -> Option<String> {
    d?.strip_prefix("sha256:").map(|h| h.to_ascii_lowercase())
}

impl From<ApiRelease> for Release {
    fn from(r: ApiRelease) -> Self {
        Release {
            tag: r.tag_name,
            name: r.name.filter(|n| !n.is_empty()),
            prerelease: r.prerelease,
            published_at: r.published_at,
            notes: r.body.filter(|b| !b.is_empty()),
            opaque: false,
            assets: r
                .assets
                .into_iter()
                .map(|a| Asset {
                    name: a.name,
                    url: a.browser_download_url,
                    size: a.size,
                    sha256: parse_digest(a.digest),
                })
                .collect(),
        }
    }
}

impl From<GlRelease> for Release {
    fn from(r: GlRelease) -> Self {
        Release {
            tag: r.tag_name,
            name: r.name.filter(|n| !n.is_empty()),
            prerelease: r.upcoming_release,
            published_at: r.released_at,
            notes: r.description.filter(|b| !b.is_empty()),
            opaque: false,
            // Only release "links" are downloadable binaries; the auto-generated source
            // archives are intentionally ignored.
            assets: r
                .assets
                .links
                .into_iter()
                .map(|l| Asset {
                    name: l.name,
                    url: l.direct_asset_url.unwrap_or(l.url),
                    size: None,
                    sha256: None,
                })
                .collect(),
        }
    }
}

const JSON: &[(&str, &str)] = &[("Accept", "application/vnd.github+json, application/json")];

fn get_json(http: &dyn Http, url: &str) -> Result<Response> {
    get_json_cond(http, url, None)
}

/// Like [`get_json`], but sends `If-None-Match`; a `304 Not Modified` is returned as a normal
/// response for the caller to interpret.
fn get_json_cond(http: &dyn Http, url: &str, etag: Option<&str>) -> Result<Response> {
    let mut headers = JSON.to_vec();
    if let Some(e) = etag {
        headers.push(("If-None-Match", e));
    }
    let r = http.get(url, &headers)?;
    match r.status {
        200..=299 | 304 => Ok(r),
        404 => Err(Error::Source(format!(
            "not found: {url} (check the repository and release name)"
        ))),
        403 | 429 => {
            if r.header("x-ratelimit-remaining") == Some("0") || r.status == 429 {
                Err(Error::RateLimited(
                    r.header("x-ratelimit-reset").map(str::to_string),
                ))
            } else {
                Err(Error::Http(format!("HTTP {} from {url}", r.status)))
            }
        }
        s => Err(Error::Http(format!("HTTP {s} from {url}"))),
    }
}

fn parse<T: for<'de> Deserialize<'de>>(r: &Response, url: &str) -> Result<T> {
    serde_json::from_slice(&r.body)
        .map_err(|e| Error::Source(format!("unexpected response from {url}: {e}")))
}

fn pick(releases: Vec<Release>, channel: &Channel) -> Option<Release> {
    // APIs list newest first.
    match channel {
        Channel::Stable => releases.into_iter().find(|r| !r.prerelease),
        Channel::Prerelease => releases.into_iter().find(|r| r.prerelease),
        Channel::Any => releases.into_iter().next(),
        Channel::Tag(t) => releases.into_iter().find(|r| &r.tag == t),
    }
}

/// Outcome of a conditional fetch.
#[derive(Debug, Clone)]
pub enum Fetched {
    Fresh {
        release: Release,
        etag: Option<String>,
    },
    /// The server confirmed nothing changed since `etag` (does not count against GitHub's rate limit).
    NotModified,
}

/// Fetch with an optional ETag from a previous fetch. Only GitHub supports this today; other
/// providers always return `Fresh`.
pub fn fetch_release_cond(http: &dyn Http, source: &Source, etag: Option<&str>) -> Result<Fetched> {
    match &source.provider {
        Provider::GitHub { owner, repo } => github(http, owner, repo, &source.channel, etag),
        _ => fetch_release(http, source).map(|release| Fetched::Fresh {
            release,
            etag: None,
        }),
    }
}

pub fn fetch_release(http: &dyn Http, source: &Source) -> Result<Release> {
    match &source.provider {
        Provider::GitHub { owner, repo } => {
            match github(http, owner, repo, &source.channel, None)? {
                Fetched::Fresh { release, .. } => Ok(release),
                Fetched::NotModified => Err(Error::Http("unexpected 304 Not Modified".into())),
            }
        }
        Provider::Forgejo { host, owner, repo } => {
            forgejo(http, host, owner, repo, &source.channel)
        }
        Provider::GitLab { host, project } => gitlab(http, host, project, &source.channel),
        Provider::Url { url } => plain_url(http, url),
        Provider::Zsync { url } => zsync(http, url),
    }
}

fn none_found(what: &str, channel: &Channel) -> Error {
    Error::Source(format!(
        "no matching release found in {what} for '{channel}'"
    ))
}

fn github(
    http: &dyn Http,
    owner: &str,
    repo: &str,
    channel: &Channel,
    etag: Option<&str>,
) -> Result<Fetched> {
    let base = format!("https://api.github.com/repos/{owner}/{repo}");
    // Tags (including moving tags like `nightly`) live under /releases/tags/, not /releases/<tag>
    // (Gear Lever #388).
    let url = match channel {
        Channel::Stable => format!("{base}/releases/latest"),
        Channel::Tag(t) => format!("{base}/releases/tags/{t}"),
        _ => format!("{base}/releases?per_page=30"),
    };
    let r = get_json_cond(http, &url, etag)?;
    if r.status == 304 {
        return Ok(Fetched::NotModified);
    }
    let new_etag = r.header("etag").map(str::to_string);
    let release = if matches!(channel, Channel::Stable | Channel::Tag(_)) {
        let rel: ApiRelease = parse(&r, &url)?;
        rel.into()
    } else {
        let list: Vec<ApiRelease> = parse(&r, &url)?;
        pick(
            list.into_iter()
                .filter(|r| !r.draft)
                .map(Release::from)
                .collect(),
            channel,
        )
        .ok_or_else(|| none_found(&format!("{owner}/{repo}"), channel))?
    };
    Ok(Fetched::Fresh {
        release,
        etag: new_etag,
    })
}

fn forgejo(
    http: &dyn Http,
    host: &str,
    owner: &str,
    repo: &str,
    channel: &Channel,
) -> Result<Release> {
    let url = format!("https://{host}/api/v1/repos/{owner}/{repo}/releases?limit=30");
    let r = get_json(http, &url)?;
    let list: Vec<ApiRelease> = parse(&r, &url)?;
    pick(
        list.into_iter()
            .filter(|r| !r.draft)
            .map(Release::from)
            .collect(),
        channel,
    )
    .ok_or_else(|| none_found(&format!("{owner}/{repo}"), channel))
}

fn gitlab(http: &dyn Http, host: &str, project: &str, channel: &Channel) -> Result<Release> {
    let url = format!(
        "https://{host}/api/v4/projects/{}/releases",
        project.replace('/', "%2F")
    );
    let r = get_json(http, &url)?;
    let list: Vec<GlRelease> = parse(&r, &url)?;
    pick(list.into_iter().map(Release::from).collect(), channel)
        .ok_or_else(|| none_found(project, channel))
}

fn plain_url(http: &dyn Http, url: &str) -> Result<Release> {
    let r = http.head(url, &[])?;
    if !(200..300).contains(&r.status) {
        return Err(Error::Http(format!("HTTP {} from {url}", r.status)));
    }
    // Whatever the server offers to tell a changed file from an unchanged one.
    let ident = r
        .header("etag")
        .map(|e| e.trim_start_matches("W/").trim_matches('"').to_string())
        .or_else(|| r.header("last-modified").map(str::to_string))
        .or_else(|| r.header("content-length").map(|l| format!("size-{l}")))
        .unwrap_or_default();
    let file = url
        .split('/')
        .next_back()
        .filter(|f| !f.is_empty())
        .unwrap_or("download.AppImage");
    Ok(Release {
        tag: ident,
        name: None,
        prerelease: false,
        published_at: r.header("last-modified").map(str::to_string),
        notes: None,
        opaque: true,
        assets: vec![Asset {
            name: file.to_string(),
            url: url.to_string(),
            size: r.header("content-length").and_then(|l| l.parse().ok()),
            sha256: None,
        }],
    })
}

/// Read a `.zsync` control file. Its text header names the real download URL, file name,
/// length and SHA-1 of the current build, which is all we need to detect an update without
/// downloading anything large (and is how AppImage authors publish "latest" files).
fn zsync(http: &dyn Http, url: &str) -> Result<Release> {
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

#[cfg(test)]
mod tests;
