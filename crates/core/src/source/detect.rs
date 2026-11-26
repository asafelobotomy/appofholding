//! Guessing where an AppImage without embedded update information is published, from what it
//! carries inside: electron-builder's `app-update.yml`, the project links in its AppStream
//! metadata, a link to its own repository inside its main program (Rust programs often carry
//! one), and reverse-DNS app ids such as `io.github.OWNER.REPO`.
//!
//! These are only guesses. Nothing here goes online; a caller must confirm that a candidate
//! really publishes this app as an AppImage before using it (see `Library::apply_suggestion`).

use crate::appstream;
use crate::desktop::{DesktopEntry, split_exec};
use crate::image::AppImage;
use crate::source::{Provider, Source, parse_input};
mod links;
use links::*;

/// One place the app may be published, and what pointed there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub source: Source,
    /// Where the hint came from, in words ("its electron-builder update settings").
    pub evidence: String,
    /// The app's own update settings name it, so its files need not resemble the app's name.
    pub trusted: bool,
}

/// What is known about the app besides the files in its image.
pub struct App<'a> {
    /// The chosen desktop entry's path inside the image.
    pub desktop_path: &'a str,
    pub entry: &'a DesktopEntry,
    /// Display name.
    pub name: &'a str,
}

impl App<'_> {
    /// The names the app goes by, lowercased with only letters and digits left: its display
    /// name, its program, its desktop file and icon names (the last part of a reverse-DNS id).
    /// Very short ones, and words any AppImage's file name may carry ("app", "linux",
    /// "x86_64"), are left out, as they would match almost anything.
    pub fn names(&self) -> Vec<String> {
        let last = |s: &str| {
            s.trim_end_matches(".desktop")
                .rsplit('.')
                .next()
                .unwrap_or("")
                .to_string()
        };
        let mut v = vec![
            squash(self.name),
            squash(&program(self.entry).unwrap_or_default()),
            squash(&last(self.desktop_path.rsplit('/').next().unwrap_or(""))),
            squash(&last(self.entry.main("Icon").unwrap_or(""))),
        ];
        v.retain(|n| n.len() >= 3 && !is_generic(n));
        v.sort();
        v.dedup();
        v
    }
}

/// Words that say nothing about which app a file is: they appear in the names of AppImages of
/// every kind. Compared squashed (lowercase letters and digits only).
const GENERIC: &[&str] = &[
    "app",
    "apps",
    "application",
    "appimage",
    "appimagekit",
    "image",
    "img",
    "linux",
    "gnu",
    "x86",
    "x8664",
    "x64",
    "amd64",
    "i386",
    "i686",
    "arm",
    "arm64",
    "aarch64",
    "armhf",
    "armv7",
    "bin",
    "run",
    "apprun",
    "exe",
    "desktop",
    "launcher",
    "setup",
    "install",
    "installer",
    "portable",
    "release",
    "latest",
    "stable",
    "beta",
    "alpha",
    "nightly",
    "dev",
    "main",
    "electron",
    "client",
    "gui",
    "tool",
    "tools",
    "icon",
    "default",
    "program",
    "x11",
    "wayland",
];

fn is_generic(squashed: &str) -> bool {
    GENERIC.contains(&squashed) || squashed.chars().all(|c| c.is_ascii_digit())
}

