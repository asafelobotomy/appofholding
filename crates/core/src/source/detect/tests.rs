use super::*;
use crate::fixture::{self, Item};

fn describe(v: &[Source]) -> Vec<String> {
    v.iter().map(Source::describe).collect()
}

fn image(items: Vec<Item>) -> (tempfile::TempDir, AppImage) {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("x.AppImage");
    std::fs::write(&p, fixture::build_appimage(None, items)).unwrap();
    let img = AppImage::open(&p).unwrap();
    (tmp, img)
}

#[test]
fn electron_builder_github_settings_are_read() {
    let yml = "owner: someone\nrepo: thing\nprovider: github\nupdaterCacheDirName: thing-updater\n";
    let s = from_app_update_yml(yml).unwrap();
    assert_eq!(s.describe(), "github.com/someone/thing [latest]");
    assert!(from_app_update_yml("provider: generic\nurl: https://x/\n").is_none());
    assert!(from_app_update_yml("provider: github\nowner: o\nrepo: r\nhost: ghe.corp\n").is_none());
    assert!(from_app_update_yml("provider: github\nowner: o\n").is_none());
    assert!(from_app_update_yml("provider: 'github'\nowner: \"o\"\nrepo: r\n").is_some());
}

#[test]
fn appstream_links_prefer_the_repository() {
    let xml = r#"<component>
        <url type="homepage">https://example.org/</url>
        <url type="bugtracker">https://github.com/o/r/issues</url>
        <url type="vcs-browser">https://gitlab.com/g/sub/proj/-/tree/main</url>
      </component>"#;
    let urls = appstream_urls(xml);
    assert_eq!(urls[0], "https://gitlab.com/g/sub/proj/-/tree/main");
    let sources: Vec<Source> = urls.iter().filter_map(|u| from_project_url(u)).collect();
    assert_eq!(
        describe(&sources),
        ["gitlab.com/g/sub/proj [latest]", "github.com/o/r [latest]"]
    );
}

#[test]
fn project_urls_are_reduced_to_the_repository() {
    let d = |u: &str| from_project_url(u).map(|s| s.describe());
    assert_eq!(
        d("https://github.com/o/r/releases/tag/v1").as_deref(),
        Some("github.com/o/r [latest]")
    );
    assert_eq!(
        d("https://www.github.com/o/r.git").as_deref(),
        Some("github.com/o/r [latest]")
    );
    assert_eq!(
        d("https://codeberg.org/o/r").as_deref(),
        Some("codeberg.org/o/r [latest]")
    );
    assert_eq!(d("https://example.org/o/r"), None);
    assert_eq!(d("https://github.com/o"), None);
    assert_eq!(d("github.com/o/r"), None);
}

#[test]
fn reverse_dns_ids_give_owner_and_repository_spellings() {
    assert_eq!(
        describe(&from_reverse_dns(
            "com.github.asafelobotomy.avmorningstar",
            "AV Morning Star"
        )),
        [
            "github.com/asafelobotomy/avmorningstar [latest]",
            "github.com/asafelobotomy/av-morning-star [latest]"
        ]
    );
    assert_eq!(
        describe(&from_reverse_dns(
            "io.github.foo_bar.my_app.desktop",
            "Other"
        )),
        [
            "github.com/foo_bar/my_app [latest]",
            "github.com/foo_bar/my-app [latest]",
            "github.com/foo-bar/my_app [latest]",
            "github.com/foo-bar/my-app [latest]"
        ]
    );
    assert_eq!(
        describe(&from_reverse_dns("page.codeberg.o.r", "R")),
        ["codeberg.org/o/r [latest]"]
    );
    assert!(from_reverse_dns("org.kde.kate", "Kate").is_empty());
    assert!(from_reverse_dns("io.github.o", "x").is_empty());
}

#[test]
fn detect_reads_the_image_and_orders_by_trust() {
    let (_t, img) = image(vec![
        Item::File(
            "/io.github.idowner.idrepo.desktop",
            b"[Desktop Entry]\nType=Application\nName=Thing\nExec=thing\n".to_vec(),
        ),
        Item::File(
            "/opt/Thing/resources/app-update.yml",
            b"provider: github\nowner: yml\nrepo: thing\n".to_vec(),
        ),
        Item::File(
            "/usr/share/metainfo/thing.appdata.xml",
            br#"<component><id>io.github.idowner.idrepo</id>
                <url type="homepage">https://github.com/meta/thing</url>
                <url type="bugtracker">https://github.com/YML/Thing/issues</url></component>"#
                .to_vec(),
        ),
    ]);
    let found = detect(&img, "/io.github.idowner.idrepo.desktop", "Thing");
    let got: Vec<(String, &str)> = found
        .iter()
        .map(|c| (c.source.describe(), c.evidence.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            (
                "github.com/yml/thing [latest]".to_string(),
                "its electron-builder update settings"
            ),
            (
                "github.com/meta/thing [latest]".to_string(),
                "the project link in its AppStream metadata"
            ),
            (
                "github.com/idowner/idrepo [latest]".to_string(),
                "its app id io.github.idowner.idrepo"
            ),
        ]
    );
}

#[test]
fn nothing_is_found_in_a_plain_image() {
    let (_t, img) = image(vec![Item::File(
        "/thing.desktop",
        b"[Desktop Entry]\nType=Application\nName=Thing\nExec=thing\n".to_vec(),
    )]);
    assert!(detect(&img, "/thing.desktop", "Thing").is_empty());
}
