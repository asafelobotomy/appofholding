use super::*;

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
