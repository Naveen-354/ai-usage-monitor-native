//! Keyboard shortcut parser, validator, and formatter.
//! Pure logic with zero UI or OS dependencies.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Shortcut {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutError {
    Empty,
    MissingModifier,
    MissingKey,
    MultipleKeys(Vec<String>),
    DuplicateModifier(String),
    UnknownToken(String),
}

impl std::fmt::Display for ShortcutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShortcutError::Empty => write!(f, "Shortcut cannot be empty"),
            ShortcutError::MissingModifier => write!(f, "Shortcut requires at least one modifier (Ctrl, Alt, Shift, Meta)"),
            ShortcutError::MissingKey => write!(f, "Shortcut requires a primary key"),
            ShortcutError::MultipleKeys(keys) => write!(f, "Multiple keys found: {}", keys.join(", ")),
            ShortcutError::DuplicateModifier(m) => write!(f, "Duplicate modifier: {m}"),
            ShortcutError::UnknownToken(t) => write!(f, "Unknown key or modifier: {t}"),
        }
    }
}

impl std::error::Error for ShortcutError {}

/// Known valid primary keys (canonical uppercase representation)
fn canonicalize_key(token: &str) -> Option<String> {
    let lower = token.trim().to_lowercase();
    match lower.as_str() {
        // Single characters a-z, 0-9
        s if s.len() == 1 => {
            let c = s.chars().next().unwrap();
            if c.is_ascii_alphanumeric() {
                Some(c.to_ascii_uppercase().to_string())
            } else {
                match c {
                    '`' | '~' => Some("Backquote".into()),
                    '-' | '_' => Some("Minus".into()),
                    '=' | '+' => Some("Equal".into()),
                    '[' | '{' => Some("BracketLeft".into()),
                    ']' | '}' => Some("BracketRight".into()),
                    '\\' | '|' => Some("Backslash".into()),
                    ';' | ':' => Some("Semicolon".into()),
                    '\'' | '"' => Some("Quote".into()),
                    ',' | '<' => Some("Comma".into()),
                    '.' | '>' => Some("Period".into()),
                    '/' | '?' => Some("Slash".into()),
                    _ => None,
                }
            }
        }
        // Function keys F1-F24
        s if s.starts_with('f') && s.len() >= 2 => {
            if let Ok(num) = s[1..].parse::<u32>() {
                if (1..=24).contains(&num) {
                    Some(format!("F{num}"))
                } else {
                    None
                }
            } else {
                None
            }
        }
        // Named keys
        "space" => Some("Space".into()),
        "enter" | "return" => Some("Enter".into()),
        "tab" => Some("Tab".into()),
        "escape" | "esc" => Some("Escape".into()),
        "backspace" => Some("Backspace".into()),
        "delete" | "del" => Some("Delete".into()),
        "insert" | "ins" => Some("Insert".into()),
        "home" => Some("Home".into()),
        "end" => Some("End".into()),
        "pageup" | "pgup" => Some("PageUp".into()),
        "pagedown" | "pgdn" => Some("PageDown".into()),
        "up" | "arrowup" => Some("Up".into()),
        "down" | "arrowdown" => Some("Down".into()),
        "left" | "arrowleft" => Some("Left".into()),
        "right" | "arrowright" => Some("Right".into()),
        _ => None,
    }
}

/// Parse a shortcut string (e.g., "Ctrl+Alt+T", "CommandOrControl+Shift+E").
/// Case-insensitive, order-insensitive modifiers.
/// Requires at least one modifier and exactly one primary key.
/// Rejects duplicate modifiers and unknown tokens.
pub fn parse_shortcut(input: &str) -> Result<Shortcut, ShortcutError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(ShortcutError::Empty);
    }

    let parts: Vec<&str> = input.split('+').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return Err(ShortcutError::Empty);
    }

    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut meta = false;

    let mut seen_modifiers = HashSet::new();
    let mut found_keys = Vec::new();

    for part in parts {
        let lower = part.to_lowercase();
        match lower.as_str() {
            "ctrl" | "control" => {
                if !seen_modifiers.insert("ctrl") {
                    return Err(ShortcutError::DuplicateModifier("Ctrl".into()));
                }
                ctrl = true;
            }
            "alt" | "option" => {
                if !seen_modifiers.insert("alt") {
                    return Err(ShortcutError::DuplicateModifier("Alt".into()));
                }
                alt = true;
            }
            "shift" => {
                if !seen_modifiers.insert("shift") {
                    return Err(ShortcutError::DuplicateModifier("Shift".into()));
                }
                shift = true;
            }
            "meta" | "super" | "win" | "windows" | "cmd" | "command" | "commandorcontrol" => {
                if !seen_modifiers.insert("meta") {
                    return Err(ShortcutError::DuplicateModifier("Meta".into()));
                }
                meta = true;
            }
            _ => {
                if let Some(canonical) = canonicalize_key(part) {
                    found_keys.push(canonical);
                } else {
                    return Err(ShortcutError::UnknownToken(part.to_string()));
                }
            }
        }
    }

    if !ctrl && !alt && !shift && !meta {
        return Err(ShortcutError::MissingModifier);
    }

    match found_keys.len() {
        0 => Err(ShortcutError::MissingKey),
        1 => Ok(Shortcut {
            ctrl,
            alt,
            shift,
            meta,
            key: found_keys.into_iter().next().unwrap(),
        }),
        _ => Err(ShortcutError::MultipleKeys(found_keys)),
    }
}

