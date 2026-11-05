//! Desktop notifications for the background updater.

use std::process::Command;

pub trait Notifier {
    fn notify(&self, title: &str, body: &str);
}

/// Uses `notify-send` when present; silently does nothing otherwise.
pub struct NotifySend;

impl Notifier for NotifySend {
    fn notify(&self, title: &str, body: &str) {
        let _ = Command::new("notify-send")
            .args([
                "--app-name=appimgr",
                "--icon=system-software-update",
                "--",
                title,
                body,
            ])
            .status();
    }
}

pub struct Silent;

impl Notifier for Silent {
    fn notify(&self, _: &str, _: &str) {}
}

/// Records notifications; for tests.
#[cfg(any(test, feature = "fixtures"))]
#[derive(Default)]
pub struct Recorder(pub std::cell::RefCell<Vec<(String, String)>>);

#[cfg(any(test, feature = "fixtures"))]
impl Notifier for Recorder {
    fn notify(&self, title: &str, body: &str) {
        self.0
            .borrow_mut()
            .push((title.to_string(), body.to_string()));
    }
}
