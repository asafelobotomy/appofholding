//! systemd user timer that runs `appofholding update --scheduled`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::desktop::quote_exec_arg;
use crate::error::{Error, Result};
use crate::library::write_atomic;
use crate::paths::Dirs;

pub const UNIT_NAME: &str = "appofholding-update";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interval {
    Hourly,
    Daily,
    Weekly,
}

impl Interval {
    pub fn parse(s: &str) -> Option<Interval> {
        Some(match s {
            "hourly" => Interval::Hourly,
            "daily" => Interval::Daily,
            "weekly" => Interval::Weekly,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Interval::Hourly => "hourly",
            Interval::Daily => "daily",
            Interval::Weekly => "weekly",
        }
    }
    fn calendar(self) -> &'static str {
        self.as_str()
    }
}

pub fn service_text(exec_bin: &Path) -> String {
    format!(
        "[Unit]\nDescription=appofholding: check AppImages for updates\nAfter=network-online.target\n\n\
         [Service]\nType=oneshot\nExecStart={} update --scheduled\nNice=10\n",
        quote_exec_arg(&exec_bin.to_string_lossy())
    )
}

pub fn timer_text(interval: Interval) -> String {
    format!(
        "[Unit]\nDescription=appofholding: periodic AppImage update check\n\n\
         [Timer]\nOnCalendar={}\n# Run at the next opportunity if the machine was off.\nPersistent=true\nRandomizedDelaySec=15m\n\n\
         [Install]\nWantedBy=timers.target\n",
        interval.calendar()
    )
}

pub fn unit_paths(dirs: &Dirs) -> (PathBuf, PathBuf) {
    let d = dirs.systemd_user_dir();
    (
        d.join(format!("{UNIT_NAME}.service")),
        d.join(format!("{UNIT_NAME}.timer")),
    )
}

/// Write the unit files; enabling them is the caller's job (`systemctl --user enable --now`).
pub fn write_units(dirs: &Dirs, exec_bin: &Path, interval: Interval) -> Result<(PathBuf, PathBuf)> {
    if !exec_bin.is_absolute() {
        return Err(Error::Other(
            "the updater needs an absolute path to appofholding".into(),
        ));
    }
    let (svc, timer) = unit_paths(dirs);
    write_atomic(&svc, service_text(exec_bin).as_bytes())?;
    write_atomic(&timer, timer_text(interval).as_bytes())?;
    Ok((svc, timer))
}

/// The interval of the installed timer, read back from its `OnCalendar=` line; `None` when the
/// units are not both installed or the timer was edited to something else.
pub fn installed_interval(dirs: &Dirs) -> Option<Interval> {
    let (svc, timer) = unit_paths(dirs);
    if !svc.exists() {
        return None;
    }
    interval_of_timer(&fs::read_to_string(timer).ok()?)
}

fn interval_of_timer(text: &str) -> Option<Interval> {
    text.lines()
        .find_map(|l| l.trim().strip_prefix("OnCalendar="))
        .and_then(|v| Interval::parse(v.trim()))
}

pub fn remove_units(dirs: &Dirs) -> Result<bool> {
    let (svc, timer) = unit_paths(dirs);
    let mut removed = false;
    for p in [svc, timer] {
        match fs::remove_file(&p) {
            Ok(()) => removed = true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::io(&p)(e)),
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_text() {
        let s = service_text(Path::new("/home/u/My Apps/appofholding"));
        assert!(
            s.contains("ExecStart=\"/home/u/My Apps/appofholding\" update --scheduled"),
            "{s}"
        );
        let t = timer_text(Interval::Weekly);
        assert!(
            t.contains("OnCalendar=weekly")
                && t.contains("Persistent=true")
                && t.contains("timers.target")
        );
        assert_eq!(Interval::parse("daily"), Some(Interval::Daily));
        assert_eq!(Interval::parse("yearly"), None);
    }

    #[test]
    fn write_and_remove() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::with_root(tmp.path());
        let (svc, timer) =
            write_units(&dirs, Path::new("/usr/bin/appofholding"), Interval::Daily).unwrap();
        assert!(svc.exists() && timer.exists());
        assert!(svc.starts_with(tmp.path().join("config/systemd/user")));
        assert_eq!(installed_interval(&dirs), Some(Interval::Daily));
        write_units(&dirs, Path::new("/usr/bin/appofholding"), Interval::Weekly).unwrap();
        assert_eq!(installed_interval(&dirs), Some(Interval::Weekly));
        assert!(remove_units(&dirs).unwrap());
        assert_eq!(installed_interval(&dirs), None);
        assert!(!remove_units(&dirs).unwrap());
        assert!(write_units(&dirs, Path::new("relative/appofholding"), Interval::Daily).is_err());
    }

    #[test]
    fn interval_is_read_back_from_the_timer() {
        for iv in [Interval::Hourly, Interval::Daily, Interval::Weekly] {
            assert_eq!(interval_of_timer(&timer_text(iv)), Some(iv));
            assert_eq!(Interval::parse(iv.as_str()), Some(iv));
        }
        assert_eq!(interval_of_timer("[Timer]\nOnCalendar=*-*-* 04:00\n"), None);
        assert_eq!(interval_of_timer("[Timer]\n"), None);
    }
}
