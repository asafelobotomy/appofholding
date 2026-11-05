//! `.desktop` file parsing and rewriting.
//!
//! The parser keeps every group, key, comment and blank line in order so that rewriting an
//! entry only changes what we explicitly touch (Gear Lever #306/#369: desktop actions and
//! other keys must survive integration).

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
enum Line {
    Raw(String),
    Kv { key: String, value: String },
}

#[derive(Debug, Clone)]
struct Group {
    name: String,
    lines: Vec<Line>,
}

#[derive(Debug, Clone, Default)]
pub struct DesktopEntry {
    /// Lines before the first group header.
    preamble: Vec<String>,
    groups: Vec<Group>,
}

pub const MAIN: &str = "Desktop Entry";

impl DesktopEntry {
    pub fn parse(text: &str) -> Result<Self> {
        let mut e = DesktopEntry::default();
        for raw in text.lines() {
            let t = raw.trim();
            if let Some(name) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                e.groups.push(Group {
                    name: name.to_string(),
                    lines: vec![],
                });
                continue;
            }
            let Some(g) = e.groups.last_mut() else {
                e.preamble.push(raw.to_string());
                continue;
            };
            if t.is_empty() || t.starts_with('#') {
                g.lines.push(Line::Raw(raw.to_string()));
            } else if let Some((k, v)) = raw.split_once('=') {
                g.lines.push(Line::Kv {
                    key: k.trim().to_string(),
                    value: v.trim_start().to_string(),
                });
            } else {
                g.lines.push(Line::Raw(raw.to_string()));
            }
        }
        if e.group(MAIN).is_none() {
            return Err(Error::DesktopEntry("missing [Desktop Entry] group".into()));
        }
        Ok(e)
    }

    fn group(&self, name: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.name == name)
    }

    pub fn get(&self, group: &str, key: &str) -> Option<&str> {
        self.group(group)?.lines.iter().find_map(|l| match l {
            Line::Kv { key: k, value } if k == key => Some(value.as_str()),
            _ => None,
        })
    }

    pub fn main(&self, key: &str) -> Option<&str> {
        self.get(MAIN, key)
    }

    /// Replace the key if present, else append it to the group.
    pub fn set(&mut self, group: &str, key: &str, value: &str) {
        let Some(g) = self.groups.iter_mut().find(|g| g.name == group) else {
            return;
        };
        for l in &mut g.lines {
            if let Line::Kv { key: k, value: v } = l
                && k == key
            {
                *v = value.to_string();
                return;
            }
        }
        // Insert before trailing blank/comment lines so groups stay tidy.
        let at = g
            .lines
            .iter()
            .rposition(|l| matches!(l, Line::Kv { .. }))
            .map_or(0, |i| i + 1);
        g.lines.insert(
            at,
            Line::Kv {
                key: key.to_string(),
                value: value.to_string(),
            },
        );
    }

    pub fn remove(&mut self, group: &str, key: &str) {
        if let Some(g) = self.groups.iter_mut().find(|g| g.name == group) {
            g.lines
                .retain(|l| !matches!(l, Line::Kv { key: k, .. } if k == key));
        }
    }

    /// Append a new, empty group (no-op if it exists).
    pub fn add_group(&mut self, name: &str) {
        if self.group(name).is_none() {
            self.groups.push(Group {
                name: name.to_string(),
                lines: vec![],
            });
        }
    }

    /// Names of `[Desktop Action X]` groups.
    pub fn action_groups(&self) -> Vec<String> {
        self.groups
            .iter()
            .filter(|g| g.name.starts_with("Desktop Action "))
            .map(|g| g.name.clone())
            .collect()
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        for l in &self.preamble {
            out.push_str(l);
            out.push('\n');
        }
        for g in &self.groups {
            out.push_str(&format!("[{}]\n", g.name));
            for l in &g.lines {
                match l {
                    Line::Raw(r) => out.push_str(r),
                    Line::Kv { key, value } => out.push_str(&format!("{key}={value}")),
                }
                out.push('\n');
            }
        }
        out
    }

    pub fn is_visible_application(&self) -> bool {
        self.main("Type").is_none_or(|t| t == "Application")
            && !matches!(self.main("NoDisplay"), Some("true"))
    }
}

