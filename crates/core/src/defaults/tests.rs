use super::*;
use crate::fixture;
use crate::library::AddOptions;
use crate::paths::Dirs;
use std::fs;

const UPD: &str = "gh-releases-zsync|o|foo|latest|Foo-*-x86_64.AppImage.zsync";

fn lib(tmp: &tempfile::TempDir) -> Library {
    Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap()
}

fn add(tmp: &tempfile::TempDir, lib: &Library, name: &str) -> String {
    let p = tmp.path().join(format!("{name}-1.0.0-x86_64.AppImage"));
    fs::write(&p, fixture::versioned_appimage(name, "1.0.0", Some(UPD))).unwrap();
    lib.add(&p, &AddOptions::default()).unwrap().id
}

#[test]
fn unset_defaults_are_the_built_in_ones() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(lib(&tmp).defaults().unwrap(), Defaults::default());
}

#[test]
fn new_apps_follow_the_default_policy_and_sandbox() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = lib(&tmp);
    let first = add(&tmp, &lib, "Foo App");
    lib.set_default_policy(Policy::Auto).unwrap();
    lib.set_default_sandbox(Level::Strict).unwrap();
    let second = add(&tmp, &lib, "Bar App");

    // The existing app keeps what it had; the new one starts with the defaults.
    assert_eq!(lib.source(&first).unwrap().unwrap().policy, Policy::Notify);
    assert!(!lib.sandbox_settings(&first).unwrap().enabled());
    assert_eq!(lib.source(&second).unwrap().unwrap().policy, Policy::Auto);
    assert_eq!(lib.sandbox_settings(&second).unwrap().level, Level::Strict);

    // Re-saving a source does not reset its policy.
    lib.set_policy(&second, Policy::Manual).unwrap();
    let s = lib.source(&second).unwrap().unwrap().source;
    lib.set_source(&second, &s).unwrap();
    assert_eq!(lib.source(&second).unwrap().unwrap().policy, Policy::Manual);

    assert_eq!(lib.set_policy_all(Policy::Pinned).unwrap(), 2);
    assert_eq!(lib.set_policy_all(Policy::Pinned).unwrap(), 0);
    assert_eq!(lib.source(&first).unwrap().unwrap().policy, Policy::Pinned);
}

#[test]
fn keep_versions_is_bounded_and_custom_is_no_default_sandbox() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = lib(&tmp);
    lib.set_keep_versions(5).unwrap();
    assert_eq!(lib.keep_versions().unwrap(), 5);
    assert!(lib.set_keep_versions(MAX_KEEP_VERSIONS + 1).is_err());
    assert_eq!(lib.keep_versions().unwrap(), 5);
    lib.set_keep_versions(0).unwrap();
    assert_eq!(lib.defaults().unwrap().keep_versions, 0);
    assert!(lib.set_default_sandbox(Level::Custom).is_err());
}