/// A file name's words, lowercased: split at anything but letters and digits, between letters
/// and digits, and before an upper-case letter that follows a lower-case one
/// (`AV-MorningStar-0.4.2-x86_64` -> av, morning, star, 0, 4, 2, x, 86, 64).
fn words(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut prev: Option<char> = None;
    for c in s.chars() {
        if !c.is_ascii_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            prev = None;
            continue;
        }
        let split = prev.is_some_and(|p| {
            p.is_ascii_digit() != c.is_ascii_digit()
                || (p.is_ascii_lowercase() && c.is_ascii_uppercase())
        });
        if split && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(c.to_ascii_lowercase());
        prev = Some(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Whether a file name (a release asset) carries one of the app's `names` as whole words: one
/// word or several in a row (`AV-Morning-Star` carries `avmorningstar`), never part of a word
/// (`keynote` does not carry `note`). Generic names ("app", "linux") never count.
pub fn resembles(file_name: &str, names: &[String]) -> bool {
    let w = words(file_name);
    names.iter().filter(|n| !is_generic(n)).any(|n| {
        (0..w.len()).any(|i| {
            let mut joined = String::new();
            for word in &w[i..] {
                joined.push_str(word);
                if joined.len() >= n.len() {
                    return joined == *n;
                }
            }
            false
        })
    })
}

/// Every candidate, most trustworthy first, without duplicates.
pub fn detect(img: &AppImage, app: &App<'_>) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let push = |out: &mut Vec<Candidate>, source: Source, evidence: &str, trusted: bool| {
        let key = source.describe().to_ascii_lowercase();
        if !out
            .iter()
            .any(|c| c.source.describe().to_ascii_lowercase() == key)
        {
            out.push(Candidate {
                source,
                evidence: evidence.to_string(),
                trusted,
            });
        }
    };

    for text in electron_update_files(img) {
        if let Some(s) = from_app_update_yml(&text) {
            push(&mut out, s, "its electron-builder update settings", true);
        }
    }

    let metainfo = appstream::files(img);
    for text in &metainfo {
        for url in appstream::project_urls(text) {
            if let Some(s) = from_project_url(&url) {
                push(
                    &mut out,
                    s,
                    "the project link in its AppStream metadata",
                    false,
                );
            }
        }
    }

    // Reading the program means unpacking it (tens of MB), so only when nothing better was found.
    let names = app.names();
    if out.is_empty()
        && let Some(bytes) = program_bytes(img, app.entry)
    {
        for s in links_in_program(&bytes, &names) {
            push(
                &mut out,
                s,
                "the link to its project inside its program",
                false,
            );
        }
    }

    for id in app_ids(img, app, &metainfo) {
        for s in from_reverse_dns(&id, app.name) {
            push(&mut out, s, &format!("its app id {id}"), false);
        }
    }
    out
}

/// Ids the app goes by, as written: its desktop files' names (an image often carries the same
/// entry under a short name too), the entry's `Icon` and `StartupWMClass`, and AppStream ids.
fn app_ids(img: &AppImage, app: &App<'_>, metainfo: &[String]) -> Vec<String> {
    let stem = |p: &str| {
        p.rsplit('/')
            .next()
            .unwrap_or("")
            .trim_end_matches(".desktop")
            .to_string()
    };
    let mut ids = vec![stem(app.desktop_path)];
    for dir in ["/", "/usr/share/applications"] {
        ids.extend(
            img.list_dir(dir)
                .into_iter()
                .filter(|(n, _)| n.ends_with(".desktop"))
                .map(|(n, _)| stem(&n)),
        );
    }
    for key in ["Icon", "StartupWMClass"] {
        ids.extend(app.entry.main(key).map(str::to_string));
    }
    ids.extend(
        metainfo
            .iter()
            .filter_map(|t| appstream::element(t, "id"))
            .map(|id| id.trim_end_matches(".desktop").to_string()),
    );
    let mut seen: Vec<String> = Vec::new();
    ids.retain(|id| {
        let fresh = !id.is_empty() && !seen.contains(id);
        seen.push(id.clone());
        fresh
    });
    ids
}

/// The program the desktop entry starts (`Exec`'s first word, without its folder).
fn program(entry: &DesktopEntry) -> Option<String> {
    let first = split_exec(entry.main("Exec")?).into_iter().next()?;
    let name = first.rsplit('/').next()?.to_string();
    (!name.is_empty() && name != "AppRun").then_some(name)
}

fn program_bytes(img: &AppImage, entry: &DesktopEntry) -> Option<Vec<u8>> {
    let name = program(entry)?;
    ["/usr/bin", "/bin", ""]
        .iter()
        .find_map(|dir| img.read_file(&format!("{dir}/{name}")).ok().flatten())
}

/// Links to a GitHub, GitLab or Codeberg repository inside a program whose repository name is
/// one of the app's `names`. Programs also carry links to their libraries' repositories (Tauri,
/// muda, ...), which the name check leaves out.
pub(crate) fn links_in_program(bytes: &[u8], names: &[String]) -> Vec<Source> {
    const HOSTS: [&str; 3] = ["github.com", "gitlab.com", "codeberg.org"];
    let ok = |c: u8| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.');
    let mut out: Vec<Source> = Vec::new();
    let mut i = 0;
    while let Some(at) = find(&bytes[i..], b"https://") {
        let start = i + at + 8;
        i = start;
        let Some(host) = HOSTS
            .iter()
            .find(|h| bytes[start..].starts_with(h.as_bytes()))
        else {
            continue;
        };
        let mut p = start + host.len();
        if bytes.get(p) != Some(&b'/') {
            continue;
        }
        p += 1;
        let owner_end = p + bytes[p..].iter().take_while(|c| ok(**c)).count();
        if owner_end == p || bytes.get(owner_end) != Some(&b'/') {
            continue;
        }
        let repo_start = owner_end + 1;
        let repo_end = repo_start + bytes[repo_start..].iter().take_while(|c| ok(**c)).count();
        let owner = String::from_utf8_lossy(&bytes[p..owner_end]).into_owned();
        let repo = String::from_utf8_lossy(&bytes[repo_start..repo_end]);
        let repo = repo.trim_end_matches('.');
        let repo = repo.strip_suffix(".git").unwrap_or(repo);
        if !names.contains(&squash(repo)) {
            continue;
        }
        if let Some(s) = from_project_url(&format!("https://{host}/{owner}/{repo}"))
            && !out
                .iter()
                .any(|o| o.describe().eq_ignore_ascii_case(&s.describe()))
        {
            out.push(s);
        }
    }
    out
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    let first = needle[0];
    let mut from = 0;
    while let Some(off) = hay[from..].iter().position(|&b| b == first) {
        let at = from + off;
        if hay[at..].starts_with(needle) {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

fn squash(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests;
