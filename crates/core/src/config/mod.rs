//! Declarative configuration: export everything you set up to a TOML file, and `apply` such a
//! file to a library (on this or another machine). Applying is idempotent: running it twice
//! changes nothing the second time.
//!
//! The file never contains absolute install paths, so it is portable.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::http::Http;
use crate::launch::{DEFAULT_PROFILE, Profile};
use crate::library::{AddOptions, Install, Library};
use crate::sandbox::{Bus, HomeAccess, Level, PathGrant, Settings};
use crate::source::{self, Channel, Origin, Policy, Provider};
mod apply;

pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    #[serde(default, rename = "app")]
    pub apps: Vec<AppConfig>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    /// The app's id in the library (derived from its name when adding).
    pub id: String,
    /// Informational only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A local AppImage to add when the app is missing (not written by `export`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
    /// Where updates come from: anything `appofholding source set` accepts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// `latest` (default), `latest-pre`, `latest-all` or a tag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_pattern: Option<String>,
    /// auto | notify | manual | pinned
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<SandboxConfig>,
    #[serde(default, rename = "profile", skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<ProfileConfig>,
    /// post-install / pre-update / post-update -> shell command
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub hooks: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxConfig {
    /// none | standard | strict (individual fields below override the preset)
    pub level: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub devices: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<bool>,
    /// private | ro | rw
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
    /// off | filtered | full
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bus: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chromium_workaround: Option<bool>,
    /// Folders to share; a `:rw` suffix makes one writable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileConfig {
    /// `default` or the name of an extra profile (shown in the app's right-click menu).
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prefix: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub extract_and_run: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub elevated: bool,
}

impl Config {
    pub fn parse(text: &str) -> Result<Config> {
        let cfg: Config = toml::from_str(text)
            .map_err(|e| Error::Other(format!("invalid configuration: {e}")))?;
        if cfg.version != FORMAT_VERSION {
            return Err(Error::Other(format!(
                "configuration format version {} is not supported (this build reads version {FORMAT_VERSION})",
                cfg.version
            )));
        }
        let mut seen = std::collections::BTreeSet::new();
        for a in &cfg.apps {
            if a.id.trim().is_empty() {
                return Err(Error::Other("an [[app]] entry has no id".into()));
            }
            if !seen.insert(&a.id) {
                return Err(Error::Other(format!("app '{}' is listed twice", a.id)));
            }
        }
        Ok(cfg)
    }

    pub fn to_toml(&self) -> Result<String> {
        let body = toml::to_string_pretty(self).map_err(|e| Error::Other(e.to_string()))?;
        Ok(format!(
            "# appOFholding configuration. Apply it with: appofholding apply <this file>\n\
             # Missing apps with a `source` are downloaded; everything else is set up as written.\n\n{body}"
        ))
    }
}

/// The link form of a source, which `source::parse_input` turns back into the same provider.
pub fn source_to_input(p: &Provider) -> String {
    match p {
        Provider::GitHub { owner, repo } => format!("https://github.com/{owner}/{repo}"),
        // Self-hosted GitLab (invent.kde.org, ...) is only recognised with the prefix.
        Provider::GitLab { host, project }
            if host == "gitlab.com" || host.starts_with("gitlab.") =>
        {
            format!("https://{host}/{project}")
        }
        Provider::GitLab { host, project } => format!("gitlab:https://{host}/{project}"),
        Provider::Forgejo { host, owner, repo } => format!("forgejo:https://{host}/{owner}/{repo}"),
        Provider::Url { url } => url.clone(),
        Provider::Zsync { url } => format!("zsync|{url}"),
    }
}

fn sandbox_to_config(s: &Settings) -> Option<SandboxConfig> {
    if !s.enabled() {
        return None;
    }
    // Record the closest preset and only the fields that differ from it, so presets stay readable.
    let differences = |b: &Settings| {
        [
            s.network != b.network,
            s.gpu != b.gpu,
            s.audio != b.audio,
            s.devices != b.devices,
            s.display != b.display,
            s.home != b.home,
            s.bus != b.bus,
            s.chromium_workaround != b.chromium_workaround,
        ]
        .iter()
        .filter(|d| **d)
        .count()
    };
    let (standard, strict) = (
        Settings::for_level(Level::Standard),
        Settings::for_level(Level::Strict),
    );
    let (base_level, base) =
        if s.level == Level::Strict || differences(&strict) < differences(&standard) {
            (Level::Strict, strict)
        } else {
            (Level::Standard, standard)
        };
    let diff = |a: bool, b: bool| (a != b).then_some(a);
    Some(SandboxConfig {
        level: base_level.as_str().into(),
        network: diff(s.network, base.network),
        gpu: diff(s.gpu, base.gpu),
        audio: diff(s.audio, base.audio),
        devices: diff(s.devices, base.devices),
        display: diff(s.display, base.display),
        home: (s.home != base.home).then(|| s.home.as_str().into()),
        bus: (s.bus != base.bus).then(|| s.bus.as_str().into()),
        chromium_workaround: diff(s.chromium_workaround, base.chromium_workaround),
        allow: s
            .paths
            .iter()
            .map(|g| format!("{}{}", g.path.display(), if g.write { ":rw" } else { "" }))
            .collect(),
    })
}

