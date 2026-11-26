//! The window's settings: theme, view, desktop integration, the update schedule.

use super::*;

impl qobject::Backend {
    pub fn schedule_status(&self) -> QString {
        json_or_err(with_dirs(|d| Ok(service::schedule_status(d))))
    }

    pub fn data_dir(&self) -> QString {
        let d = appofholding_core::Dirs::detect()
            .map(|d| d.library_dir.display().to_string())
            .unwrap_or_default();
        q(&d)
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

    pub fn follow_uq_theme(self: Pin<&mut Self>) {
        static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if STARTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let thread = self.qt_thread();
        crate::uq_theme::watch(crate::uqtheme::tokens_path(), move |_| {
            thread
                .queue(|mut qobj: Pin<&mut qobject::Backend>| qobj.as_mut().uq_theme_changed())
                .is_ok()
        });
    }

    pub fn theme(&self) -> QString {
        q(&current_theme())
    }

    pub fn uq_palette(&self) -> QString {
        q(&apply_uq_palette().unwrap_or_default())
    }

    pub fn restart_soon(&self) {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        // The running copy answers any new start by showing its window, so wait for it to be gone.
        let _ = std::process::Command::new("sh")
            .args(["-c", "sleep 1; exec \"$0\" \"$@\""])
            .arg(exe)
            .args(
                std::env::args_os()
                    .skip(1)
                    .filter(|a| a != service::TRAY_FLAG),
            )
            .spawn();
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
}
