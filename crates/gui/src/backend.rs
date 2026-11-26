//! The QObject the QML interface talks to. It owns no state of its own: every call runs one
//! function from `service` on the library (kept open per thread) and hands JSON back. Slow calls (network,
//! unpacking) run on a worker thread and report through the `finished` / `progress` signals.

use core::pin::Pin;
use cxx_qt::Threading;
use cxx_qt_lib::QString;
use serde_json::{Value, json};

use crate::service::{self, Res};

mod prefs;
mod startup;
mod tasks;

pub use startup::*;

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
        /// The app page asked for on the command line (`--app`/`--desktop-id`, `--tab`), as
        /// JSON {app, tab}, or "null".
        #[cxx_name = "startupOpen"]
        #[qinvokable]
        fn startup_open(self: &Backend) -> QString;
        /// Files dropped on the window or opened with appOFholding (a JSON array of paths or
        /// file:// URLs): JSON [{path, name, inLibrary}] for the ones that are files.
        #[cxx_name = "describeDrop"]
        #[qinvokable]
        fn describe_drop(self: &Backend, files: &QString) -> QString;
        /// Started with `--tray` (from the login autostart): stay in the tray.
        #[cxx_name = "startedInTray"]
        #[qinvokable]
        fn started_in_tray(self: &Backend) -> bool;
        /// Tray and startup settings: {tray, minimiseToTray, closeToTray, notifyUpdates,
        /// startInTray, dropAction, autostart}.
        #[cxx_name = "desktopSettings"]
        #[qinvokable]
        fn desktop_settings(self: &Backend) -> QString;

        /// Settings for every app: {policy, keepVersions, maxKeepVersions, sandbox, scan}.
        #[cxx_name = "libraryDefaults"]
        #[qinvokable]
        fn library_defaults(self: &Backend) -> QString;

        // ---- changes: "" on success, otherwise the error message
        /// Change some of {policy, keepVersions, sandbox, scan}. Lowering keepVersions deletes the
        /// extra kept versions.
        #[cxx_name = "setLibraryDefaults"]
        #[qinvokable]
        fn set_library_defaults(self: &Backend, json: &QString) -> QString;
        /// Give every app with an update source this policy: {changed} or {error}.
        #[cxx_name = "applyPolicyToAll"]
        #[qinvokable]
        fn apply_policy_to_all(self: &Backend, policy: &QString) -> QString;
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
        /// The saved colour theme: uq, system, gruvbox-dark or gruvbox-light.
        #[qinvokable]
        fn theme(self: &Backend) -> QString;
        /// uQ's current palette as JSON (applied to the application too when it changed since
        /// the last call), or an empty string when uQ's theme is not there.
        #[cxx_name = "uqPalette"]
        #[qinvokable]
        fn uq_palette(self: &Backend) -> QString;
        /// Start a fresh copy of this program a moment from now (the caller then quits).
        /// Follow uQ's tokens file (a file watch on its own thread, started once); each change
        /// arrives as `uqThemeChanged`.
        #[cxx_name = "followUqTheme"]
        #[qinvokable]
        fn follow_uq_theme(self: Pin<&mut Backend>);
        #[cxx_name = "restartSoon"]
        #[qinvokable]
        fn restart_soon(self: &Backend);
        #[cxx_name = "setTheme"]
        #[qinvokable]
        fn set_theme(self: &Backend, name: &QString) -> QString;
        /// The saved library layout: list, grid or compact.
        #[qinvokable]
        fn view(self: &Backend) -> QString;
        #[cxx_name = "setView"]
        #[qinvokable]
        fn set_view(self: &Backend, name: &QString) -> QString;
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
        // kind: notifyCheck | check | update | resolve | install | addFile | selfCheck | selfUpdate | importScan | importRun | updateFromFile | applySuggestion | suggestions | scheduleInstall | scheduleRemove
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
        /// uQ's theme changed: call `uqPalette` for the new one.
        #[qsignal]
        #[cxx_name = "uqThemeChanged"]
        fn uq_theme_changed(self: Pin<&mut Backend>);
        #[qsignal]
        fn progress(self: Pin<&mut Backend>, id: &QString, fraction: f64);
        /// Another start of the program handed over: files is a JSON array of AppImages to add,
        /// show says whether to bring the window up, target is the app page to show (JSON
        /// {app, tab}, or "null").
        #[qsignal]
        fn activated(self: Pin<&mut Backend>, files: &QString, show: bool, target: &QString);
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

