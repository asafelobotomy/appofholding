//! The QObject the QML interface talks to. It owns no state of its own: every call opens the
//! library, runs one function from `service`, and hands JSON back. Slow calls (network,
//! unpacking) run on a worker thread and report through the `finished` / `progress` signals.

use core::pin::Pin;
use cxx_qt::Threading;
use cxx_qt_lib::QString;
use serde_json::{Value, json};

use crate::service::{self, Res};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(i32, tasks)]
        type Backend = super::BackendRust;

        // ---- reads: JSON, or {"error": "..."}
        #[qinvokable]
        fn apps(self: &Backend) -> QString;
        #[qinvokable]
        fn detail(self: &Backend, id: &QString) -> QString;
        #[cxx_name = "profileDetail"]
        #[qinvokable]
        fn profile_detail(self: &Backend, id: &QString, name: &QString) -> QString;
        #[cxx_name = "sandboxDefaults"]
        #[qinvokable]
        fn sandbox_defaults(self: &Backend, level: &QString) -> QString;
        #[qinvokable]
        fn presets(self: &Backend) -> QString;
        #[qinvokable]
        fn preview(self: &Backend, id: &QString, profile: &QString) -> QString;
        #[cxx_name = "scheduleStatus"]
        #[qinvokable]
        fn schedule_status(self: &Backend) -> QString;
        #[cxx_name = "dataDir"]
        #[qinvokable]
        fn data_dir(self: &Backend) -> QString;
        /// Files passed on the command line (e.g. by double-clicking an AppImage), as a JSON array.
        #[cxx_name = "startupFiles"]
        #[qinvokable]
        fn startup_files(self: &Backend) -> QString;
        /// Started with `--tray` (from the login autostart): stay in the tray.
        #[cxx_name = "startedInTray"]
        #[qinvokable]
        fn started_in_tray(self: &Backend) -> bool;
        /// Tray and startup settings: {tray, minimiseToTray, closeToTray, notifyUpdates,
        /// startInTray, autostart}.
        #[cxx_name = "desktopSettings"]
        #[qinvokable]
        fn desktop_settings(self: &Backend) -> QString;

        // ---- changes: "" on success, otherwise the error message
        #[cxx_name = "saveProfile"]
        #[qinvokable]
        fn save_profile(self: &Backend, id: &QString, name: &QString, json: &QString) -> QString;
        #[cxx_name = "deleteProfile"]
        #[qinvokable]
        fn delete_profile(self: &Backend, id: &QString, name: &QString) -> QString;
        #[cxx_name = "saveSandbox"]
        #[qinvokable]
        fn save_sandbox(self: &Backend, id: &QString, json: &QString) -> QString;
        #[cxx_name = "setPolicy"]
        #[qinvokable]
        fn set_policy(self: &Backend, id: &QString, policy: &QString) -> QString;
        #[qinvokable]
        fn skip(self: &Backend, id: &QString, clear: bool) -> QString;
        #[cxx_name = "setSource"]
        #[qinvokable]
        fn set_source(
            self: &Backend,
            id: &QString,
            input: &QString,
            pattern: &QString,
            channel: &QString,
        ) -> QString;
        #[cxx_name = "clearSource"]
        #[qinvokable]
        fn clear_source(self: &Backend, id: &QString) -> QString;
        #[qinvokable]
        fn rollback(self: &Backend, id: &QString, to: &QString) -> QString;
        #[qinvokable]
        fn launch(self: &Backend, id: &QString) -> QString;
        #[qinvokable]
        fn remove(self: &Backend, id: &QString, purge: bool) -> QString;
        #[cxx_name = "scheduleInstall"]
        #[qinvokable]
        fn schedule_install(self: &Backend, interval: &QString) -> QString;
        #[cxx_name = "scheduleRemove"]
        #[qinvokable]
        fn schedule_remove(self: &Backend) -> QString;
        /// The saved colour theme: system, gruvbox-dark or gruvbox-light.
        #[qinvokable]
        fn theme(self: &Backend) -> QString;
        #[cxx_name = "setTheme"]
        #[qinvokable]
        fn set_theme(self: &Backend, name: &QString) -> QString;
        /// The saved library layout: list, grid or compact.
        #[qinvokable]
        fn view(self: &Backend) -> QString;
        #[cxx_name = "setView"]
        #[qinvokable]
        fn set_view(self: &Backend, name: &QString) -> QString;
        /// Suggested improvements for an app: JSON array of {key, title, detail, action,
        /// dismissible}, or {"error": ...}.
        #[qinvokable]
        fn suggestions(self: &Backend, id: &QString) -> QString;
        /// Apply one: {"message": ...} or {"error": ...}.
        #[cxx_name = "applySuggestion"]
        #[qinvokable]
        fn apply_suggestion(self: &Backend, id: &QString, key: &QString) -> QString;
        #[cxx_name = "dismissSuggestion"]
        #[qinvokable]
        fn dismiss_suggestion(self: &Backend, id: &QString, key: &QString) -> QString;
        /// Start a fresh copy of the program (after an update); "" or the error. The caller
        /// then quits this one.
        #[qinvokable]
        fn relaunch(self: &Backend) -> QString;
        #[cxx_name = "importRestore"]
        #[qinvokable]
        fn import_restore(self: &Backend) -> QString;
        #[cxx_name = "setDesktopSettings"]
        #[qinvokable]
        fn set_desktop_settings(self: &Backend, json: &QString) -> QString;
        /// Answer later starts of the program (see `instance`); they arrive as `activated`.
        #[qinvokable]
        fn listen(self: Pin<&mut Backend>);
        #[cxx_name = "setHook"]
        #[qinvokable]
        fn set_hook(self: &Backend, id: &QString, kind: &QString, command: &QString) -> QString;

        // ---- slow work, answered through `finished`
        // kind: notifyCheck | check | update | resolve | install | addFile | prepare | selfCheck | selfUpdate | importScan | importRun
        #[qinvokable]
        fn start(
            self: Pin<&mut Backend>,
            kind: &QString,
            id: &QString,
            a: &QString,
            b: &QString,
            c: &QString,
        );

        #[qsignal]
        fn finished(
            self: Pin<&mut Backend>,
            kind: &QString,
            id: &QString,
            ok: bool,
            payload: &QString,
        );
        #[qsignal]
        fn progress(self: Pin<&mut Backend>, id: &QString, fraction: f64);
        /// Another start of the program handed over: files is a JSON array of AppImages to add,
        /// show says whether to bring the window up.
        #[qsignal]
        fn activated(self: Pin<&mut Backend>, files: &QString, show: bool);
    }

    impl cxx_qt::Threading for Backend {}
}

