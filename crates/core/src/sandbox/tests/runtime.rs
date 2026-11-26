use super::*;

#[test]
fn display_parsing_helpers() {
    assert_eq!(x11_socket(":1"), Some("/tmp/.X11-unix/X1".into()));
    assert_eq!(x11_socket(":0.0"), Some("/tmp/.X11-unix/X0".into()));
    assert_eq!(x11_socket("unix:2"), Some("/tmp/.X11-unix/X2".into()));
    assert_eq!(x11_socket("remote:10"), None);
    assert_eq!(
        dbus_path("unix:path=/run/user/1000/bus,guid=abc"),
        Some("/run/user/1000/bus".into())
    );
    assert_eq!(dbus_path("unix:abstract=/tmp/dbus-x"), None);
}

#[test]
fn network_on_keeps_the_hosts_interfaces() {
    if !bwrap_works() {
        return;
    }
    let r = rig(PROBE);
    let env: BTreeMap<String, String> =
        [("REAL_SECRET".to_string(), "/nonexistent".to_string())].into();
    let out = run_in(&r, &quiet(Level::Standard), &env, &[]);
    let host_ifaces = std::fs::read_to_string("/proc/net/dev")
        .unwrap()
        .lines()
        .filter(|l| l.contains(':'))
        .count();
    assert_eq!(field(&out, "NET"), host_ifaces.to_string());
}

#[test]
fn private_home_persists_between_runs_and_stays_off_the_real_home() {
    if !bwrap_works() {
        return;
    }
    let r = rig(
        "#!/bin/sh\nif [ -f \"$HOME/state\" ]; then echo \"STATE=$(cat $HOME/state)\"; else echo STATE=none; echo first > \"$HOME/state\"; fi\n",
    );
    let env = BTreeMap::new();
    let first = run_in(&r, &quiet(Level::Standard), &env, &[]);
    assert_eq!(field(&first, "STATE"), "none");
    let second = run_in(&r, &quiet(Level::Standard), &env, &[]);
    assert_eq!(field(&second, "STATE"), "first");
    assert!(r.private.join("state").exists());
    assert!(
        !r.home.join("state").exists(),
        "nothing was written to the real home"
    );
}

#[test]
fn exit_codes_pass_through() {
    if !bwrap_works() {
        return;
    }
    let r = rig("#!/bin/sh\nexit 7\n");
    let out = run_in(&r, &quiet(Level::Strict), &BTreeMap::new(), &[]);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn disabled_settings_refuse_to_build() {
    let env = BTreeMap::new();
    assert!(build(&Settings::default(), &host(FULL), &launch(&env, &[])).is_err());
}
