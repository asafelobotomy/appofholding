use super::*;
use appofholding_core::assets::Asset;
use appofholding_core::providers::Release;

fn app(id: &str) -> AppRecord {
    AppRecord {
        id: id.into(),
        name: id.to_uppercase(),
        appimage_path: format!("/apps/{id}.AppImage").into(),
        desktop_file: format!("/apps/appofholding-{id}.desktop").into(),
        icon_file: None,
        version: Some("1.0".into()),
        sha256: "00".repeat(32),
        size: 1,
        appimage_type: 2,
        update_info: None,
        added_at: 0,
        external_file: false,
    }
}

fn src(id: &str, status: &str, latest: &str, policy: Policy) -> SourceRecord {
    SourceRecord {
        app_id: id.into(),
        source: Source::new(source::Provider::GitHub {
            owner: "o".into(),
            repo: id.into(),
        }),
        last_checked: Some(10),
        last_status: Some(status.into()),
        last_error: None,
        last_latest: Some(latest.into()),
        installed_version: Some("1.0".into()),
        installed_digest: None,
        policy,
        skipped_version: None,
        notified_version: None,
        etag: None,
        etag_ctx: None,
    }
}

#[test]
fn list_json_counts_updates_the_user_can_act_on() {
    let apps = ["a", "b", "c", "d", "e"].map(app);
    let mut skipped = src("d", "update-available", "3.0", Policy::Notify);
    skipped.skipped_version = Some("3.0".into());
    let sources = vec![
        src("a", "update-available", "2.0", Policy::Notify),
        src("b", "update-available", "2.0", Policy::Pinned),
        src("c", "up-to-date", "1.0", Policy::Auto),
        skipped,
    ];
    let v = list_json(&apps, &sources);
    assert_eq!(v["version"], 1);
    assert_eq!(v["updates"], 1);
    let rows = v["apps"].as_array().unwrap();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0]["update"], true);
    assert_eq!(rows[0]["latest"], "2.0");
    assert_eq!(rows[0]["policy"], "notify");
    assert_eq!(rows[0]["source"], "github.com/o/a [latest]");
    assert_eq!(rows[0]["desktop_id"], "appofholding-a.desktop");
    assert_eq!(rows[1]["update"], false, "pinned");
    assert_eq!(rows[2]["status"], "up-to-date");
    assert_eq!(rows[3]["update"], false, "skipped");
    assert_eq!(rows[3]["skipped"], "3.0");
    assert_eq!(rows[4]["has_source"], false);
    assert!(rows[4]["policy"].is_null());
}

fn report(tag: &str, status: Status, assets: usize) -> Report {
    let asset = |n: usize| Asset {
        name: format!("A-{n}.AppImage"),
        url: format!("https://x/{n}"),
        size: None,
        sha256: None,
    };
    let list: Vec<Asset> = (0..assets).map(asset).collect();
    Report {
        release: Release {
            tag: tag.into(),
            name: None,
            prerelease: false,
            published_at: None,
            notes: None,
            assets: list.clone(),
            opaque: false,
        },
        selection: match assets {
            0 => Selection::None,
            1 => Selection::One(list[0].clone()),
            _ => Selection::Ambiguous(list),
        },
        status,
    }
}

#[test]
fn check_json_rows_and_summaries() {
    let results = vec![
        (
            "a".to_string(),
            Ok(report("v2", Status::UpdateAvailable, 1)),
        ),
        ("b".to_string(), Ok(report("v1", Status::UpToDate, 1))),
        (
            "c".to_string(),
            Ok(report("v3", Status::UpdateAvailable, 2)),
        ),
        (
            "d".to_string(),
            Err(appofholding_core::Error::Other("offline".into())),
        ),
    ];
    let v = check_json(&results, &[]);
    assert_eq!(v["version"], 1);
    assert_eq!(v["updates"], 1, "an ambiguous release is not an update yet");
    let rows = v["apps"].as_array().unwrap();
    assert_eq!(rows[0]["status"], "update-available");
    assert_eq!(rows[0]["latest"], "v2");
    assert!(rows[0]["problem"].is_null() && rows[0]["error"].is_null());
    assert!(
        rows[2]["problem"]
            .as_str()
            .unwrap()
            .contains("2 assets match")
    );
    assert_eq!(rows[3]["error"], "offline");
    assert!(rows[3]["status"].is_null());

    assert_eq!(
        check_summary(&results[0].1, None),
        "Version v2 is available."
    );
    assert_eq!(
        check_summary(&results[1].1, None),
        "Up to date (latest: v1)."
    );
    assert!(check_summary(&results[2].1, None).starts_with("Could not choose the update"));
    assert!(check_summary(&results[3].1, None).contains("offline"));
}

#[test]
fn check_json_does_not_count_pinned_or_skipped_updates() {
    let results = vec![
        (
            "p".to_string(),
            Ok(report("v2", Status::UpdateAvailable, 1)),
        ),
        (
            "s".to_string(),
            Ok(report("v2", Status::UpdateAvailable, 1)),
        ),
        (
            "n".to_string(),
            Ok(report("v2", Status::UpdateAvailable, 1)),
        ),
    ];
    let mut skipped = src("s", "update-available", "v2", Policy::Notify);
    skipped.skipped_version = Some("v2".into());
    let sources = vec![
        src("p", "update-available", "v2", Policy::Pinned),
        skipped,
        src("n", "update-available", "v2", Policy::Notify),
    ];
    let v = check_json(&results, &sources);
    assert_eq!(v["updates"], 1);
    let rows = v["apps"].as_array().unwrap();
    assert_eq!(rows[0]["update"], false);
    assert_eq!(rows[1]["update"], false);
    assert_eq!(rows[2]["update"], true);
    assert!(check_summary(&results[0].1, Some(&sources[0])).contains("pinned"));
    assert!(check_summary(&results[1].1, Some(&sources[1])).contains("skip"));
    assert_eq!(
        check_summary(&results[2].1, Some(&sources[2])),
        "Version v2 is available."
    );
}

#[test]
fn schedule_json_shapes() {
    assert_eq!(
        schedule_json(true, Some(Interval::Weekly)),
        json!({"version": 1, "installed": true, "interval": "weekly"})
    );
    assert_eq!(
        schedule_json(true, None),
        json!({"version": 1, "installed": true, "interval": null})
    );
    assert_eq!(
        schedule_json(false, Some(Interval::Daily)),
        json!({"version": 1, "installed": false, "interval": null})
    );
}
