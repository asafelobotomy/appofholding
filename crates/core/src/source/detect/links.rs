//! Turning what an AppImage carries (an `app-update.yml`, a project link, a reverse-DNS id)
//! into candidate sources.

use super::*;

/// electron-builder writes `resources/app-update.yml` next to `app.asar`, at the top of the
/// image or under `opt/<name>/` or `usr/lib/<name>/`.
pub(crate) fn electron_update_files(img: &AppImage) -> Vec<String> {
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
pub(crate) fn slug(name: &str) -> String {
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

pub(crate) fn valid_segment(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}
