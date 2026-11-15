use super::*;
use crate::fixture::{self, Item};

fn found(img: &AppImage, desktop_path: &str, _unused: &str) -> Vec<(String, String)> {
    let path = desktop_path.to_string();
    let bytes = img.read_file(&path).unwrap().unwrap();
    let e = DesktopEntry::parse(&String::from_utf8_lossy(&bytes)).unwrap();
    let name = e.main("Name").unwrap_or("").to_string();
    let app = App {
        desktop_path: &path,
        entry: &e,
        name: &name,
    };
    detect(img, &app)
        .into_iter()
        .map(|c| (c.source.describe(), c.evidence))
        .collect()
}

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
    let urls = appstream::project_urls(xml);
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
    let got = found(&img, "/io.github.idowner.idrepo.desktop", "");
    let want = [
        (
            "github.com/yml/thing [latest]",
            "its electron-builder update settings",
        ),
        (
            "github.com/meta/thing [latest]",
            "the project link in its AppStream metadata",
        ),
        (
            "github.com/idowner/idrepo [latest]",
            "its app id io.github.idowner.idrepo",
        ),
    ];
    assert_eq!(got, want.map(|(a, b)| (a.to_string(), b.to_string())));
}

#[test]
fn nothing_is_found_in_a_plain_image() {
    let (_t, img) = image(vec![Item::File(
        "/thing.desktop",
        b"[Desktop Entry]\nType=Application\nName=Thing\nExec=thing\n".to_vec(),
    )]);
    assert!(found(&img, "/thing.desktop", "").is_empty());
}

#[test]
fn ids_come_from_every_desktop_file_name_and_the_icon() {
    // The chosen entry has a short name, but the image also carries it under its real id, and
    // the icon and window class use that id too (a Tauri layout).
    let entry = b"[Desktop Entry]\nType=Application\nName=emobie\nExec=emobie\nIcon=io.github.someone.emobie\nStartupWMClass=io.github.someone.emobie\n";
    let (_t, img) = image(vec![
        Item::File("/emobie.desktop", entry.to_vec()),
        Item::File("/io.github.someone.emobie.desktop", entry.to_vec()),
    ]);
    assert_eq!(
        found(&img, "/emobie.desktop", ""),
        [(
            "github.com/someone/emobie [latest]".to_string(),
            "its app id io.github.someone.emobie".to_string()
        )]
    );
}

#[test]
fn a_matching_project_link_inside_the_program_is_used() {
    let mut program = b"\x7fELF junk https://github.com/tauri-apps/tauri more ".to_vec();
    program.extend(b"https://github.com/someone/romforge0.15.0Refusing x ");
    program
        .extend(b"https://github.com/someone/romforge\0 https://github.com/someone/romforge.git");
    let (_t, img) = image(vec![
        Item::File(
            "/romforge.desktop",
            b"[Desktop Entry]\nType=Application\nName=romforge\nExec=romforge-app\nIcon=romforge-app\n"
                .to_vec(),
        ),
        Item::File("/usr/bin/romforge-app", program),
    ]);
    assert_eq!(
        found(&img, "/romforge.desktop", ""),
        [(
            "github.com/someone/romforge [latest]".to_string(),
            "the link to its project inside its program".to_string()
        )]
    );
}

#[test]
fn links_in_programs_must_name_the_app() {
    let names = vec!["thing".to_string()];
    let bytes = b"https://github.com/a/other https://gitlab.com/b/Thing https://example.com/c/thing https://github.com/d";
    assert_eq!(
        describe(&links_in_program(bytes, &names)),
        ["gitlab.com/b/Thing [latest]"]
    );
}

#[test]
fn app_names_and_resemblance() {
    let e = DesktopEntry::parse(
        "[Desktop Entry]\nType=Application\nName=AV Morning Star\nExec=/usr/bin/AV-Morning-Star %U\nIcon=av\n",
    )
    .unwrap();
    let app = App {
        desktop_path: "/com.github.o.avmorningstar.desktop",
        entry: &e,
        name: "AV Morning Star",
    };
    assert_eq!(app.names(), ["avmorningstar"]);
    assert!(resembles(
        "AV-Morning-Star-0.4.2-x86_64.AppImage",
        &app.names()
    ));
    assert!(!resembles("search-plugins-x86_64.AppImage", &app.names()));
}

#[test]
fn generic_names_never_count_as_resemblance() {
    let e = DesktopEntry::parse(
        "[Desktop Entry]\nType=Application\nName=App\nExec=app %U\nIcon=com.foo.app\n",
    )
    .unwrap();
    let app = App {
        desktop_path: "/linux.desktop",
        entry: &e,
        name: "App",
    };
    assert!(app.names().is_empty(), "{:?}", app.names());
    assert!(!resembles("Whatever-1.0-x86_64.AppImage", &app.names()));
    // Generic names passed in directly do not match either.
    let generic: Vec<String> = ["app", "image", "img", "linux", "x8664"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert!(!resembles("Other-1.0-linux-x86_64.AppImage", &generic));
}

#[test]
fn names_only_match_whole_words() {
    let names = vec!["note".to_string()];
    assert!(resembles("Note-2.1-x86_64.AppImage", &names));
    assert!(resembles("note_2.1.AppImage", &names));
    assert!(!resembles("Joplin-Notebook-2.1.AppImage", &names));
    assert!(!resembles("keynote-2.1.AppImage", &names));
    // Several words in a row, and camel case.
    let names = vec!["morningstar".to_string(), "app2".to_string()];
    assert!(resembles("MorningStar-0.4.2.AppImage", &names));
    assert!(resembles("Morning_Star-0.4.2.AppImage", &names));
    assert!(resembles("app2-1.4.0-x86_64.AppImage", &names));
    assert!(!resembles("happ2-1.4.0.AppImage", &names));
}
