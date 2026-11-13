//! Guessing where an AppImage without embedded update information is published, from what it
//! carries inside: electron-builder's `app-update.yml`, the project links in its AppStream
//! metadata, and reverse-DNS app ids such as `io.github.OWNER.REPO`.
//!
//! These are only guesses. Nothing here goes online; a caller must confirm that a candidate
//! really publishes this app as an AppImage before using it (see `Library::apply_suggestion`).

use crate::image::AppImage;
use crate::source::{Provider, Source, parse_input};

/// One place the app may be published, and what pointed there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub source: Source,
    /// Where the hint came from, in words ("its electron-builder update settings").
    pub evidence: String,
}

/// Every candidate, most trustworthy first, without duplicates. `desktop_path` is the desktop
/// entry's path inside the image and `name` the app's display name.
pub fn detect(img: &AppImage, desktop_path: &str, name: &str) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let mut push = |source: Source, evidence: &str| {
        let key = source.describe().to_ascii_lowercase();
        if !out
            .iter()
            .any(|c| c.source.describe().to_ascii_lowercase() == key)
        {
            out.push(Candidate {
                source,
                evidence: evidence.to_string(),
            });
        }
    };

    for text in electron_update_files(img) {
        if let Some(s) = from_app_update_yml(&text) {
            push(s, "its electron-builder update settings");
        }
    }

    let metainfo = metainfo_files(img);
    for text in &metainfo {
        for url in appstream_urls(text) {
            if let Some(s) = from_project_url(&url) {
                push(s, "the project link in its AppStream metadata");
            }
        }
    }

    let desktop_id = desktop_path
        .rsplit('/')
        .next()
        .unwrap_or("")
        .trim_end_matches(".desktop")
        .to_string();
    let mut ids = vec![desktop_id];
    ids.extend(metainfo.iter().filter_map(|t| element(t, "id")));
    for id in ids {
        for s in from_reverse_dns(&id, name) {
            push(
                s,
                &format!("its app id {}", id.trim_end_matches(".desktop")),
            );
        }
    }
    out
}

/// electron-builder writes `resources/app-update.yml` next to `app.asar`, at the top of the
/// image or under `opt/<name>/` or `usr/lib/<name>/`.
fn electron_update_files(img: &AppImage) -> Vec<String> {
    let mut dirs = vec!["/".to_string()];
    for parent in ["/opt", "/usr/lib"] {
        dirs.extend(
            img.list_dir(parent)
                .into_iter()
                .filter(|(_, is_dir)| *is_dir)
                .map(|(n, _)| format!("{parent}/{n}")),
        );
    }
    dirs.iter()
        .filter_map(|d| {
            let p = format!("{}/resources/app-update.yml", d.trim_end_matches('/'));
            img.read_file(&p).ok().flatten()
        })
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .collect()
}

