//! Profile files and built-in presets.
//!
//! A profile is UTF-8 text, one `key = value` per line, read and written
//! through the settings schema, so every saved value passes the same checks as
//! one typed into the editor:
//!
//! ```text
//! # SliceCraft profile 1
//! name = Standard 0.20 mm
//! layer_height = 0.2
//! infill_pattern = grid
//! start_script = "G28\nG1 Z5"
//! ```
//!
//! Unknown keys and bad values are skipped and reported, never fatal: a
//! profile from a newer version still loads what it can.

use crate::settings::{SETTINGS, SettingKind, SettingValue, spec};
use crate::{DocError, PrintSettings};

/// First line of every profile file.
pub const HEADER: &str = "# SliceCraft profile 1";
/// Largest profile file accepted, in bytes.
pub const MAX_PROFILE_BYTES: usize = 1 << 20;

/// A named profile.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub name: String,
    pub settings: PrintSettings,
}

/// A string as a double-quoted, escaped value (shared with project files).
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The inverse of `quote`; `None` when the value is not a well-formed quoted string.
pub fn unquote(s: &str) -> Option<String> {
    let inner = s.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                _ => return None,
            }
        } else {
            out.push(c);
        }
    }
    Some(out)
}

/// A setting value as profile text.
pub fn format_value(kind: SettingKind, v: &SettingValue) -> String {
    match (kind, v) {
        (SettingKind::Choice(opts), SettingValue::Choice(i)) => {
            opts.get(*i).map_or_else(|| i.to_string(), |s| s.to_string())
        }
        (_, SettingValue::Float(x)) => format!("{x}"),
        (_, SettingValue::Int(x)) => x.to_string(),
        (_, SettingValue::Bool(b)) => b.to_string(),
        (_, SettingValue::Choice(i)) => i.to_string(),
        (_, SettingValue::Text(t)) => quote(t),
    }
}

/// Profile text back into a setting value of `kind`.
pub fn parse_value(kind: SettingKind, raw: &str) -> Option<SettingValue> {
    match kind {
        SettingKind::Float { .. } => raw.parse::<f64>().ok().map(SettingValue::Float),
        SettingKind::Int { .. } => raw.parse::<i64>().ok().map(SettingValue::Int),
        SettingKind::Bool => match raw {
            "true" | "yes" | "1" => Some(SettingValue::Bool(true)),
            "false" | "no" | "0" => Some(SettingValue::Bool(false)),
            _ => None,
        },
        SettingKind::Choice(opts) => opts
            .iter()
            .position(|o| o.eq_ignore_ascii_case(raw))
            .or_else(|| raw.parse::<usize>().ok())
            .map(SettingValue::Choice),
        SettingKind::Text => unquote(raw).map(SettingValue::Text),
    }
}

impl Profile {
    /// The profile as text. Every setting is written, so a file is complete
    /// on its own and survives a change of defaults.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(HEADER);
        out.push('\n');
        out.push_str(&format!("name = {}\n", quote(&self.name)));
        for s in SETTINGS {
            if let Some(v) = self.settings.get(s.key) {
                out.push_str(s.key);
                out.push_str(" = ");
                out.push_str(&format_value(s.kind, &v));
                out.push('\n');
            }
        }
        out
    }

    /// Read a profile. Returns it with one warning per skipped line.
    pub fn from_text(text: &str) -> Result<(Profile, Vec<String>), DocError> {
        if text.len() > MAX_PROFILE_BYTES {
            return Err(DocError::InvalidSetting("the profile file is too large".into()));
        }
        let mut lines = text.lines();
        if lines.next().map(str::trim) != Some(HEADER) {
            return Err(DocError::InvalidSetting("not a SliceCraft profile".into()));
        }
        let mut profile = Profile { name: "Imported".into(), settings: PrintSettings::default() };
        let mut warnings = Vec::new();
        for (n, line) in lines.enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, raw)) = line.split_once('=') else {
                warnings.push(format!("line {}: no '='", n + 2));
                continue;
            };
            let (key, raw) = (key.trim(), raw.trim());
            if key == "name" {
                profile.name = unquote(raw).unwrap_or_else(|| raw.to_string());
                continue;
            }
            let Some(s) = spec(key) else {
                warnings.push(format!("line {}: unknown setting {key}", n + 2));
                continue;
            };
            let Some(v) = parse_value(s.kind, raw) else {
                warnings.push(format!("line {}: cannot read {key}", n + 2));
                continue;
            };
            if let Err(e) = profile.settings.set(key, v) {
                warnings.push(format!("line {}: {e}", n + 2));
            }
        }
        Ok((profile, warnings))
    }
}

