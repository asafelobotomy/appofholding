//! uQ's theme tokens (`~/.local/state/uq/theme/tokens.json`, written by uqd) as the palette the
//! interface wears. Without uQ (no file, or one that does not parse) there is no palette and the
//! window follows the desktop.

use std::path::PathBuf;

use serde_json::{Value, json};

const REQUIRED: [&str; 7] = [
    "bg",
    "surface",
    "overlay",
    "text",
    "text_muted",
    "accent",
    "accent_text",
];

/// `$APPOFHOLDING_UQ_TOKENS`, else uQ's tokens file (see the vendored `uq_theme.rs`).
pub fn tokens_path() -> Option<PathBuf> {
    crate::uq_theme::tokens_path("APPOFHOLDING_UQ_TOKENS")
}

/// The palette (same keys as `Themes.qml`, plus `variant` and `icons`) for tokens JSON text, or
/// `None` when it is not usable.
pub fn palette_from(text: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(text).ok()?;
    let c = v.get("colors")?;
    let get = |k: &str| {
        c.get(k)
            .and_then(Value::as_str)
            .filter(|s| s.starts_with('#'))
    };
    if REQUIRED.iter().any(|k| get(k).is_none()) {
        return None;
    }
    let g = |k: &str, fallback: &str| get(k).or_else(|| get(fallback)).unwrap_or("#808080");
    Some(json!({
        "bg": g("bg", "bg"),
        "bgAlt": g("surface", "bg"),
        "bgActive": g("overlay", "surface"),
        "text": g("text", "text"),
        "dim": g("text_muted", "text"),
        "accent": g("accent", "text"),
        "accentText": g("accent_text", "bg"),
        "link": g("accent", "text"),
        "visited": g("text_muted", "text"),
        "positive": g("success", "text"),
        "neutral": g("warning", "text"),
        "negative": g("error", "text"),
        "variant": v.get("variant").and_then(Value::as_str).unwrap_or("dark"),
        "icons": v.pointer("/icons/theme").and_then(Value::as_str).unwrap_or(""),
    }))
}

/// The current uQ palette, read from disk.
pub fn load() -> Option<Value> {
    palette_from(&std::fs::read_to_string(tokens_path()?).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKENS: &str = r##"{"variant":"light","colors":{"bg":"#ffffff","surface":"#eeeeee","surface_alt":"#dddddd",
        "overlay":"#cccccc","text":"#111111","text_muted":"#555555","accent":"#0066cc","accent_text":"#ffffff",
        "border":"#999999","success":"#00aa00","warning":"#ffaa00","error":"#cc0000"},
        "icons":{"theme":"Papirus"}}"##;

    #[test]
    fn tokens_become_a_palette() {
        let p = palette_from(TOKENS).unwrap();
        assert_eq!(p["bg"], "#ffffff");
        assert_eq!(p["bgAlt"], "#eeeeee");
        assert_eq!(p["bgActive"], "#cccccc");
        assert_eq!(p["dim"], "#555555");
        assert_eq!(p["negative"], "#cc0000");
        assert_eq!(p["variant"], "light");
        assert_eq!(p["icons"], "Papirus");
    }

    #[test]
    fn unusable_tokens_give_no_palette() {
        assert!(palette_from("not json").is_none());
        assert!(palette_from("{}").is_none());
        assert!(palette_from(r#"{"colors":{"bg":"red"}}"#).is_none());
    }
}