thread_local! {
    /// The library, opened once per thread. The interface thread makes many small calls (every
    /// refresh reads the list and the open page), and opening means finding the folders and the
    /// `appofholding` binary, opening the database and checking its schema. Worker threads run
    /// one task each, so for them this is the same as opening it per task.
    static LIB: std::cell::RefCell<Option<appofholding_core::Library>> = const { std::cell::RefCell::new(None) };
}

fn with_lib<T>(f: impl FnOnce(&appofholding_core::Library) -> Res<T>) -> Res<T> {
    LIB.with(|cell| {
        // Opened outside the borrow; a failure is not cached, so the next call tries again.
        if cell.borrow().is_none() {
            let lib = service::open_lib()?;
            *cell.borrow_mut() = Some(lib);
        }
        let guard = cell.borrow();
        let lib = guard.as_ref().expect("opened above");
        let out = f(lib);
        // Warnings belong to the call that made them (and are reported by it, if at all); a
        // library kept open must not collect them forever.
        let _ = lib.take_warnings();
        out
    })
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

    pub fn startup_files(&self) -> QString {
        q(&json!(startup_file_list()).to_string())
    }
    pub fn startup_open(&self) -> QString {
        q(&open_json(startup_open().as_ref()))
    }
    pub fn describe_drop(&self, files: &QString) -> QString {
        json_or_err((|| {
            let paths: Vec<String> = serde_json::from_str(&s(files)).map_err(|e| e.to_string())?;
            with_lib(|l| Ok(service::describe_drop(l, &paths)))
        })())
    }
    pub fn started_in_tray(&self) -> bool {
        started_in_tray()
    }
    pub fn listen(self: Pin<&mut Self>) {
        self.answer_later_starts();
    }
    fn answer_later_starts(&self) {
        let Some(path) = crate::instance::socket_path() else {
            return;
        };
        let thread = self.qt_thread();
        crate::instance::listen(&path, move |req| {
            let files = json!(req.files).to_string();
            let target = open_json(req.open.as_ref());
            let _ = thread.queue(move |mut qobj: Pin<&mut qobject::Backend>| {
                qobj.as_mut().activated(&q(&files), req.show, &q(&target));
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
    pub fn library_defaults(&self) -> QString {
        json_or_err(with_lib(service::library_defaults))
    }
    pub fn set_library_defaults(&self, json: &QString) -> QString {
        unit((|| {
            let v: Value = serde_json::from_str(&s(json)).map_err(|e| e.to_string())?;
            with_lib(|l| service::set_library_defaults(l, &v))
        })())
    }
    pub fn apply_policy_to_all(&self, policy: &QString) -> QString {
        json_or_err(with_lib(|l| service::apply_policy_all(l, &s(policy))))
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
    pub fn dismiss_suggestion(&self, id: &QString, key: &QString) -> QString {
        unit(with_lib(|l| {
            service::dismiss_suggestion(l, &s(id), &s(key))
        }))
    }
    pub fn relaunch(&self) -> QString {
        // Let the new copy take over the single-window socket instead of handing its start
        // back to this one, which is about to quit. That has to happen before the spawn.
        let socket = crate::instance::socket_path();
        if let Some(path) = &socket {
            crate::instance::release(path);
        }
        // The AppImage file or the package's binary, which now hold the new version.
        let r = service::gui_exe().and_then(|exe| {
            std::process::Command::new(exe)
                .spawn()
                .map(|_| ())
                .map_err(|e| format!("could not start the new version: {e}"))
        });
        if r.is_err() && socket.is_some() {
            // No new copy: this one keeps running, so it must stay reachable.
            self.answer_later_starts();
        }
        unit(r)
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
}