fn sandbox_from_config(c: &SandboxConfig) -> Result<Settings> {
    let level = Level::parse(&c.level)
        .filter(|l| *l != Level::Custom)
        .ok_or_else(|| {
            Error::Other(format!(
                "sandbox level '{}' must be none, standard or strict",
                c.level
            ))
        })?;
    let mut s = Settings::for_level(level);
    if level == Level::None {
        return Ok(s);
    }
    if let Some(v) = c.network {
        s.network = v
    }
    if let Some(v) = c.gpu {
        s.gpu = v
    }
    if let Some(v) = c.audio {
        s.audio = v
    }
    if let Some(v) = c.devices {
        s.devices = v
    }
    if let Some(v) = c.display {
        s.display = v
    }
    if let Some(v) = c.chromium_workaround {
        s.chromium_workaround = v
    }
    if let Some(h) = &c.home {
        s.home = HomeAccess::parse(h)
            .ok_or_else(|| Error::Other(format!("sandbox home '{h}' must be private, ro or rw")))?;
    }
    if let Some(b) = &c.bus {
        s.bus = Bus::parse(b).ok_or_else(|| {
            Error::Other(format!("sandbox bus '{b}' must be off, filtered or full"))
        })?;
    }
    for a in &c.allow {
        let (p, write) = match a.rsplit_once(':') {
            Some((p, "rw")) => (p, true),
            Some((p, "ro")) => (p, false),
            _ => (a.as_str(), false),
        };
        s.paths.push(PathGrant {
            path: p.into(),
            write,
        });
    }
    Ok(s.normalized())
}

fn profile_to_config(name: &str, p: &Profile) -> ProfileConfig {
    ProfileConfig {
        name: name.into(),
        args: p.args.clone(),
        env: p.env.clone(),
        prefix: p.prefix.clone(),
        presets: p.presets.clone(),
        cwd: p.cwd.clone(),
        extract_and_run: p.extract_and_run,
        elevated: p.elevated,
    }
}

fn profile_from_config(c: &ProfileConfig) -> Profile {
    Profile {
        args: c.args.clone(),
        env: c.env.clone(),
        prefix: c.prefix.clone(),
        cwd: c.cwd.clone(),
        extract_and_run: c.extract_and_run,
        elevated: c.elevated,
        presets: c.presets.clone(),
    }
}

/// What `apply` did (or would do) for one app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    Installed(String),
    Added(String),
    /// Not in the library and nothing says how to get it.
    Missing {
        id: String,
        reason: String,
    },
    Configured(String),
    Failed {
        id: String,
        error: String,
    },
}

#[derive(Debug, Clone, Default)]
pub struct ApplyOptions {
    /// Do not download missing apps.
    pub no_install: bool,
    /// Also delete extra profiles and hooks that the file does not list.
    pub prune: bool,
    /// Accept entries that make appOFholding run commands: hooks, command prefixes and
    /// profiles that start the app as root. Off by default, so a file from someone else cannot
    /// slip them in.
    pub allow_commands: bool,
}

/// What in this entry would run commands on this machine, in words (empty when nothing does).
pub fn commands_in(a: &AppConfig) -> Vec<String> {
    let mut out: Vec<String> = a
        .hooks
        .iter()
        .map(|(kind, cmd)| format!("{kind} hook: {cmd}"))
        .collect();
    for p in &a.profiles {
        if !p.prefix.is_empty() {
            out.push(format!(
                "profile '{}' runs under: {}",
                p.name,
                p.prefix.join(" ")
            ));
        }
        if p.elevated {
            out.push(format!("profile '{}' starts the app as root", p.name));
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests;
