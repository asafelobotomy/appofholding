//! Choosing the right downloadable file among a release's assets.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: Option<u64>,
    /// Lowercase hex SHA-256, when the host publishes one.
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    One(Asset),
    /// Several assets match; the user must narrow it with a pattern.
    Ambiguous(Vec<Asset>),
    None,
}

impl Selection {
    pub fn asset(&self) -> Option<&Asset> {
        match self {
            Selection::One(a) => Some(a),
            _ => None,
        }
    }
}

/// Case-insensitive glob supporting `*` and `?`. The whole text must match.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
    Armv7,
    X86,
}

impl Arch {
    pub fn host() -> Arch {
        match std::env::consts::ARCH {
            "aarch64" => Arch::Aarch64,
            "arm" => Arch::Armv7,
            "x86" => Arch::X86,
            _ => Arch::X86_64,
        }
    }
}

/// The architecture an asset name advertises, if any.
pub fn detect_arch(name: &str) -> Option<Arch> {
    let n = name.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|k| n.contains(k));
    if has(&["x86_64", "x86-64", "amd64", "x64"]) {
        Some(Arch::X86_64)
    } else if has(&["aarch64", "arm64"]) {
        Some(Arch::Aarch64)
    } else if has(&["armhf", "armv7", "armv6"]) {
        Some(Arch::Armv7)
    } else if has(&["i386", "i686", "ia32", "x86"]) {
        Some(Arch::X86)
    } else {
        None
    }
}

fn is_appimage(name: &str) -> bool {
    name.to_lowercase().ends_with(".appimage")
}

/// Picks the asset to install/update from. With a `pattern`, only matching AppImage files are
/// considered; assets for other architectures are always excluded, and assets naming the host
/// architecture are preferred over unlabeled ones.
pub fn select(assets: &[Asset], pattern: Option<&str>, arch: Arch) -> Selection {
    let mut cands: Vec<&Asset> = assets
        .iter()
        .filter(|a| match pattern {
            Some(p) => glob_match(p, &a.name),
            None => is_appimage(&a.name),
        })
        .filter(|a| detect_arch(&a.name).is_none_or(|x| x == arch))
        .collect();
    // Prefer explicitly labeled assets when both labeled and unlabeled remain.
    if cands.iter().any(|a| detect_arch(&a.name) == Some(arch))
        && cands.iter().any(|a| detect_arch(&a.name).is_none())
    {
        cands.retain(|a| detect_arch(&a.name) == Some(arch));
    }
    match cands.as_slice() {
        [] => Selection::None,
        [one] => Selection::One((*one).clone()),
        many => Selection::Ambiguous(many.iter().map(|a| (*a).clone()).collect()),
    }
}

/// Turn a concrete file name into a pattern that survives version bumps:
/// `Foo-1.2.3-x86_64.AppImage` -> `Foo-*-x86_64.AppImage`.
pub fn suggest_pattern(file: &str) -> String {
    let mut out = String::new();
    let mut token = String::new();
    let flush = |token: &mut String, out: &mut String| {
        if token.is_empty() {
            return;
        }
        let versionish = token.chars().any(|c| c.is_ascii_digit())
            && (token.matches('.').count() >= 1
                && token
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit() || c == 'v' || c == 'V'));
        if versionish && !token.to_lowercase().ends_with("appimage") {
            if !out.ends_with('*') {
                out.push('*');
            }
        } else {
            out.push_str(token);
        }
        token.clear();
    };
    for c in file.chars() {
        if c == '-' || c == '_' {
            flush(&mut token, &mut out);
            out.push(c);
        } else {
            token.push(c);
        }
    }
    // The last token ("x86_64.AppImage") keeps its extension, so split it off first.
    if let Some((stem, ext)) = token.rsplit_once('.')
        && ext.eq_ignore_ascii_case("appimage") {
            let mut t = stem.to_string();
            flush(&mut t, &mut out);
            out.push('.');
            out.push_str(ext);
            return collapse(&out);
        }
    flush(&mut token, &mut out);
    collapse(&out)
}

