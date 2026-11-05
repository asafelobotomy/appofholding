//! Comparing versions and deciding whether an update is available.

use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    UpToDate,
    UpdateAvailable,
    /// Not enough information to say (e.g. the installed version is unknown). We never claim
    /// an update in this case, to avoid phantom updates (Gear Lever #353).
    Unknown,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::UpToDate => "up-to-date",
            Status::UpdateAvailable => "update-available",
            Status::Unknown => "unknown",
        }
    }
    pub fn parse(s: &str) -> Option<Status> {
        Some(match s {
            "up-to-date" => Status::UpToDate,
            "update-available" => Status::UpdateAvailable,
            "unknown" => Status::Unknown,
            _ => return None,
        })
    }
}

/// `v1.2.3` / `release-1.2.3` / `1.2.3-beta1` -> numeric components, if there are any.
fn numeric_parts(v: &str) -> Option<Vec<u64>> {
    let start = v.find(|c: char| c.is_ascii_digit())?;
    let rest = &v[start..];
    let core: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let parts: Vec<u64> = core
        .split('.')
        .filter(|p| !p.is_empty())
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    if parts.is_empty() { None } else { Some(parts) }
}

fn normalize(v: &str) -> &str {
    v.trim().trim_start_matches(['v', 'V'])
}

/// Ordering when both look like dotted numbers; `None` when they can't be compared.
pub fn compare(a: &str, b: &str) -> Option<Ordering> {
    let (x, y) = (numeric_parts(a)?, numeric_parts(b)?);
    let n = x.len().max(y.len());
    for i in 0..n {
        let (p, q) = (
            x.get(i).copied().unwrap_or(0),
            y.get(i).copied().unwrap_or(0),
        );
        if p != q {
            return Some(p.cmp(&q));
        }
    }
    Some(Ordering::Equal)
}

pub struct Installed<'a> {
    pub version: Option<&'a str>,
    pub sha256: Option<&'a str>,
}

/// Decide the status of `latest` (tag/version + optional digest) against what is installed.
///
/// `opaque` marks identifiers (ETags, SHA-1s) that can only be compared for equality.
pub fn decide(
    installed: &Installed<'_>,
    latest_version: &str,
    latest_sha256: Option<&str>,
    opaque: bool,
) -> Status {
    // Identical content is always up to date, whatever the tags say.
    if let (Some(a), Some(b)) = (installed.sha256, latest_sha256)
        && a.eq_ignore_ascii_case(b) {
            return Status::UpToDate;
        }
    let Some(cur) = installed.version.filter(|v| !v.is_empty()) else {
        // Nothing to compare by version; a differing digest is still conclusive.
        return match (installed.sha256, latest_sha256) {
            (Some(_), Some(_)) => Status::UpdateAvailable,
            _ => Status::Unknown,
        };
    };
    if normalize(cur) == normalize(latest_version) {
        // Same tag, different content: a moving tag such as `nightly`.
        return match (installed.sha256, latest_sha256) {
            (Some(_), Some(_)) => Status::UpdateAvailable,
            _ => Status::UpToDate,
        };
    }
    if opaque {
        return Status::UpdateAvailable; // differs (equality was handled above)
    }
    match compare(cur, latest_version) {
        Some(Ordering::Less) => Status::UpdateAvailable,
        Some(_) => Status::UpToDate, // never "update" to an older version
        None => Status::UpdateAvailable, // e.g. nightly-20261105 vs nightly-20261106
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inst<'a>(v: Option<&'a str>, d: Option<&'a str>) -> Installed<'a> {
        Installed {
            version: v,
            sha256: d,
        }
    }

    #[test]
    fn comparison() {
        assert_eq!(compare("1.2.3", "1.10.0"), Some(Ordering::Less));
        assert_eq!(compare("v2.0", "2.0.0"), Some(Ordering::Equal));
        assert_eq!(
            compare("release-1.5", "release-1.4.9"),
            Some(Ordering::Greater)
        );
        assert_eq!(compare("nightly", "1.0"), None);
    }

    #[test]
    fn newer_older_and_equal() {
        assert_eq!(
            decide(&inst(Some("1.0.0"), None), "v1.1.0", None, false),
            Status::UpdateAvailable
        );
        assert_eq!(
            decide(&inst(Some("1.1.0"), None), "v1.1.0", None, false),
            Status::UpToDate
        );
        assert_eq!(
            decide(&inst(Some("2.0.0"), None), "1.9.0", None, false),
            Status::UpToDate
        );
    }

    #[test]
    fn unknown_installed_version_never_claims_update() {
        assert_eq!(
            decide(&inst(None, None), "1.1.0", None, false),
            Status::Unknown
        );
        assert_eq!(
            decide(&inst(None, None), "1.1.0", Some("abc"), false),
            Status::Unknown
        );
    }

    #[test]
    fn digest_wins() {
        assert_eq!(
            decide(&inst(Some("1.0"), Some("AA")), "9.9", Some("aa"), false),
            Status::UpToDate
        );
        assert_eq!(
            decide(
                &inst(Some("nightly"), Some("aa")),
                "nightly",
                Some("bb"),
                false
            ),
            Status::UpdateAvailable
        );
        assert_eq!(
            decide(&inst(None, Some("aa")), "1.0", Some("bb"), false),
            Status::UpdateAvailable
        );
    }

    #[test]
    fn opaque_identifiers_are_never_ordered() {
        // An ETag that happens to start with a smaller number must still count as a change.
        assert_eq!(
            decide(&inst(Some("9f3a"), None), "2b7c", None, true),
            Status::UpdateAvailable
        );
        assert_eq!(
            decide(&inst(Some("sha1:aa"), None), "sha1:aa", None, true),
            Status::UpToDate
        );
        assert_eq!(
            decide(&inst(None, None), "sha1:aa", None, true),
            Status::Unknown
        );
    }

    #[test]
    fn non_numeric_tags_differ() {
        assert_eq!(
            decide(
                &inst(Some("nightly-20261105"), None),
                "nightly-20261106",
                None,
                false
            ),
            Status::UpdateAvailable
        );
        assert_eq!(
            decide(&inst(Some("nightly"), None), "nightly", None, false),
            Status::UpToDate
        );
    }
}
