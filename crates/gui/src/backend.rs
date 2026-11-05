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
        #[cxx_name = "setHook"]
        #[qinvokable]
        fn set_hook(self: &Backend, id: &QString, kind: &QString, command: &QString) -> QString;

        // ---- slow work, answered through `finished`
        // kind: checkAll | check | update | resolve | install | addFile | prepare | selfCheck | selfUpdate
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
        json_or_err(
            appofholding_core::Dirs::detect()
                .map(|d| service::schedule_status(&d))
                .map_err(|e| e.to_string()),
        )
    }
    pub fn data_dir(&self) -> QString {
        let d = appofholding_core::Dirs::detect()
            .map(|d| d.library_dir.display().to_string())
            .unwrap_or_default();
        q(&d)
    }

    pub fn startup_files(&self) -> QString {
        let files: Vec<String> = std::env::args()
            .skip(1)
            .filter(|a| !a.starts_with('-'))
            .map(|a| a.strip_prefix("file://").unwrap_or(&a).to_string())
            .collect();
        q(&json!(files).to_string())
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
        unit(
            appofholding_core::Dirs::detect()
                .map_err(|e| e.to_string())
                .and_then(|d| service::schedule_install(&d, &s(interval))),
        )
    }
    pub fn schedule_remove(&self) -> QString {
        unit(
            appofholding_core::Dirs::detect()
                .map_err(|e| e.to_string())
                .and_then(|d| service::schedule_remove(&d)),
        )
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
        let thread = self.qt_thread();
        std::thread::spawn(move || {
            let progress_thread = thread.clone();
            let progress_id = id.clone();
            let mut last = -1.0f64;
            let mut progress = move |done: u64, total: Option<u64>| {
                let f = total
                    .filter(|t| *t > 0)
                    .map_or(-1.0, |t| done as f64 / t as f64);
                // Only tell the interface about visible changes.
                if (f - last).abs() >= 0.005 || f < 0.0 {
                    last = f;
                    let pid = progress_id.clone();
                    let _ = progress_thread.queue(move |mut qobj: Pin<&mut qobject::Backend>| {
                        qobj.as_mut().progress(&q(&pid), f);
                    });
                }
            };
            let result = run_task(&kind, &id, &a, &b, &c, &mut progress);
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
        "checkAll" => with_lib(|l| service::check_all(l, &http)),
        "check" => with_lib(|l| service::check_app(l, &http, id)),
        "update" => with_lib(|l| service::update_app(l, &http, id, progress)),
        // a = link, b = asset pattern, c = channel
        "resolve" => service::resolve_link(a, b, c),
        "install" => with_lib(|l| service::install_link(l, &http, a, b, c, progress)),
        "addFile" => with_lib(|l| service::add_file(l, a)),
        // a = "force" to skip the cache
        "selfCheck" => service::self_check(
            &http,
            appofholding_core::selfupdate::detect_current(),
            a == "force",
        ),
        "selfUpdate" => service::self_update(
            &http,
            appofholding_core::selfupdate::detect_current(),
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