fn collapse(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c == '*' && out.ends_with('*') {
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(name: &str) -> Asset {
        Asset {
            name: name.into(),
            url: format!("https://x/{name}"),
            size: None,
            sha256: None,
        }
    }

    #[test]
    fn globbing() {
        assert!(glob_match(
            "foo-*-linux.appimage",
            "Foo-1.2.3-linux.AppImage"
        ));
        assert!(glob_match("*x86_64.AppImage", "bar-x86_64.AppImage"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("foo-*.AppImage", "foo-1.0.AppImage.zsync"));
        assert!(!glob_match("foo", "foobar"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn arch_detection() {
        assert_eq!(detect_arch("App-1.0-x86_64.AppImage"), Some(Arch::X86_64));
        assert_eq!(detect_arch("App-1.0-aarch64.AppImage"), Some(Arch::Aarch64));
        assert_eq!(detect_arch("App-arm64.AppImage"), Some(Arch::Aarch64));
        assert_eq!(detect_arch("App-i386.AppImage"), Some(Arch::X86));
        assert_eq!(detect_arch("App.AppImage"), None);
    }

    #[test]
    fn selects_host_arch_and_ignores_sidecar_files() {
        let assets = vec![
            a("Foo-1.0-x86_64.AppImage"),
            a("Foo-1.0-aarch64.AppImage"),
            a("Foo-1.0-x86_64.AppImage.zsync"),
            a("Foo-1.0.tar.gz"),
            a("SHA256SUMS"),
        ];
        assert_eq!(
            select(&assets, None, Arch::X86_64),
            Selection::One(a("Foo-1.0-x86_64.AppImage"))
        );
        assert_eq!(
            select(&assets, None, Arch::Aarch64),
            Selection::One(a("Foo-1.0-aarch64.AppImage"))
        );
        assert_eq!(select(&assets, None, Arch::Armv7), Selection::None);
    }

    #[test]
    fn arm_suffix_mismatch_does_not_confuse_x86() {
        // Gear Lever #114: only the ARM file has a suffix.
        let assets = vec![a("Foo-1.0.AppImage"), a("Foo-1.0-arm64.AppImage")];
        assert_eq!(
            select(&assets, None, Arch::X86_64),
            Selection::One(a("Foo-1.0.AppImage"))
        );
        assert_eq!(
            select(&assets, None, Arch::Aarch64),
            Selection::One(a("Foo-1.0-arm64.AppImage"))
        );
    }

    #[test]
    fn ambiguity_is_reported_and_pattern_resolves_it() {
        let assets = vec![
            a("Foo-1.0-x86_64.AppImage"),
            a("Foo-lite-1.0-x86_64.AppImage"),
        ];
        assert!(
            matches!(select(&assets, None, Arch::X86_64), Selection::Ambiguous(v) if v.len() == 2)
        );
        // `Foo-*-x86_64.AppImage` also matches the lite build, so it stays ambiguous.
        assert!(matches!(
            select(&assets, Some("Foo-*-x86_64.AppImage"), Arch::X86_64),
            Selection::Ambiguous(_)
        ));
        assert_eq!(
            select(&assets, Some("Foo-[0-9]*"), Arch::X86_64),
            Selection::None,
            "brackets are literal, not supported"
        );
        assert_eq!(
            select(&assets, Some("Foo-1.*"), Arch::X86_64)
                .asset()
                .map(|x| x.name.as_str()),
            Some("Foo-1.0-x86_64.AppImage")
        );
    }

    #[test]
    fn pattern_suggestions() {
        assert_eq!(
            suggest_pattern("Foo-1.2.3-x86_64.AppImage"),
            "Foo-*-x86_64.AppImage"
        );
        assert_eq!(
            suggest_pattern("foo_v2.0.1_linux.AppImage"),
            "foo_*_linux.AppImage"
        );
        assert_eq!(suggest_pattern("Foo.AppImage"), "Foo.AppImage");
        assert_eq!(
            suggest_pattern("upscayl-2.15.0-linux.AppImage"),
            "upscayl-*-linux.AppImage"
        );
        assert_eq!(suggest_pattern("Foo-1.2.3.AppImage"), "Foo-*.AppImage");
    }
}
