use super::*;
use crate::http::fake::FakeHttp;

const GH_RELEASE: &str = r#"{
      "tag_name": "v1.2.0", "name": "One two", "prerelease": false, "draft": false,
      "published_at": "2026-11-01T00:00:00Z", "body": "notes",
      "assets": [
        {"name": "Foo-1.2.0-x86_64.AppImage", "browser_download_url": "https://dl/Foo-1.2.0-x86_64.AppImage", "size": 10,
         "digest": "sha256:ABCDEF"},
        {"name": "Foo-1.2.0-x86_64.AppImage.zsync", "browser_download_url": "https://dl/z", "size": 1, "digest": null}
      ]}"#;

fn src(s: &str) -> Source {
    crate::source::parse_input(s).unwrap().source
}

#[test]
fn github_latest() {
    let http = FakeHttp::default().json(
        "https://api.github.com/repos/o/r/releases/latest",
        GH_RELEASE,
    );
    let rel = fetch_release(&http, &src("o/r")).unwrap();
    assert_eq!(rel.tag, "v1.2.0");
    assert_eq!(rel.assets.len(), 2);
    assert_eq!(rel.assets[0].sha256.as_deref(), Some("abcdef"));
    assert_eq!(rel.assets[1].sha256, None);
}

#[test]
fn conditional_requests_return_not_modified() {
    let url = "https://api.github.com/repos/o/r/releases/latest";
    let mut http = FakeHttp::default().json(url, GH_RELEASE);
    http.responses
        .get_mut(url)
        .unwrap()
        .headers
        .push(("etag".into(), "W/\"abc\"".into()));
    let s = src("o/r");
    let Fetched::Fresh { etag, .. } = fetch_release_cond(&http, &s, None).unwrap() else {
        panic!("expected fresh")
    };
    assert_eq!(etag.as_deref(), Some("W/\"abc\""));
    assert!(matches!(
        fetch_release_cond(&http, &s, etag.as_deref()).unwrap(),
        Fetched::NotModified
    ));
    // A stale ETag gets the full release again.
    assert!(matches!(
        fetch_release_cond(&http, &s, Some("W/\"old\"")).unwrap(),
        Fetched::Fresh { .. }
    ));
}

#[test]
fn github_uses_tags_endpoint_for_moving_tags() {
    let http = FakeHttp::default().json(
        "https://api.github.com/repos/o/r/releases/tags/nightly",
        GH_RELEASE,
    );
    let rel = fetch_release(&http, &src("https://github.com/o/r/releases/tag/nightly")).unwrap();
    assert_eq!(rel.tag, "v1.2.0");
}

#[test]
fn github_prerelease_channels() {
    let list = r#"[
          {"tag_name":"v2.0.0-rc1","prerelease":true,"draft":false,"assets":[]},
          {"tag_name":"v1.9.0","prerelease":false,"draft":false,"assets":[]},
          {"tag_name":"v3.0.0-draft","prerelease":false,"draft":true,"assets":[]}
        ]"#;
    let http = FakeHttp::default().json(
        "https://api.github.com/repos/o/r/releases?per_page=30",
        list,
    );
    let pre = fetch_release(&http, &src("gh-releases-zsync|o|r|latest-pre|x.zsync")).unwrap();
    assert_eq!(pre.tag, "v2.0.0-rc1");
    let any = fetch_release(&http, &src("gh-releases-zsync|o|r|latest-all|x.zsync")).unwrap();
    assert_eq!(any.tag, "v2.0.0-rc1");
    let mut s = src("o/r");
    s.channel = Channel::Prerelease;
    assert_eq!(fetch_release(&http, &s).unwrap().tag, "v2.0.0-rc1");
}

#[test]
fn missing_prerelease_is_a_clear_error() {
    let http = FakeHttp::default().json(
        "https://api.github.com/repos/o/r/releases?per_page=30",
        r#"[{"tag_name":"v1","prerelease":false,"draft":false,"assets":[]}]"#,
    );
    let err = fetch_release(&http, &src("gh-releases-zsync|o|r|latest-pre|x.zsync")).unwrap_err();
    assert!(err.to_string().contains("no matching release"), "{err}");
}