/// Built-in starting points. Values are SliceCraft's own choices for a
/// typical 0.4 mm nozzle and PLA.
pub fn presets() -> Vec<Profile> {
    let base = PrintSettings::default();
    vec![
        Profile { name: "Standard 0.20 mm".into(), settings: base.clone() },
        Profile {
            name: "Fine 0.12 mm".into(),
            settings: PrintSettings {
                layer_height: 0.12,
                first_layer_height: 0.2,
                solid_layers_top: 6,
                solid_layers_bottom: 5,
                print_speed: 50.0,
                perimeter_speed: 30.0,
                ..base.clone()
            },
        },
        Profile {
            name: "Draft 0.28 mm".into(),
            settings: PrintSettings {
                layer_height: 0.28,
                first_layer_height: 0.3,
                solid_layers_top: 3,
                solid_layers_bottom: 2,
                infill_percentage: 15.0,
                print_speed: 80.0,
                perimeter_speed: 50.0,
                ..base.clone()
            },
        },
        Profile {
            name: "Strong parts".into(),
            settings: PrintSettings {
                perimeter_count: 4,
                solid_layers_top: 5,
                solid_layers_bottom: 4,
                infill_percentage: 40.0,
                infill_pattern: crate::InfillPattern::Triangles,
                ..base
            },
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_is_exact() {
        let mut s = PrintSettings {
            layer_height: 0.16,
            infill_pattern: crate::InfillPattern::Concentric,
            start_script: "G28 ; \"home\"\nG1 Z5\\".into(),
            ..PrintSettings::default()
        };
        assert!(s.set("support_enabled", SettingValue::Bool(true)).is_ok());
        let p = Profile { name: "Mine \"best\"".into(), settings: s };
        let text = p.to_text();
        let back = Profile::from_text(&text);
        assert!(back.as_ref().is_ok_and(|(q, w)| q == &p && w.is_empty()), "{back:?}");
        // and writing it again gives the same bytes
        assert_eq!(back.map(|(q, _)| q.to_text()).ok(), Some(text));
    }

    #[test]
    fn bad_lines_are_skipped_with_warnings() {
        let text = format!(
            "{HEADER}\nlayer_height = 0.1\nlayer_height = 99\nno_such = 1\ngarbage\ninfill_pattern = GRID\nstart_script = \"unterminated\n"
        );
        let r = Profile::from_text(&text);
        assert!(r.is_ok());
        let Ok((p, w)) = r else { return };
        assert_eq!(p.settings.layer_height, 0.1);
        assert_eq!(p.settings.infill_pattern, crate::InfillPattern::Grid);
        assert_eq!(w.len(), 4, "{w:?}");
    }

    #[test]
    fn foreign_and_huge_files_are_refused() {
        assert!(Profile::from_text("hello").is_err());
        let big = format!("{HEADER}\n{}", "x".repeat(MAX_PROFILE_BYTES));
        assert!(Profile::from_text(&big).is_err());
    }

    #[test]
    fn presets_validate() {
        for p in presets() {
            assert!(p.settings.validate().is_ok(), "{}", p.name);
        }
    }
}
