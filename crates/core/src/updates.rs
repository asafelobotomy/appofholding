//! Library operations that involve update sources: set/clear, check, install-from-link.

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::assets::{self, Arch, Selection};
use crate::check::{self, Report};
use crate::db::{AppRecord, SourceRecord};
use crate::download;
use crate::error::{Error, Result};
use crate::http::Http;
use crate::library::{AddOptions, Install, Library};
use crate::source::{Origin, Parsed, Source};
use crate::version::Installed;

pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl Library {
    pub fn source(&self, id: &str) -> Result<Option<SourceRecord>> {
        self.get(id)?;
        self.db.get_source(id)
    }

    pub fn sources(&self) -> Result<Vec<SourceRecord>> {
        self.db.list_sources()
    }

    pub fn set_source(&self, id: &str, source: &Source) -> Result<()> {
        self.get(id)?;
        self.db.upsert_source(id, source)
    }

    pub fn clear_source(&self, id: &str) -> Result<bool> {
        self.get(id)?;
        self.db.delete_source(id)
    }

    /// Check one app's source (always asking the server) and record the outcome in the
    /// database: a status, or the error.
    pub fn check(&self, http: &dyn Http, id: &str) -> Result<Report> {
        self.check_impl(http, id, false)?
            .report
            .ok_or_else(|| Error::Other("internal: missing report".into()))
    }

    /// Check several apps at once (a few at a time; each worker opens its own database
    /// connection). Results come back in the order of `ids`.
    pub fn check_many(
        &self,
        http: &(dyn Http + Sync),
        ids: &[String],
    ) -> Vec<(String, Result<Report>)> {
        use std::sync::Mutex;
        use std::sync::atomic::{AtomicUsize, Ordering};
        const WORKERS: usize = 4;
        let (dirs, exec_bin) = (self.dirs.clone(), self.exec_bin.clone());
        let next = AtomicUsize::new(0);
        let out: Mutex<Vec<Option<Result<Report>>>> =
            Mutex::new((0..ids.len()).map(|_| None).collect());
        std::thread::scope(|scope| {
            for _ in 0..WORKERS.min(ids.len()) {
                scope.spawn(|| {
                    let lib = Library::open(dirs.clone(), exec_bin.clone());
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(id) = ids.get(i) else { break };
                        let r = match &lib {
                            Ok(l) => l.check(http, id),
                            Err(e) => Err(Error::Other(e.to_string())),
                        };
                        out.lock().unwrap()[i] = Some(r);
                    }
                });
            }
        });
        let results = out.into_inner().unwrap();
        ids.iter()
            .cloned()
            .zip(
                results
                    .into_iter()
                    .map(|r| r.unwrap_or_else(|| Err(Error::Other("check did not run".into())))),
            )
            .collect()
    }

    /// Install straight from a pasted link: resolve release, choose and download the asset,
    /// verify its checksum, integrate it and remember the source.
    pub fn install_from_source(
        &self,
        http: &dyn Http,
        parsed: Parsed,
        opts: &AddOptions,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<(AppRecord, Report)> {
        let mut source = parsed.source.clone();
        source.origin = Origin::User;
        let report = check::check(
            http,
            &source,
            &Installed {
                version: None,
                sha256: None,
            },
            Arch::host(),
        )?;
        let asset = match (&report.selection, &parsed.asset_url) {
            (Selection::One(a), _) => a.clone(),
            (_, Some(u)) => report
                .release
                .assets
                .iter()
                .find(|a| &a.url == u)
                .cloned()
                .ok_or_else(|| Error::Source(report.problem().unwrap_or_default()))?,
            _ => return Err(Error::Source(report.problem().unwrap_or_default())),
        };

        let dir = self.dirs.app_data_dir().join("downloads").join(format!(
            "{}-{}",
            std::process::id(),
            now()
        ));
        let (path, sha) = download::download_asset(http, &asset, &dir, progress)?;
        let result = self.add(
            &path,
            &AddOptions {
                install: Install::Move,
                id: opts.id.clone(),
            },
        );
        let _ = fs::remove_dir_all(&dir);
        let rec = result?;

        // Pin the asset pattern so later updates keep picking the same kind of file, but only
        // if the suggested pattern still selects exactly this asset.
        if source.asset_pattern.is_none() {
            let suggested = assets::suggest_pattern(&asset.name);
            let sel = assets::select(&report.release.assets, Some(&suggested), Arch::host());
            if sel.asset().map(|a| &a.name) == Some(&asset.name) {
                source.asset_pattern = Some(suggested);
            }
        }
        self.db.upsert_source(&rec.id, &source)?;
        self.db
            .record_installed(&rec.id, Some(&report.release.tag), Some(&sha))?;
        self.db.record_check(
            &rec.id,
            now(),
            Some("up-to-date"),
            None,
            Some(&report.release.tag),
        )?;
        Ok((rec, report))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture;
    use crate::http::fake::FakeHttp;
    use crate::paths::Dirs;
    use crate::source::parse_input;
    use sha2::{Digest, Sha256};

    struct Env {
        _tmp: tempfile::TempDir,
        lib: Library,
    }

    fn env() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let lib =
            Library::open(Dirs::with_root(tmp.path()), tmp.path().join("appofholding")).unwrap();
        Env { _tmp: tmp, lib }
    }

    fn release_json(tag: &str, sha: &str) -> String {
        format!(
            r#"{{"tag_name":"{tag}","prerelease":false,"draft":false,"assets":[
              {{"name":"Foo-{tag}-x86_64.AppImage","browser_download_url":"https://dl/foo-{tag}","digest":"sha256:{sha}"}},
              {{"name":"Foo-{tag}-aarch64.AppImage","browser_download_url":"https://dl/foo-arm"}}]}}"#
        )
    }

    #[test]
    fn install_from_github_link_end_to_end() {
        let e = env();
        let img = fixture::simple_appimage("Foo App", None);
        let sha = hex::encode(Sha256::digest(&img));
        let http = FakeHttp::default()
            .json(
                "https://api.github.com/repos/o/foo/releases/latest",
                &release_json("1.2.3", &sha),
            )
            .bytes("https://dl/foo-1.2.3", img);

        let parsed = parse_input("https://github.com/o/foo/").unwrap();
        let (rec, report) = e
            .lib
            .install_from_source(&http, parsed, &AddOptions::default(), &mut |_, _| {})
            .unwrap();
        assert_eq!(rec.id, "foo-app");
        assert_eq!(report.latest_version(), "1.2.3");
        assert!(rec.appimage_path.exists());

        let s = e.lib.source("foo-app").unwrap().unwrap();
        assert_eq!(s.installed_version.as_deref(), Some("1.2.3"));
        assert_eq!(s.installed_digest.as_deref(), Some(sha.as_str()));
        assert_eq!(
            s.source.asset_pattern.as_deref(),
            Some("Foo-*-x86_64.AppImage")
        );
        assert_eq!(s.last_status.as_deref(), Some("up-to-date"));
        // The download scratch directory is cleaned up.
        assert!(
            fs::read_dir(e.lib.dirs.app_data_dir().join("downloads"))
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[test]
    fn checksum_mismatch_installs_nothing() {
        let e = env();
        let http = FakeHttp::default()
            .json(
                "https://api.github.com/repos/o/foo/releases/latest",
                &release_json("1.0.0", "deadbeef"),
            )
            .bytes(
                "https://dl/foo-1.0.0",
                fixture::simple_appimage("Foo App", None),
            );
        let err = e
            .lib
            .install_from_source(
                &http,
                parse_input("o/foo").unwrap(),
                &AddOptions::default(),
                &mut |_, _| {},
            )
            .unwrap_err();
        assert!(matches!(err, Error::Checksum { .. }), "{err}");
        assert!(e.lib.list().unwrap().is_empty());
    }

    #[test]
    fn check_records_status_then_error_and_keeps_pattern() {
        let e = env();
        let img = fixture::simple_appimage(
            "Foo App",
            Some("gh-releases-zsync|o|foo|latest|Foo-*-x86_64.AppImage.zsync"),
        );
        let p = e._tmp.path().join("Foo.AppImage");
        fs::write(&p, img).unwrap();
        e.lib.add(&p, &AddOptions::default()).unwrap();

        // The embedded update info became the source automatically.
        let s = e.lib.source("foo-app").unwrap().unwrap();
        assert_eq!(s.source.origin, Origin::Embedded);

        // Installed version comes from the desktop file (1.2.3); latest is 1.3.0.
        let http = FakeHttp::default().json(
            "https://api.github.com/repos/o/foo/releases/latest",
            &release_json("1.3.0", "aa"),
        );
        let r = e.lib.check(&http, "foo-app").unwrap();
        assert_eq!(r.status.as_str(), "update-available");
        let s = e.lib.source("foo-app").unwrap().unwrap();
        assert_eq!(
            (s.last_status.as_deref(), s.last_latest.as_deref()),
            (Some("update-available"), Some("1.3.0"))
        );
        assert!(s.last_checked.is_some());

        // A network failure is recorded, not swallowed or left "checking".
        let bad = FakeHttp::default();
        assert!(e.lib.check(&bad, "foo-app").is_err());
        let s = e.lib.source("foo-app").unwrap().unwrap();
        assert!(s.last_status.is_none() && s.last_error.is_some());
        assert_eq!(s.last_latest.as_deref(), Some("1.3.0"));
    }

    #[test]
    fn zsync_source_compares_sha1_of_the_installed_file() {
        use sha1::{Digest as _, Sha1};
        let e = env();
        let img = fixture::simple_appimage("Foo App", Some("zsync|https://dl/foo.AppImage.zsync"));
        let mine = hex::encode(Sha1::digest(&img));
        let p = e._tmp.path().join("Foo.AppImage");
        fs::write(&p, img).unwrap();
        e.lib.add(&p, &AddOptions::default()).unwrap();

        let control = |sha: &str| {
            format!("zsync: 0.6.2\nFilename: Foo.AppImage\nURL: https://dl/Foo.AppImage\nSHA-1: {sha}\n\n").into_bytes()
        };
        // Same build published: up to date, with no version string needed (adopted app).
        let same = FakeHttp::default().bytes("https://dl/foo.AppImage.zsync", control(&mine));
        assert_eq!(
            e.lib.check(&same, "foo-app").unwrap().status.as_str(),
            "up-to-date"
        );
        // A different SHA-1 is an update, even though both identifiers look "numeric".
        let newer =
            FakeHttp::default().bytes("https://dl/foo.AppImage.zsync", control("0123456789abcdef"));
        assert_eq!(
            e.lib.check(&newer, "foo-app").unwrap().status.as_str(),
            "update-available"
        );
    }

    #[test]
    fn check_without_a_source_explains_what_to_do() {
        let e = env();
        let p = e._tmp.path().join("Foo.AppImage");
        fs::write(&p, fixture::simple_appimage("Foo App", None)).unwrap();
        e.lib.add(&p, &AddOptions::default()).unwrap();
        let err = e.lib.check(&FakeHttp::default(), "foo-app").unwrap_err();
        assert!(err.to_string().contains("no update source"), "{err}");
    }

    #[test]
    fn ambiguous_assets_ask_for_a_pattern() {
        let e = env();
        let rel = r#"{"tag_name":"1","prerelease":false,"draft":false,"assets":[
          {"name":"A-x86_64.AppImage","browser_download_url":"https://dl/a"},
          {"name":"B-x86_64.AppImage","browser_download_url":"https://dl/b"}]}"#;
        let http =
            FakeHttp::default().json("https://api.github.com/repos/o/r/releases/latest", rel);
        let err = e
            .lib
            .install_from_source(
                &http,
                parse_input("o/r").unwrap(),
                &AddOptions::default(),
                &mut |_, _| {},
            )
            .unwrap_err();
        assert!(err.to_string().contains("set an asset pattern"), "{err}");
    }

    #[test]
    fn set_and_clear_source() {
        let e = env();
        let p = e._tmp.path().join("Foo.AppImage");
        fs::write(&p, fixture::simple_appimage("Foo App", None)).unwrap();
        e.lib.add(&p, &AddOptions::default()).unwrap();
        e.lib
            .set_source("foo-app", &parse_input("o/foo").unwrap().source)
            .unwrap();
        assert!(e.lib.source("foo-app").unwrap().is_some());
        assert!(e.lib.clear_source("foo-app").unwrap());
        assert!(matches!(
            e.lib
                .set_source("nope", &parse_input("o/foo").unwrap().source),
            Err(Error::NotFound(_))
        ));
    }
}
