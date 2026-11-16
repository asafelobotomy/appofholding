//! `appofholding scan`: have oysterAV look at the AppImages in the library, and keep what it says.
//!
//! With `--json` one object (`{"version": 1, "ok": true, "results": [...], "threats": n}`); with
//! `--ndjson` a line per step (`start`, one `scanned` per app, `done`), for progress bars. The exit
//! status is 1 when oysterAV found something. Nothing is moved or deleted: that is for the person
//! to decide, in oysterAV.

use anyhow::{Result, bail};
use appofholding_core::Library;
use appofholding_core::scan::ScanState;
use clap::Args;
use serde_json::{Value, json};

use crate::pagejson;

#[derive(Args)]
pub struct ScanArgs {
    /// Apps to scan (ids from `appofholding list`).
    ids: Vec<String>,
    /// Scan every app in the library.
    #[arg(long, conflicts_with = "ids")]
    all: bool,
    /// Machine-readable output: one object when done.
    #[arg(long)]
    json: bool,
    /// One JSON line per step (start, one per app, done), for progress bars.
    #[arg(long, conflicts_with = "json")]
    ndjson: bool,
}

fn row(lib: &Library, id: &str) -> Value {
    let name = lib.get(id).map_or_else(|_| id.to_string(), |a| a.name);
    match lib.scan_app(id) {
        Ok(v) => json!({
            "id": id, "name": name, "state": v.state.as_str(), "detail": v.detail, "at": v.at
        }),
        Err(e) => json!({
            "id": id, "name": name, "state": "error", "detail": e.to_string(), "at": null
        }),
    }
}

pub fn run(lib: &Library, args: ScanArgs) -> Result<()> {
    let ids: Vec<String> = if args.all {
        lib.list()?.into_iter().map(|a| a.id).collect()
    } else if args.ids.is_empty() {
        let msg = "say which apps to scan: their ids, or --all";
        if args.json || args.ndjson {
            pagejson::print_error(&anyhow::anyhow!(msg));
            std::process::exit(1);
        }
        bail!(msg);
    } else {
        args.ids
    };
    if !args.json && !args.ndjson {
        println!("Scanning {} app(s) with oysterAV…", ids.len());
    }
    if args.ndjson {
        pagejson::event(&json!({ "event": "start", "total": ids.len() }));
    }
    let mut results = Vec::new();
    for (n, id) in ids.iter().enumerate() {
        let r = row(lib, id);
        if args.ndjson {
            let mut ev = json!({ "event": "scanned", "index": n + 1, "total": ids.len() });
            ev.as_object_mut()
                .unwrap()
                .extend(r.as_object().cloned().unwrap_or_default());
            pagejson::event(&ev);
        } else if !args.json {
            let detail = r["detail"].as_str().unwrap_or("");
            let sep = if detail.is_empty() { "" } else { ": " };
            println!(
                "{:<24} {}{sep}{detail}",
                id,
                r["state"].as_str().unwrap_or("")
            );
        }
        results.push(r);
    }
    let count = |s: ScanState| results.iter().filter(|r| r["state"] == s.as_str()).count();
    let (threats, errors) = (count(ScanState::Threat), count(ScanState::Error));
    if args.ndjson {
        pagejson::event(&json!({
            "event": "done", "threats": threats, "errors": errors, "total": ids.len()
        }));
    } else if args.json {
        println!(
            "{}",
            json!({ "version": 1, "ok": true, "results": results, "threats": threats, "errors": errors })
        );
    }
    if threats > 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// Adds each app's scan verdict to a `list --json` document: `"scan": {state, detail, at}` or null.
pub fn add_verdicts(lib: &Library, doc: &mut Value) {
    let Some(apps) = doc.get_mut("apps").and_then(Value::as_array_mut) else {
        return;
    };
    for app in apps {
        let id = app["id"].as_str().unwrap_or("").to_string();
        app["scan"] = match lib.scan_verdict(&id) {
            Ok(Some(v)) => json!({ "state": v.state.as_str(), "detail": v.detail, "at": v.at }),
            _ => Value::Null,
        };
    }
}