/// Split an `Exec` value into arguments, honouring the desktop-entry quoting rules.
/// Field codes (`%U`, `%f`, ...) come back as ordinary tokens.
pub fn split_exec(value: &str) -> Vec<String> {
    // File-level string escape: `\\` -> `\` (other escapes are not meaningful in Exec).
    let value = value.replace("\\\\", "\\");
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut in_arg = false;
    let mut quoted = false;
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                in_arg = true;
            }
            '\\' if quoted => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
                in_arg = true;
            }
            c if c.is_whitespace() && !quoted => {
                if in_arg {
                    args.push(std::mem::take(&mut cur));
                    in_arg = false;
                }
            }
            c => {
                cur.push(c);
                in_arg = true;
            }
        }
    }
    if in_arg {
        args.push(cur);
    }
    args
}

/// Quote one argument for an `Exec` value (spec-compliant, including the file-level
/// backslash doubling). Field codes such as `%U` are passed through unquoted.
pub fn quote_exec_arg(arg: &str) -> String {
    let is_field_code = arg.len() == 2 && arg.starts_with('%') && arg != "%%";
    let special = |c: char| c.is_whitespace() || "\"'\\><~|&;$*?#()`%".contains(c);
    if arg.is_empty() {
        return "\"\"".into();
    }
    if is_field_code || !arg.chars().any(special) {
        return arg.to_string();
    }
    let mut s = String::from("\"");
    for c in arg.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                s.push('\\');
                s.push(c);
            }
            // A literal `%` is written `%%`.
            '%' => s.push_str("%%"),
            _ => s.push(c),
        }
    }
    s.push('"');
    s.replace('\\', "\\\\")
}

pub fn join_exec(args: &[String]) -> String {
    args.iter()
        .map(|a| quote_exec_arg(a))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# comment\n[Desktop Entry]\nType=Application\nName=Foo\nName[de]=Fuh\nExec=AppRun --flag %U\nIcon=foo\n\n[Desktop Action new]\nName=New\nExec=AppRun --new-window\n";

    #[test]
    fn roundtrip_preserves_everything() {
        let e = DesktopEntry::parse(SAMPLE).unwrap();
        assert_eq!(e.render(), SAMPLE);
        assert_eq!(e.main("Name[de]"), Some("Fuh"));
        assert_eq!(e.action_groups(), vec!["Desktop Action new".to_string()]);
    }

    #[test]
    fn set_and_remove() {
        let mut e = DesktopEntry::parse(SAMPLE).unwrap();
        e.set(MAIN, "Icon", "bar");
        e.set(MAIN, "StartupNotify", "true");
        e.remove(MAIN, "Type");
        let r = e.render();
        assert!(r.contains("Icon=bar\n"));
        assert!(r.contains("StartupNotify=true\n"));
        assert!(!r.contains("Type=Application"));
        // New keys stay inside their group, before the blank separator.
        assert!(r.find("StartupNotify").unwrap() < r.find("[Desktop Action new]").unwrap());
    }

    #[test]
    fn add_group_and_keys() {
        let mut e = DesktopEntry::parse(SAMPLE).unwrap();
        e.add_group("Desktop Action extra");
        e.add_group("Desktop Action extra"); // idempotent
        e.set("Desktop Action extra", "Name", "Extra");
        e.set("Desktop Action extra", "Exec", "x");
        let r = e.render();
        assert_eq!(r.matches("[Desktop Action extra]").count(), 1);
        assert!(
            r.ends_with("[Desktop Action extra]\nName=Extra\nExec=x\n"),
            "{r}"
        );
    }

    #[test]
    fn missing_main_group_is_error() {
        assert!(DesktopEntry::parse("[Other]\nA=b\n").is_err());
    }

    #[test]
    fn exec_roundtrip_with_spaces_and_specials() {
        let args = vec![
            "/home/u/My Apps/foo bar".to_string(),
            "run".into(),
            "--opt=$HOME \"x\"".into(),
            "%U".into(),
        ];
        let joined = join_exec(&args);
        assert_eq!(split_exec(&joined), args);
        assert_eq!(quote_exec_arg("/usr/bin/foo"), "/usr/bin/foo");
    }

    #[test]
    fn split_plain() {
        assert_eq!(
            split_exec("AppRun --flag %U"),
            vec!["AppRun", "--flag", "%U"]
        );
        assert_eq!(split_exec("\"/a b/c\" x"), vec!["/a b/c", "x"]);
    }
}