#[derive(Default)]
pub struct BackendRust {
    tasks: i32,
}

fn s(q: &QString) -> String {
    q.to_string()
}

fn q(text: &str) -> QString {
    QString::from(text)
}

fn json_or_err(r: Res<Value>) -> QString {
    match r {
        Ok(v) => q(&v.to_string()),
        Err(e) => q(&json!({ "error": e }).to_string()),
    }
}

fn unit(r: Res<()>) -> QString {
    match r {
        Ok(()) => q(""),
        Err(e) => q(&e),
    }
}

fn with_dirs<T>(f: impl FnOnce(&appofholding_core::Dirs) -> Res<T>) -> Res<T> {
    f(&appofholding_core::Dirs::detect().map_err(|e| e.to_string())?)
}

fn with_lib<T>(f: impl FnOnce(&appofholding_core::Library) -> Res<T>) -> Res<T> {
    f(&service::open_lib()?)
}

impl qobject::Backend {
    pub fn apps(&self) -> QString {
        json_or_err(with_lib(service::list_apps))
    }
    pub fn detail(&self, id: &QString) -> QString {
        json_or_err(with_lib(|l| service::app_detail(l, &s(id))))
    }
    pub fn profile_detail(&self, id: &QString, name: &QString) -> QString {
        json_or_err(with_lib(|l| service::profile_detail(l, &s(id), &s(name))))
    }
    pub fn sandbox_defaults(&self, level: &QString) -> QString {
        json_or_err(service::sandbox_defaults(&s(level)))
    }
    pub fn presets(&self) -> QString {
        q(&service::presets().to_string())
    }
    pub fn preview(&self, id: &QString, profile: &QString) -> QString {
        match with_lib(|l| service::preview(l, &s(id), &s(profile))) {
            Ok(t) => q(&json!({ "text": t }).to_string()),
            Err(e) => q(&json!({ "error": e }).to_string()),
        }
    }
    pub fn schedule_status(&self) -> QString {
        json_or_err(with_dirs(|d| Ok(service::schedule_status(d))))
    }
    pub fn data_dir(&self) -> QString {
        let d = appofholding_core::Dirs::detect()
            .map(|d| d.library_dir.display().to_string())
            .unwrap_or_default();
        q(&d)
    }

