//! Slow work on a worker thread: `start`, and the task it runs.

use super::*;

impl qobject::Backend {
    /// Start slow work on a worker thread; the answer arrives through `finished`.
    pub fn start(
        mut self: Pin<&mut Self>,
        kind: &QString,
        id: &QString,
        a: &QString,
        b: &QString,
        c: &QString,
    ) {
        let (kind, id, a, b, c) = (s(kind), s(id), s(a), s(b), s(c));
        // Background reads (see `QUIET_TASKS`) neither count as work nor show progress.
        let quiet = QUIET_TASKS.contains(&kind.as_str());
        if !quiet {
            let n = *self.tasks() + 1;
            self.as_mut().set_tasks(n);
        }
        // Tasks about one app report under its id; the others under "@<kind>", so an install
        // and a self-update never share a progress bar.
        let key = progress_key(&kind, &id);
        // Mark the task busy at once, so a second click cannot start the same work again before
        // the first real progress report arrives.
        if !quiet {
            self.as_mut().progress(&q(&key), -1.0);
        }
        let thread = self.qt_thread();
        std::thread::spawn(move || {
            let progress_thread = thread.clone();
            let progress_id = key.clone();
            let mut last = -1.0f64;
            let mut last_at = std::time::Instant::now();
            let mut progress = move |done: u64, total: Option<u64>| {
                let f = total
                    .filter(|t| *t > 0)
                    .map_or(-1.0, |t| done as f64 / t as f64);
                // Only tell the interface about visible changes; without a known size, a few
                // times a second is plenty.
                let visible = if f < 0.0 {
                    last >= 0.0 || last_at.elapsed() >= std::time::Duration::from_millis(250)
                } else {
                    (f - last).abs() >= 0.005
                };
                if visible && !quiet {
                    last = f;
                    last_at = std::time::Instant::now();
                    let pid = progress_id.clone();
                    let _ = progress_thread.queue(move |mut qobj: Pin<&mut qobject::Backend>| {
                        qobj.as_mut().progress(&q(&pid), f);
                    });
                }
            };
            // A bug in a task must not leave the interface waiting forever.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_task(&kind, &id, &a, &b, &c, &mut progress)
            }))
            .unwrap_or_else(|panic| {
                let what = panic
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| panic.downcast_ref::<&str>().copied())
                    .unwrap_or("unknown error");
                Err(format!("internal error in {kind}: {what}"))
            });
            let (ok, payload) = match result {
                Ok(v) => (true, v.to_string()),
                Err(e) => (false, e),
            };
            let _ = thread.queue(move |mut qobj: Pin<&mut qobject::Backend>| {
                if !quiet {
                    let left = (*qobj.tasks() - 1).max(0);
                    qobj.as_mut().set_tasks(left);
                }
                qobj.as_mut().finished(&q(&kind), &q(&id), ok, &q(&payload));
            });
        });
    }
}

/// Tasks that only read, in the background, for one page: they are not work the user started,
/// so they do not count in `tasks` (which disables buttons) or show progress.
pub(crate) const QUIET_TASKS: [&str; 1] = ["suggestions"];

/// The key a task's progress is reported under (see `start`).
pub fn progress_key(kind: &str, id: &str) -> String {
    if id.is_empty() {
        format!("@{kind}")
    } else {
        id.to_string()
    }
}

pub(crate) fn run_task(
    kind: &str,
    id: &str,
    a: &str,
    b: &str,
    c: &str,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Res<Value> {
    use appofholding_core::http::UreqHttp;
    let http = UreqHttp::new();
    match kind {
        // a = "force" when the user asked (not the background timer)
        "notifyCheck" => with_lib(|l| service::notify_check(l, &http, a == "force")),
        "check" => with_lib(|l| service::check_app(l, &http, id)),
        "update" => with_lib(|l| service::update_app(l, &http, id, progress)),
        // oysterAV scans the app's AppImage; waits for it, so it is a task. Answers {state, detail}.
        "scan" => with_lib(|l| service::scan_app(l, id)),
        // a = link, b = asset pattern, c = channel
        "resolve" => service::resolve_link(a, b, c),
        "install" => with_lib(|l| service::install_link(l, &http, a, b, c, progress)),
        // a = path or file:// URL, b = move | copy | in-place
        "addFile" => with_lib(|l| service::add_file(l, a, b)),
        // id = the app, a = the file, b = "move" or "copy"
        "updateFromFile" => with_lib(|l| service::update_from_file(l, id, a, b)),
        // a = options JSON {mode, replaceEntries}; b = JSON array of the chosen file paths
        "importScan" => {
            let opts: Value = serde_json::from_str(a).map_err(|e| e.to_string())?;
            let home = std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .ok_or("$HOME is not set")?;
            with_lib(|l| service::import_scan(l, &home, &opts))
        }
        "importRun" => {
            let opts: Value = serde_json::from_str(a).map_err(|e| e.to_string())?;
            let paths: Vec<String> = serde_json::from_str(b).map_err(|e| e.to_string())?;
            let home = std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .ok_or("$HOME is not set")?;
            with_lib(|l| service::import_run(l, &home, &opts, &paths))
        }
        // a = "force" to skip the cache
        "selfCheck" => service::self_check(
            &http,
            appofholding_core::selfupdate::detect_current(),
            a == "force",
        ),
        "selfUpdate" => service::self_update(
            &http,
            appofholding_core::selfupdate::detect_current(),
            &service::run_privileged,
            progress,
        ),
        // a = suggestion key; payload {"message": ...}. A task because applying one may go
        // online (to confirm a detected update source).
        "applySuggestion" => with_lib(|l| service::apply_suggestion(l, &http, id, a)),
        // Suggested improvements for app a (unpacks the AppImage, so not on the interface
        // thread): {"id": a, "list": [{key, title, detail, action, dismissible}]}, or
        // {"id": a, "error": ...}; always ok, so the page can tell which app it is about.
        // Started without an id, so it never touches that app's progress.
        "suggestions" => Ok(match with_lib(|l| service::suggestions(l, a)) {
            Ok(list) => json!({ "id": a, "list": list }),
            Err(e) => json!({ "id": a, "error": e }),
        }),
        // systemctl can take a while. a = hourly | daily | weekly
        "scheduleInstall" => with_dirs(|d| service::schedule_install(d, a)).map(|()| json!({})),
        "scheduleRemove" => with_dirs(service::schedule_remove).map(|()| json!({})),
        other => Err(format!("unknown task '{other}'")),
    }
}
