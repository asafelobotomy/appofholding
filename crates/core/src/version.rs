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

/// A version split into its numbers and, if it is one, its pre-release kind.
///
/// Numbers are the digit runs from the first digit on, separated by `.`, `-` or `_` as long as
/// another digit follows: `v1.2.3` -> 1,2,3; `nightly-2026-11-05` -> 2026,11,5;
/// `app2-1.4.0` -> 2,1,4,0. What follows (`-rc1`, `-beta.2`, `-x86_64`) is the suffix.
#[derive(Debug, PartialEq, Eq)]
struct Parsed {
    numbers: Vec<u64>,
    /// `None` for a release; otherwise (rank, number): dev < alpha < beta < pre < rc.
    pre: Option<(u8, u64)>,
}

fn parse(v: &str) -> Option<Parsed> {
    let start = v.find(|c: char| c.is_ascii_digit())?;
    let bytes = &v.as_bytes()[start..];
    let mut numbers = Vec::new();
    let mut i = 0;
    loop {
        let from = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        // Absurdly long digit runs (hashes) saturate rather than fail.
        numbers.push(
            std::str::from_utf8(&bytes[from..i])
                .ok()?
                .parse()
                .unwrap_or(u64::MAX),
        );
        let joins = i + 1 < bytes.len()
            && matches!(bytes[i], b'.' | b'-' | b'_')
            && bytes[i + 1].is_ascii_digit();
        if !joins {
            break;
        }
        i += 1;
    }
    let suffix = v[start + i..].to_ascii_lowercase();
    let pre = [
        ("dev", 0u8),
        ("alpha", 1),
        ("beta", 2),
        ("preview", 3),
        ("pre", 3),
        ("rc", 4),
    ]
    .iter()
    .find_map(|(word, rank)| {
        // Only a whole word counts: `rc1` and `-rc.1`, but not the `rc` in `source`.
        let at = suffix.match_indices(word).map(|(i, _)| i).find(|&i| {
            !suffix[..i].ends_with(|c: char| c.is_ascii_alphabetic())
                && !suffix[i + word.len()..].starts_with(|c: char| c.is_ascii_alphabetic())
        })?;
        let n: String = suffix[at + word.len()..]
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit())
            .collect();
        Some((*rank, n.parse().unwrap_or(0)))
    });
    Some(Parsed { numbers, pre })
}

fn normalize(v: &str) -> &str {
    v.trim().trim_start_matches(['v', 'V'])
}

/// Ordering when both contain version numbers; `None` when they can't be compared.
/// A pre-release sorts before its release: `2.0.0-rc1` < `2.0.0` < `2.0.1`.
pub fn compare(a: &str, b: &str) -> Option<Ordering> {
    let (x, y) = (parse(a)?, parse(b)?);
    let n = x.numbers.len().max(y.numbers.len());
    for i in 0..n {
        let (p, q) = (
            x.numbers.get(i).copied().unwrap_or(0),
            y.numbers.get(i).copied().unwrap_or(0),
        );
        if p != q {
            return Some(p.cmp(&q));
        }
    }
    Some(match (x.pre, y.pre) {
        (None, None) => Ordering::Equal,
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (Some(p), Some(q)) => p.cmp(&q),
    })
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
        && a.eq_ignore_ascii_case(b)
    {
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
        None => Status::UpdateAvailable, // e.g. `nightly-abc` vs `nightly-def`
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
        // Pre-releases come before their release.
        assert_eq!(compare("2.0.0-rc1", "v2.0.0"), Some(Ordering::Less));
        assert_eq!(compare("2.0.0-beta.2", "2.0.0-rc1"), Some(Ordering::Less));
        assert_eq!(compare("2.0.0-rc2", "2.0.0-rc10"), Some(Ordering::Less));
        assert_eq!(compare("2.0.0-alpha", "1.9.9"), Some(Ordering::Greater));
        // Dated builds and names with digits in them.
        assert_eq!(
            compare("nightly-2026-11-05", "nightly-2026-11-06"),
            Some(Ordering::Less)
        );
        assert_eq!(compare("app2-1.4.0", "app2-1.5.0"), Some(Ordering::Less));
        // Architecture suffixes are not pre-releases.
        assert_eq!(compare("1.2.0-x86_64", "1.2.0"), Some(Ordering::Equal));
        assert_eq!(compare("1.2.0-source", "1.2.0"), Some(Ordering::Equal));
        assert_eq!(compare("1.2.0-developer", "1.2.0"), Some(Ordering::Equal));
    }

    #[test]
    fn a_pre_release_user_is_offered_the_final_release() {
        assert_eq!(
            decide(&inst(Some("2.0.0-rc1"), None), "v2.0.0", None, false),
            Status::UpdateAvailable
        );
        assert_eq!(
            decide(&inst(Some("2.0.0"), None), "v2.0.0-rc1", None, false),
            Status::UpToDate
        );
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