fn metainfo_files(img: &AppImage) -> Vec<String> {
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

/// `provider: github` with `owner` and `repo` (and no other `host`) is all we need. Other
/// providers (generic, s3, gitlab by numeric project id) cannot be followed.
pub(crate) fn from_app_update_yml(text: &str) -> Option<Source> {
    let get = |key: &str| {
        text.lines()
            .filter(|l| !l.starts_with([' ', '\t', '-', '#']))
            .filter_map(|l| l.split_once(':'))
            .find(|(k, _)| k.trim() == key)
            .map(|(_, v)| v.trim().trim_matches(['"', '\'']).to_string())
            .filter(|v| !v.is_empty())
    };
    if get("provider")?.as_str() != "github" {
        return None;
    }
    if get("host").is_some_and(|h| h != "github.com" && h != "api.github.com") {
        return None;
    }
    let (owner, repo) = (get("owner")?, get("repo")?);
    if !valid_segment(&owner) || !valid_segment(&repo) {
        return None;
    }
    Some(Source::new(Provider::GitHub { owner, repo }))
}

/// The `<url>` links of an AppStream file, the ones most likely to name the repository first.
pub(crate) fn appstream_urls(text: &str) -> Vec<String> {
    const ORDER: [&str; 5] = [
        "vcs-browser",
        "homepage",
        "bugtracker",
        "contribute",
        "help",
    ];
    let mut found: Vec<(usize, String)> = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("<url") {
        rest = &rest[start + 4..];
        let Some(gt) = rest.find('>') else { break };
        let attrs = &rest[..gt];
        let body = &rest[gt + 1..];
        let Some(end) = body.find("</url>") else {
            break;
        };
        let kind = attr(attrs, "type").unwrap_or_default();
        if let Some(rank) = ORDER.iter().position(|k| *k == kind) {
            found.push((rank, unescape(body[..end].trim())));
        }
        rest = &body[end..];
    }
    found.sort_by_key(|(rank, _)| *rank);
    found.into_iter().map(|(_, u)| u).collect()
}

fn attr(attrs: &str, name: &str) -> Option<String> {
    let at = attrs.find(&format!("{name}="))? + name.len() + 1;
    let rest = &attrs[at..];
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let rest = &rest[1..];
    Some(rest[..rest.find(quote)?].to_string())
}

/// The text of the first top-level-looking `<name>…</name>` element.
fn element(text: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&format!("</{name}>"))?;
    Some(unescape(text[start..start + end].trim())).filter(|s| !s.is_empty())
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// A link to a repository on GitHub, GitLab or a Forgejo/Gitea host, reduced to the repository
/// itself (a link to an issue list or a release page still names the project). Plain websites
/// are not sources.
pub(crate) fn from_project_url(url: &str) -> Option<Source> {
    let url = url.trim();
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return None;
    }
    let after = url.split_once("://")?.1;
    let after = after.split(['?', '#']).next()?;
    let mut segs = after.split('/').filter(|s| !s.is_empty());
    let host = segs.next()?.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    let rest: Vec<&str> = segs.collect();
    let repo_path = match host.as_str() {
        "github.com" | "codeberg.org" | "gitea.com" => rest.get(..2)?.join("/"),
        // GitLab projects can sit in subgroups; everything before `/-/` is the project.
        h if h == "gitlab.com" || h.starts_with("gitlab.") => {
            let cut = rest.iter().position(|s| *s == "-").unwrap_or(rest.len());
            if cut < 2 {
                return None;
            }
            rest[..cut].join("/")
        }
        _ => return None,
    };
    let source = parse_input(&format!("https://{host}/{repo_path}"))
        .ok()?
        .source;
    matches!(
        source.provider,
        Provider::GitHub { .. } | Provider::GitLab { .. } | Provider::Forgejo { .. }
    )
    .then_some(source)
}

/// `io.github.OWNER.REPO`, `com.github.OWNER.REPO`, `io.gitlab.OWNER.REPO` and
/// `page.codeberg.OWNER.REPO`. The id's last part is often a squashed form of the repository
/// name (`avmorningstar` for `av-morning-star`), and Flathub-style ids turn `-` into `_`, so a
/// few spellings are offered, the id's own first.
pub(crate) fn from_reverse_dns(id: &str, name: &str) -> Vec<Source> {
    let id = id.trim().trim_end_matches(".desktop");
    let parts: Vec<&str> = id.split('.').collect();
    if parts.len() < 4 {
        return vec![];
    }
    let forge = match (parts[0], parts[1]) {
        ("io" | "com", "github") => "github",
        ("io" | "com", "gitlab") => "gitlab",
        ("page" | "org", "codeberg") => "codeberg",
        _ => return vec![],
    };
    let owner_raw = parts[2];
    let repo_raw = parts[3..].join(".");
    let mut owners = vec![owner_raw.to_string()];
    if owner_raw.contains('_') {
        owners.push(owner_raw.replace('_', "-"));
    }
    let mut repos = vec![repo_raw.clone()];
    if repo_raw.contains('_') {
        repos.push(repo_raw.replace('_', "-"));
    }
    let slug = slug(name);
    if !slug.is_empty() && squash(&slug) == squash(&repo_raw) {
        repos.push(slug);
    }
    let mut out: Vec<Source> = Vec::new();
    for owner in &owners {
        for repo in &repos {
            if !valid_segment(owner) || !valid_segment(repo) {
                continue;
            }
            let provider = match forge {
                "github" => Provider::GitHub {
                    owner: owner.clone(),
                    repo: repo.clone(),
                },
                "gitlab" => Provider::GitLab {
                    host: "gitlab.com".into(),
                    project: format!("{owner}/{repo}"),
                },
                _ => Provider::Forgejo {
                    host: "codeberg.org".into(),
                    owner: owner.clone(),
                    repo: repo.clone(),
                },
            };
            let s = Source::new(provider);
            if !out
                .iter()
                .any(|o| o.describe().eq_ignore_ascii_case(&s.describe()))
            {
                out.push(s);
            }
        }
    }
    out
}

/// "AV Morning Star" -> "av-morning-star".
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

fn squash(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn valid_segment(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

#[cfg(test)]
mod tests;