    pub fn startup_files(&self) -> QString {
        q(&json!(startup_file_list()).to_string())
    }
    pub fn started_in_tray(&self) -> bool {
        started_in_tray()
    }
    pub fn desktop_settings(&self) -> QString {
        json_or_err(with_dirs(|d| Ok(service::desktop_load(d))))
    }
    pub fn set_desktop_settings(&self, json: &QString) -> QString {
        unit((|| {
            let v: Value = serde_json::from_str(&s(json)).map_err(|e| e.to_string())?;
            with_dirs(|d| service::desktop_save(d, &v, &service::gui_exe()?))
        })())
    }
    pub fn listen(self: Pin<&mut Self>) {
        let Some(path) = crate::instance::socket_path() else {
            return;
        };
        let thread = self.qt_thread();
        crate::instance::listen(&path, move |req| {
            let files = json!(req.files).to_string();
            let _ = thread.queue(move |mut qobj: Pin<&mut qobject::Backend>| {
                qobj.as_mut().activated(&q(&files), req.show);
            });
        });
    }

    pub fn save_profile(&self, id: &QString, name: &QString, json: &QString) -> QString {
        unit((|| {
            let v: Value = serde_json::from_str(&s(json)).map_err(|e| e.to_string())?;
            with_lib(|l| service::save_profile(l, &s(id), &s(name), &v))
        })())
    }
    pub fn delete_profile(&self, id: &QString, name: &QString) -> QString {
        unit(with_lib(|l| service::delete_profile(l, &s(id), &s(name))))
    }
    pub fn save_sandbox(&self, id: &QString, json: &QString) -> QString {
        // Returns the saved settings (normalised: a tweaked preset becomes "custom"), or an error.
        json_or_err((|| {
            let v: Value = serde_json::from_str(&s(json)).map_err(|e| e.to_string())?;
            with_lib(|l| service::save_sandbox(l, &s(id), &v))
        })())
    }
    pub fn set_policy(&self, id: &QString, policy: &QString) -> QString {
        unit(with_lib(|l| service::set_policy(l, &s(id), &s(policy))))
    }
    pub fn skip(&self, id: &QString, clear: bool) -> QString {
        unit(with_lib(|l| {
            let id = s(id);
            if clear {
                l.clear_skip(&id).map_err(|e| e.to_string())
            } else {
                l.skip_latest(&id).map(|_| ()).map_err(|e| e.to_string())
            }
        }))
    }
    pub fn set_source(
        &self,
        id: &QString,
        input: &QString,
        pattern: &QString,
        channel: &QString,
    ) -> QString {
        unit(with_lib(|l| {
            service::set_source(l, &s(id), &s(input), &s(pattern), &s(channel))
        }))
    }
    pub fn clear_source(&self, id: &QString) -> QString {
        unit(with_lib(|l| {
            l.clear_source(&s(id))
                .map(|_| ())
                .map_err(|e| e.to_string())
        }))
    }
    pub fn rollback(&self, id: &QString, to: &QString) -> QString {
        unit(with_lib(|l| {
            let to = s(to);
            l.rollback(
                &s(id),
                if to.is_empty() {
                    None
                } else {
                    Some(to.as_str())
                },
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        }))
    }
    pub fn launch(&self, id: &QString) -> QString {
        unit(with_lib(|l| service::launch_app(l, &s(id))))
    }
    pub fn remove(&self, id: &QString, purge: bool) -> QString {
        unit(with_lib(|l| service::remove_app(l, &s(id), purge)))
    }
    pub fn schedule_install(&self, interval: &QString) -> QString {
        unit(with_dirs(|d| service::schedule_install(d, &s(interval))))
    }
    pub fn schedule_remove(&self) -> QString {
        unit(with_dirs(service::schedule_remove))
    }
    pub fn theme(&self) -> QString {
        q(&current_theme())
    }
    pub fn set_theme(&self, name: &QString) -> QString {
        unit(with_dirs(|d| service::theme_save(d, &s(name))))
    }
    pub fn view(&self) -> QString {
        q(&appofholding_core::Dirs::detect()
            .map(|d| service::view_load(&d))
            .unwrap_or_else(|_| "list".into()))
    }
    pub fn set_view(&self, name: &QString) -> QString {
        unit(with_dirs(|d| service::view_save(d, &s(name))))
    }
    pub fn suggestions(&self, id: &QString) -> QString {
        json_or_err(with_lib(|l| service::suggestions(l, &s(id))))
    }
    pub fn apply_suggestion(&self, id: &QString, key: &QString) -> QString {
        json_or_err(with_lib(|l| service::apply_suggestion(l, &s(id), &s(key))))
    }
    pub fn dismiss_suggestion(&self, id: &QString, key: &QString) -> QString {
        unit(with_lib(|l| {
            service::dismiss_suggestion(l, &s(id), &s(key))
        }))
    }
    pub fn relaunch(&self) -> QString {
        // Let the new copy take over the single-window socket instead of handing its start
        // back to this one, which is about to quit.
        if let Some(path) = crate::instance::socket_path() {
            crate::instance::release(&path);
        }
        // The AppImage file or the package's binary, which now hold the new version.
        unit(service::gui_exe().and_then(|exe| {
            std::process::Command::new(exe)
                .spawn()
                .map(|_| ())
                .map_err(|e| format!("could not start the new version: {e}"))
        }))
    }
    pub fn import_restore(&self) -> QString {
        json_or_err(with_lib(service::import_restore))
    }
    pub fn set_hook(&self, id: &QString, kind: &QString, command: &QString) -> QString {
        unit(with_lib(|l| {
            let (id, kind, cmd) = (s(id), s(kind), s(command));
            if cmd.trim().is_empty() {
                l.clear_hook(&id, &kind)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            } else {
                l.set_hook(&id, &kind, &cmd).map_err(|e| e.to_string())
            }
        }))
    }

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
        let n = *self.tasks() + 1;
        self.as_mut().set_tasks(n);
        // Tasks about one app report under its id; the others under "@<kind>", so an install
        // and a self-update never share a progress bar.
        let key = progress_key(&kind, &id);
        // Mark the task busy at once, so a second click cannot start the same work again before
        // the first real progress report arrives.
        self.as_mut().progress(&q(&key), -1.0);
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
                if visible {
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
                let left = (*qobj.tasks() - 1).max(0);
                qobj.as_mut().set_tasks(left);
                qobj.as_mut().finished(&q(&kind), &q(&id), ok, &q(&payload));
            });
        });
    }
}

