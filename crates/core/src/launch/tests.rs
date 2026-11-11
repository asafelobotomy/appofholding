use super::*;

fn p(f: impl FnOnce(&mut Profile)) -> Profile {
    let mut x = Profile::default();
    f(&mut x);
    x
}

fn argv(pl: &Plan) -> Vec<&str> {
    pl.argv.iter().map(String::as_str).collect()
}

#[test]
fn empty_profile_just_runs_the_appimage_with_passthrough() {
    let pl = plan(
        Path::new("/a/Foo.AppImage"),
        &Profile::default(),
        &["--x".into(), "file.txt".into()],
        None,
        &[],
    )
    .unwrap();
    assert_eq!(argv(&pl), ["/a/Foo.AppImage", "--x", "file.txt"]);
    assert!(pl.env.is_empty() && pl.cwd.is_none());
}

#[test]
fn order_is_prefix_app_presets_profile_args_passthrough() {
    let prof = p(|x| {
        x.prefix = vec!["gamemoderun".into(), "-v".into()];
        x.presets = vec!["electron-wayland".into()];
        x.args = vec!["--my-flag".into()];
    });
    let pl = plan(
        Path::new("/a/F.AppImage"),
        &prof,
        &["%file".into()],
        None,
        &[],
    )
    .unwrap();
    assert_eq!(
        argv(&pl),
        [
            "gamemoderun",
            "-v",
            "/a/F.AppImage",
            "--ozone-platform=wayland",
            "--enable-wayland-ime",
            "--my-flag",
            "%file"
        ]
    );
}

#[test]
fn explicit_env_beats_preset_env_and_extract_and_run_sets_the_variable() {
    let prof = p(|x| {
        x.presets = vec!["x11".into()];
        x.env.insert("GDK_BACKEND".into(), "wayland".into());
        x.env.insert("FOO".into(), "a b".into());
        x.extract_and_run = true;
    });
    let pl = plan(Path::new("/a/F.AppImage"), &prof, &[], None, &[]).unwrap();
    let get = |k: &str| pl.env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    assert_eq!(get("GDK_BACKEND"), Some("wayland"));
    assert_eq!(get("QT_QPA_PLATFORM"), Some("xcb"));
    assert_eq!(get("APPIMAGE_EXTRACT_AND_RUN"), Some("1"));
    assert!(pl.display().contains("FOO='a b'"), "{}", pl.display());
}

#[test]
fn elevation_uses_run0_with_session_and_app_environment() {
    let prof = p(|x| {
        x.elevated = true;
        x.cwd = Some("/srv".into());
        x.env.insert("K".into(), "v".into());
        x.prefix = vec!["nice".into()];
    });
    let session = vec![("DISPLAY".to_string(), ":0".to_string())];
    let pl = plan(
        Path::new("/a/F.AppImage"),
        &prof,
        &["arg".into()],
        Some(Elevator::Run0),
        &session,
    )
    .unwrap();
    assert_eq!(
        argv(&pl),
        [
            "run0",
            "--setenv=DISPLAY=:0",
            "--setenv=K=v",
            "--chdir=/srv",
            "nice",
            "/a/F.AppImage",
            "arg"
        ]
    );
    assert!(
        pl.env.is_empty() && pl.cwd.is_none(),
        "nothing is applied to the unprivileged launcher itself"
    );
}

#[test]
fn elevation_with_pkexec_goes_through_env() {
    let prof = p(|x| x.elevated = true);
    let pl = plan(
        Path::new("/a/F.AppImage"),
        &prof,
        &[],
        Some(Elevator::Pkexec),
        &[("DISPLAY".into(), ":1".into())],
    )
    .unwrap();
    assert_eq!(argv(&pl), ["pkexec", "env", "DISPLAY=:1", "/a/F.AppImage"]);
}

#[test]
fn elevation_without_a_tool_is_an_error_not_a_silent_unprivileged_launch() {
    let prof = p(|x| x.elevated = true);
    let err = plan(Path::new("/a/F.AppImage"), &prof, &[], None, &[])
        .unwrap_err()
        .to_string();
    assert!(err.contains("neither run0 nor pkexec"), "{err}");
}

#[test]
fn validation() {
    assert!(
        p(|x| x.presets = vec!["nope".into()])
            .validate()
            .unwrap_err()
            .to_string()
            .contains("available:")
    );
    assert!(
        p(|x| {
            x.env.insert("1BAD".into(), "x".into());
        })
        .validate()
        .is_err()
    );
    assert!(
        p(|x| {
            x.env.insert("GOOD_1".into(), "x".into());
        })
        .validate()
        .is_ok()
    );
    assert!(p(|x| x.args = vec!["a\0b".into()]).validate().is_err());
    assert!(
        valid_profile_name("debug-2")
            && !valid_profile_name("Debug")
            && !valid_profile_name("-x")
            && !valid_profile_name("")
    );
    assert!(
        PRESETS.iter().all(|p| valid_profile_name(p.name)),
        "built-in preset names follow the naming rule"
    );
}

#[test]
fn display_quotes_for_the_shell() {
    let pl = Plan {
        argv: vec!["/a b/F".into(), "it's".into()],
        cwd: Some("/x y".into()),
        ..Default::default()
    };
    assert_eq!(pl.display(), "cd '/x y' && '/a b/F' 'it'\\''s'");
}

#[test]
fn a_plan_really_runs_with_its_args_env_and_cwd() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("app.sh");
    std::fs::write(&script, "#!/bin/sh\nprintf '%s|' \"$@\" > out.txt\nprintf '%s|%s' \"$MY_VAR\" \"$(pwd)\" >> out.txt\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let work = dir.path().join("work");
    std::fs::create_dir(&work).unwrap();
    let prof = p(|x| {
        x.args = vec!["one two".into()];
        x.env.insert("MY_VAR".into(), "hello".into());
        x.cwd = Some(work.clone());
    });
    let pl = plan(&script, &prof, &["last".into()], None, &[]).unwrap();
    assert!(pl.command().status().unwrap().success());
    let out = std::fs::read_to_string(work.join("out.txt")).unwrap();
    assert_eq!(
        out,
        format!(
            "one two|last|hello|{}",
            std::fs::canonicalize(&work).unwrap().display()
        )
    );
}