/// Formats a shortcut struct into standard canonical string:
/// `[Ctrl+][Alt+][Shift+][Meta+]<Key>`
pub fn format_shortcut(sc: &Shortcut) -> String {
    let mut parts = Vec::new();
    if sc.ctrl {
        parts.push("Ctrl");
    }
    if sc.alt {
        parts.push("Alt");
    }
    if sc.shift {
        parts.push("Shift");
    }
    if sc.meta {
        parts.push("Meta");
    }
    parts.push(&sc.key);
    parts.join("+")
}

/// Check for conflicts/duplicates across the three main global shortcuts.
/// Returns a list of conflict messages if any two shortcuts parse to the same combination.
pub fn check_shortcut_duplicates(toggle: &str, expand: &str, focus: &str) -> Vec<String> {
    let mut conflicts = Vec::new();
    let entries = [
        ("Toggle overlay", parse_shortcut(toggle)),
        ("Expand overlay", parse_shortcut(expand)),
        ("Focus overlay", parse_shortcut(focus)),
    ];

    for i in 0..entries.len() {
        for j in (i + 1)..entries.len() {
            if let (Ok(sc1), Ok(sc2)) = (&entries[i].1, &entries[j].1) {
                if sc1 == sc2 {
                    conflicts.push(format!(
                        "'{}' and '{}' use the same shortcut: {}",
                        entries[i].0,
                        entries[j].0,
                        format_shortcut(sc1)
                    ));
                }
            }
        }
    }

    conflicts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_shortcuts() {
        let s = parse_shortcut("Ctrl+Alt+T").unwrap();
        assert_eq!(s, Shortcut { ctrl: true, alt: true, shift: false, meta: false, key: "T".into() });

        let s2 = parse_shortcut("shift+alt+f1").unwrap();
        assert_eq!(s2, Shortcut { ctrl: false, alt: true, shift: true, meta: false, key: "F1".into() });

        let s3 = parse_shortcut("CommandOrControl+Shift+E").unwrap();
        assert_eq!(s3, Shortcut { ctrl: false, alt: false, shift: true, meta: true, key: "E".into() });
    }

    #[test]
    fn order_and_case_insensitive() {
        let s1 = parse_shortcut("alt+CTRL+k").unwrap();
        let s2 = parse_shortcut("Ctrl+Alt+K").unwrap();
        assert_eq!(s1, s2);
    }

    #[test]
    fn rejects_missing_modifier() {
        assert_eq!(parse_shortcut("T"), Err(ShortcutError::MissingModifier));
        assert_eq!(parse_shortcut("F1"), Err(ShortcutError::MissingModifier));
    }

    #[test]
    fn rejects_missing_key() {
        assert_eq!(parse_shortcut("Ctrl+Alt"), Err(ShortcutError::MissingKey));
        assert_eq!(parse_shortcut("Shift+"), Err(ShortcutError::MissingKey));
    }

    #[test]
    fn rejects_multiple_keys() {
        assert!(matches!(parse_shortcut("Ctrl+A+B"), Err(ShortcutError::MultipleKeys(_))));
    }

    #[test]
    fn rejects_duplicate_modifiers() {
        assert_eq!(parse_shortcut("Ctrl+ctrl+A"), Err(ShortcutError::DuplicateModifier("Ctrl".into())));
    }

    #[test]
    fn rejects_unknown_tokens() {
        assert_eq!(parse_shortcut("Ctrl+BogusToken"), Err(ShortcutError::UnknownToken("BogusToken".into())));
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(parse_shortcut(""), Err(ShortcutError::Empty));
        assert_eq!(parse_shortcut("   "), Err(ShortcutError::Empty));
    }

    #[test]
    fn format_roundtrip() {
        let inputs = ["Ctrl+Alt+T", "Ctrl+Shift+F12", "Alt+Shift+Meta+Space", "Ctrl+Delete"];
        for input in inputs {
            let sc = parse_shortcut(input).unwrap();
            let formatted = format_shortcut(&sc);
            let re_parsed = parse_shortcut(&formatted).unwrap();
            assert_eq!(sc, re_parsed, "Failed roundtrip for {input}");
        }
    }

    #[test]
    fn detects_conflicts() {
        let conflicts = check_shortcut_duplicates("Ctrl+Alt+T", "ctrl+alt+t", "Ctrl+Alt+F");
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].contains("Toggle overlay"));
        assert!(conflicts[0].contains("Expand overlay"));

        let no_conflicts = check_shortcut_duplicates("Ctrl+Alt+T", "Ctrl+Alt+E", "Ctrl+Alt+F");
        assert!(no_conflicts.is_empty());
    }
}