/// The key a task's progress is reported under (see `start`).
pub fn progress_key(kind: &str, id: &str) -> String {
    if id.is_empty() {
        format!("@{kind}")
    } else {
        id.to_string()
    }
}

fn run_task(
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
        "notifyCheck" => with_lib(|l| service::notify_check(l, &http)),
        "check" => with_lib(|l| service::check_app(l, &http, id)),
        "update" => with_lib(|l| service::update_app(l, &http, id, progress)),
        // a = link, b = asset pattern, c = channel
        "resolve" => service::resolve_link(a, b, c),
        "install" => with_lib(|l| service::install_link(l, &http, a, b, c, progress)),
        "addFile" => with_lib(|l| service::add_file(l, a)),
        // a = "force" to skip the cache
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
        "prepare" => with_lib(|l| {
            l.prepare_launch(id)
                .map(|()| json!({"ok": true}))
                .map_err(|e| e.to_string())
        }),
        other => Err(format!("unknown task '{other}'")),
    }
}

/// Files passed on the command line (e.g. by double-clicking an AppImage), as absolute paths.
pub fn startup_file_list() -> Vec<String> {
    std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .map(|a| service::local_path(&a).to_string_lossy().into_owned())
        .collect()
}

pub fn started_in_tray() -> bool {
    std::env::args().skip(1).any(|a| a == service::TRAY_FLAG)
}

/// The saved colour theme, read before the interface starts (it decides the controls style).
pub fn current_theme() -> String {
    appofholding_core::Dirs::detect()
        .map(|d| service::theme_load(&d))
        .unwrap_or_else(|_| "system".into())
}
