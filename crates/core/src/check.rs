//! Checking a source for updates and "testing" a source before saving it.

use crate::assets::{self, Arch, Asset, Selection};
use crate::error::Result;
use crate::http::Http;
use crate::providers::{self, Release};
use crate::source::Source;
use crate::version::{self, Installed, Status};

#[derive(Debug, Clone)]
pub struct Report {
    pub release: Release,
    pub selection: Selection,
    pub status: Status,
}

impl Report {
    pub fn asset(&self) -> Option<&Asset> {
        self.selection.asset()
    }

    /// The version/identifier of the latest release.
    pub fn latest_version(&self) -> &str {
        &self.release.tag
    }

    /// Human-readable reason when no single asset could be chosen.
    pub fn problem(&self) -> Option<String> {
        match &self.selection {
            Selection::One(_) => None,
            Selection::None => Some(format!(
                "release {} has no AppImage matching your pattern/architecture (assets: {})",
                self.release.tag,
                names(&self.release.assets)
            )),
            Selection::Ambiguous(v) => Some(format!(
                "{} assets match; set an asset pattern to choose one: {}",
                v.len(),
                names(v)
            )),
        }
    }
}

fn names(a: &[Asset]) -> String {
    if a.is_empty() {
        return "none".into();
    }
    a.iter()
        .map(|x| x.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Fetch the latest release for `source`, choose the asset and decide whether it is newer than
/// what is installed. This is also what "Test source" runs.
pub fn check(
    http: &dyn Http,
    source: &Source,
    installed: &Installed<'_>,
    arch: Arch,
) -> Result<Report> {
    let release = providers::fetch_release(http, source)?;
    let selection = assets::select(&release.assets, source.asset_pattern.as_deref(), arch);
    let status = match selection.asset() {
        Some(asset) => version::decide(
            installed,
            &release.tag,
            asset.sha256.as_deref(),
            release.opaque,
        ),
        // Without a downloadable file there is nothing to update to.
        None => Status::Unknown,
    };
    Ok(Report {
        release,
        selection,
        status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::fake::FakeHttp;
    use crate::source::parse_input;

    const REL: &str = r#"{"tag_name":"v2.0.0","prerelease":false,"draft":false,"assets":[
        {"name":"Foo-2.0.0-x86_64.AppImage","browser_download_url":"https://dl/a","size":1,"digest":"sha256:aa"},
        {"name":"Foo-2.0.0-aarch64.AppImage","browser_download_url":"https://dl/b","size":1}]}"#;

    fn http() -> FakeHttp {
        FakeHttp::default().json("https://api.github.com/repos/o/r/releases/latest", REL)
    }

    #[test]
    fn reports_update_and_picks_asset() {
        let s = parse_input("o/r").unwrap().source;
        let r = check(
            &http(),
            &s,
            &Installed {
                version: Some("1.0.0"),
                sha256: None,
            },
            Arch::X86_64,
        )
        .unwrap();
        assert_eq!(r.status, Status::UpdateAvailable);
        assert_eq!(r.asset().unwrap().name, "Foo-2.0.0-x86_64.AppImage");
        assert_eq!(r.latest_version(), "v2.0.0");
        assert!(r.problem().is_none());
    }

    #[test]
    fn same_digest_is_up_to_date_even_with_unknown_version() {
        let s = parse_input("o/r").unwrap().source;
        let r = check(
            &http(),
            &s,
            &Installed {
                version: None,
                sha256: Some("AA"),
            },
            Arch::X86_64,
        )
        .unwrap();
        assert_eq!(r.status, Status::UpToDate);
    }

    #[test]
    fn explains_why_no_asset_matches() {
        let mut s = parse_input("o/r").unwrap().source;
        s.asset_pattern = Some("Bar-*.AppImage".into());
        let r = check(
            &http(),
            &s,
            &Installed {
                version: Some("1"),
                sha256: None,
            },
            Arch::X86_64,
        )
        .unwrap();
        assert_eq!(r.status, Status::Unknown);
        let p = r.problem().unwrap();
        assert!(
            p.contains("no AppImage matching") && p.contains("Foo-2.0.0-x86_64.AppImage"),
            "{p}"
        );
    }
}
