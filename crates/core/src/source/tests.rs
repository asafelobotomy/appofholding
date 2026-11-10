use super::*;

fn gh(owner: &str, repo: &str) -> Provider {
    Provider::GitHub {
        owner: owner.into(),
        repo: repo.into(),
    }
}

#[test]
fn embedded_github_update_info() {
    let p = parse_input("gh-releases-zsync|lizzieshinkicker|Cacoco|nightly|*x86_64.AppImage.zsync")
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
        let p = parse_input(&format!("gh-releases-zsync|o|r|{rel}|a-*.AppImage.zsync")).unwrap();
        assert_eq!(p.source.channel, ch);
    }
    assert!(parse_input("gh-releases-zsync|o|r").is_err());
}

#[test]
fn embedded_zsync_url() {
    let p =
        parse_input("zsync|https://downloads.cursor.com/x/cursor-latest.appimage.zsync").unwrap();
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
    let p =
        parse_input("https://github.com/o/r/releases/download/v1.2.3/Foo-1.2.3-x86_64.AppImage")
            .unwrap();
    assert_eq!(p.source.channel, Channel::Tag("v1.2.3".into()));
    assert_eq!(
        p.source.asset_pattern.as_deref(),
        Some("Foo-*-x86_64.AppImage")
    );
    assert!(p.asset_url.is_some());
    let p =
        parse_input("https://github.com/o/r/releases/latest/download/Foo-x86_64.AppImage").unwrap();
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
