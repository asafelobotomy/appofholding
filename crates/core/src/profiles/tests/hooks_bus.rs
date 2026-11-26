use super::*;

#[test]
fn a_hook_that_writes_a_lot_to_stderr_does_not_hang() {
    let e = env();
    // Far more than a pipe holds (64 KiB): without draining, the hook blocks until the timeout.
    e.lib
        .set_hook(
            "foo-app",
            "pre-update",
            "head -c 1000000 /dev/zero | tr '\\0' x >&2; echo; echo last words >&2; exit 1",
        )
        .unwrap();
    let start = Instant::now();
    let err = e
        .lib
        .run_hook("foo-app", "pre-update")
        .unwrap_err()
        .to_string();
    assert!(
        start.elapsed() < Duration::from_secs(30),
        "took {:?}",
        start.elapsed()
    );
    assert!(
        err.ends_with("last words"),
        "{}",
        &err[err.len().saturating_sub(80)..]
    );
}

#[test]
fn every_sandboxed_launch_gets_its_own_bus_socket() {
    let e = env();
    let mut s = Settings::for_level(Level::Standard);
    s.bus = Bus::Filtered;
    e.lib.set_sandbox("foo-app", &s).unwrap();
    let socket = || {
        let mut host = test_host(e.tmp.path());
        host.dbus_address = Some("unix:path=/run/user/1000/bus".into());
        let ctx = LaunchCtx {
            host: Some(host),
            ..Default::default()
        };
        let plan = e.lib.plan_launch("foo-app", None, &[], &ctx).unwrap();
        plan.helper.unwrap().ready
    };
    let (a, b) = (socket(), socket());
    assert_ne!(a, b);
    let name = a.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with(&format!("appofholding-foo-app-bus-{}-", std::process::id())),
        "{name}"
    );
}
