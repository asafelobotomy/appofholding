//! Exporting the library to a config file and applying one back.

use super::*;

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

    pub(super) fn apply_app(
        &self,
        http: &dyn Http,
        a: &AppConfig,
        opts: &ApplyOptions,
    ) -> Result<Vec<Note>> {
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