#[test]
fn rate_limit_and_not_found() {
    let http = FakeHttp::default()
        .status(
            "https://api.github.com/repos/o/r/releases/latest",
            403,
            &[
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset", "1700000000"),
            ],
        )
        .status(
            "https://api.github.com/repos/o/gone/releases/latest",
            404,
            &[],
        );
    assert!(matches!(
        fetch_release(&http, &src("o/r")),
        Err(Error::RateLimited(Some(_)))
    ));
    assert!(
        fetch_release(&http, &src("o/gone"))
            .unwrap_err()
            .to_string()
            .contains("not found")
    );
}

#[test]
fn forgejo_release_list() {
    let list = r#"[{"tag_name":"v1.0","prerelease":false,"draft":false,
            "assets":[{"name":"A-x86_64.AppImage","browser_download_url":"https://cb/a","size":5}]}]"#;
    let http = FakeHttp::default().json(
        "https://codeberg.org/api/v1/repos/o/r/releases?limit=30",
        list,
    );
    let rel = fetch_release(&http, &src("https://codeberg.org/o/r")).unwrap();
    assert_eq!(rel.assets[0].url, "https://cb/a");
}

#[test]
fn gitlab_release_links_ignore_source_archives() {
    // Gear Lever #493: package assets and source archives must not break lookup.
    let list = r#"[{"tag_name":"v1.0","description":"d","released_at":"2026-01-01",
            "assets":{"count":3,"sources":[{"format":"zip","url":"https://gl/src.zip"}],
            "links":[{"name":"A.AppImage","url":"https://gl/a","direct_asset_url":"https://gl/direct/a","link_type":"package"}]}}]"#;
    let http = FakeHttp::default().json("https://gitlab.com/api/v4/projects/g%2Fp/releases", list);
    let rel = fetch_release(&http, &src("https://gitlab.com/g/p")).unwrap();
    assert_eq!(rel.assets.len(), 1);
    assert_eq!(rel.assets[0].url, "https://gl/direct/a");
}

#[test]
fn zsync_control_file_gives_real_url_and_identity() {
    // Modelled on the control file Cursor publishes: the AppImage's own URL differs from the
    // .zsync URL minus its suffix.
    let mut body = b"zsync: 0.6.2\nFilename: Cursor-3.28.7-x86_64.AppImage\nMTime: Tue, 03 Nov 2026 06:26:00 +0000\nBlocksize: 4096\nLength: 326855160\nHash-Lengths: 2,2,5\nURL: https://cdn.example/prod/abc/Cursor-3.28.7-x86_64.AppImage\nSHA-1: 6A27AC85\n\n".to_vec();
    body.extend([0xff, 0x00, 0x7f]);
    let http = FakeHttp::default().bytes("https://dl.example/cursor-latest.appimage.zsync", body);
    let s = src("zsync|https://dl.example/cursor-latest.appimage.zsync");
    let rel = fetch_release(&http, &s).unwrap();
    assert_eq!(rel.tag, "sha1:6a27ac85");
    assert!(rel.opaque);
    assert_eq!(rel.assets[0].name, "Cursor-3.28.7-x86_64.AppImage");
    assert_eq!(
        rel.assets[0].url,
        "https://cdn.example/prod/abc/Cursor-3.28.7-x86_64.AppImage"
    );
    assert_eq!(rel.assets[0].size, Some(326855160));
}

#[test]
fn zsync_relative_url_and_garbage() {
    let http = FakeHttp::default()
        .bytes(
            "https://h/d/a.AppImage.zsync",
            b"zsync: 0.6.2\nFilename: a.AppImage\nURL: a.AppImage\n\n".to_vec(),
        )
        .bytes("https://h/d/b.zsync", b"<html>nope</html>".to_vec());
    let rel = fetch_release(&http, &src("zsync|https://h/d/a.AppImage.zsync")).unwrap();
    assert_eq!(rel.assets[0].url, "https://h/d/a.AppImage");
    assert!(fetch_release(&http, &src("zsync|https://h/d/b.zsync")).is_err());
}

#[test]
fn plain_url_identity_prefers_etag() {
    let http = FakeHttp::default().status(
        "https://example.com/Foo.AppImage",
        200,
        &[
            ("etag", "W/\"abc123\""),
            ("last-modified", "Wed, 04 Nov 2026 10:00:00 GMT"),
            ("content-length", "99"),
        ],
    );
    let rel = fetch_release(&http, &src("https://example.com/Foo.AppImage")).unwrap();
    assert_eq!(rel.tag, "abc123");
    assert_eq!(rel.assets[0].name, "Foo.AppImage");
    assert_eq!(rel.assets[0].size, Some(99));
}
