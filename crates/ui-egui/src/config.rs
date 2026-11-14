//! What the window remembers between sessions: preferences, recent projects
//! and the last profile.
//!
//! Two small text files in the per-user configuration folder:
//! `preferences.txt` (`key = value` lines) and `last.scprofile` (an ordinary
//! profile file). Both are read as hostile input: size-capped, unknown keys
//! skipped, bad values left at their defaults. A missing or unreadable file
//! is never an error; the window starts with defaults.

use crate::app::Preferences;
use slicecraft_doc::profile::{self, MAX_PROFILE_BYTES, Profile};
use std::path::{Path, PathBuf};

/// First line of the preferences file.
pub const PREFS_HEADER: &str = "# SliceCraft preferences 1";
/// Largest preferences file read.
pub const MAX_PREFS_BYTES: u64 = 64 * 1024;
/// Recent projects remembered.
pub const MAX_RECENT: usize = 8;
/// Overrides the configuration folder (portable installs, tests).
pub const CONFIG_DIR_ENV: &str = "SLICECRAFT_CONFIG_DIR";

const PREFS_FILE: &str = "preferences.txt";
const PROFILE_FILE: &str = "last.scprofile";

/// Everything remembered.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub prefs: Preferences,
    pub recent: Vec<PathBuf>,
    pub profile: Option<Profile>,
}

/// The per-user configuration folder, or `None` when the platform gives none.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(CONFIG_DIR_ENV).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        env("APPDATA").map(|d| d.join("SliceCraft"))
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library").join("Application Support").join("SliceCraft"))
    } else {
        env("XDG_CONFIG_HOME")
            .or_else(|| env("HOME").map(|h| h.join(".config")))
            .map(|d| d.join("slicecraft"))
    }
}

fn read_capped(path: &Path, limit: u64) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > limit {
        return None;
    }
    String::from_utf8(std::fs::read(path).ok()?).ok()
}

/// Read what was remembered. Problems are reported as notes, never errors.
pub fn load(dir: &Path) -> (Config, Vec<String>) {
    let mut notes = Vec::new();
    let mut config = Config::default();
    if let Some(text) = read_capped(&dir.join(PREFS_FILE), MAX_PREFS_BYTES) {
        let (prefs, recent, mut n) = prefs_from_text(&text);
        config.prefs = prefs;
        config.recent = recent;
        notes.append(&mut n);
    }
    if let Some(text) = read_capped(&dir.join(PROFILE_FILE), MAX_PROFILE_BYTES as u64) {
        match Profile::from_text(&text) {
            Ok((p, warnings)) if p.settings.validate().is_ok() => {
                if !warnings.is_empty() {
                    notes.push(format!("last profile: {} line(s) skipped", warnings.len()));
                }
                config.profile = Some(p);
            }
            _ => notes.push("the last profile could not be read; using defaults".into()),
        }
    }
    (config, notes)
}

/// Write preferences, recent projects and the profile.
pub fn save(dir: &Path, config: &Config) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let write = |name: &str, text: String| {
        let path = dir.join(name);
        std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
    };
    write(PREFS_FILE, prefs_to_text(&config.prefs, &config.recent))?;
    if let Some(p) = &config.profile {
        write(PROFILE_FILE, p.to_text())?;
    }
    Ok(())
}

/// Put `path` first in a recent list, without duplicates, capped.
pub fn push_recent(recent: &mut Vec<PathBuf>, path: &Path) {
    recent.retain(|p| p != path);
    recent.insert(0, path.to_path_buf());
    recent.truncate(MAX_RECENT);
}

pub fn prefs_to_text(p: &Preferences, recent: &[PathBuf]) -> String {
    let mut out = String::from(PREFS_HEADER);
    out.push('\n');
    let mut line = |k: &str, v: String| {
        out.push_str(k);
        out.push_str(" = ");
        out.push_str(&v);
        out.push('\n');
    };
    line("dark", p.dark.to_string());
    line("bed_grid", p.bed.grid.to_string());
    line("bed_volume", p.bed.volume.to_string());
    line("bed_axes", p.bed.axes.to_string());
    line("invert_zoom", p.invert_zoom.to_string());
    line("orbit_sensitivity", p.orbit_sensitivity.to_string());
    line("preview_after_slice", p.preview_after_slice.to_string());
    line("fit_on_import", p.fit_on_import.to_string());
    line("network_kind", p.network.kind.name().to_string());
    line("network_url", profile::quote(&p.network.url));
    line("network_key", profile::quote(&p.network.api_key));
    line("camera_url", profile::quote(&p.camera_url));
    line("ortho", p.ortho.to_string());
    line("wireframe", p.wireframe.to_string());
    line("gpu_view", p.gpu_view.to_string());
    line("inch_check", p.inch_check.to_string());
    line(
        "import_rotate",
        format!("{} {} {}", p.import_rotate[0], p.import_rotate[1], p.import_rotate[2]),
    );
    line("import_scale", p.import_scale.to_string());
    line("language", p.language.code().to_string());
    for l in p.keys.to_lines() {
        line("shortcut", profile::quote(&l));
    }
    for r in recent.iter().take(MAX_RECENT) {
        line("recent", profile::quote(&r.to_string_lossy()));
    }
    out
}

pub fn prefs_from_text(text: &str) -> (Preferences, Vec<PathBuf>, Vec<String>) {
    let mut p = Preferences::default();
    let mut recent = Vec::new();
    let mut notes = Vec::new();
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some(PREFS_HEADER) {
        notes.push("the preferences file is not recognised; using defaults".into());
        return (p, recent, notes);
    }
    for (i, line) in lines.enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        let flag = |target: &mut bool| match v {
            "true" => *target = true,
            "false" => *target = false,
            _ => {}
        };
        match k {
            "dark" => flag(&mut p.dark),
            "bed_grid" => flag(&mut p.bed.grid),
            "bed_volume" => flag(&mut p.bed.volume),
            "bed_axes" => flag(&mut p.bed.axes),
            "invert_zoom" => flag(&mut p.invert_zoom),
            "preview_after_slice" => flag(&mut p.preview_after_slice),
            "fit_on_import" => flag(&mut p.fit_on_import),
            "network_kind" => {
                if let Some(k) = slicecraft_io::network::HostKind::from_name(v) {
                    p.network.kind = k;
                }
            }
            "network_url" => p.network.url = profile::unquote(v).unwrap_or_default(),
            "network_key" => p.network.api_key = profile::unquote(v).unwrap_or_default(),
            "camera_url" => p.camera_url = profile::unquote(v).unwrap_or_default(),
            "ortho" => flag(&mut p.ortho),
            "language" => {
                if let Some(l) = crate::i18n::Language::from_code(v) {
                    p.language = l;
                }
            }
            "wireframe" => flag(&mut p.wireframe),
            "gpu_view" => flag(&mut p.gpu_view),
            "inch_check" => flag(&mut p.inch_check),
            "import_rotate" => {
                let n: Vec<f64> = v.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                if let [x, y, z] = n.as_slice()
                    && [x, y, z].iter().all(|a| a.is_finite() && a.abs() <= 360.0)
                {
                    p.import_rotate = [*x, *y, *z];
                }
            }
            "import_scale" => {
                if let Ok(k) = v.parse::<f64>()
                    && k.is_finite()
                    && (0.001..=1000.0).contains(&k)
                {
                    p.import_scale = k;
                }
            }
            "shortcut" => {
                if !profile::unquote(v).is_some_and(|l| p.keys.read_line(&l)) {
                    notes.push(format!("preferences line {}: shortcut not understood", i + 2));
                }
            }
            "orbit_sensitivity" => {
                if let Ok(s) = v.parse::<f32>()
                    && (0.2..=3.0).contains(&s)
                {
                    p.orbit_sensitivity = s;
                }
            }
            "recent" => {
                if recent.len() < MAX_RECENT
                    && let Some(path) = profile::unquote(v).filter(|s| !s.is_empty())
                {
                    recent.push(PathBuf::from(path));
                }
            }
            _ => notes.push(format!("preferences line {}: unknown key {k} skipped", i + 2)),
        }
    }
    (p, recent, notes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use slicecraft_doc::PrintSettings;

    #[test]
    fn preferences_round_trip() {
        let p = Preferences {
            dark: false,
            invert_zoom: true,
            orbit_sensitivity: 1.75,
            fit_on_import: true,
            ..Preferences::default()
        };
        let recent =
            vec![PathBuf::from("C:\\jobs\\a \"b\".scproj"), PathBuf::from("/tmp/x.scproj")];
        let (q, r, notes) = prefs_from_text(&prefs_to_text(&p, &recent));
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(r, recent);
        assert!(!q.dark && q.invert_zoom && q.fit_on_import);
        assert_eq!(q.orbit_sensitivity, 1.75);
    }

    #[test]
    fn hostile_preferences_fall_back_to_defaults() {
        let (p, r, notes) = prefs_from_text("garbage");
        assert!(p.dark && r.is_empty() && !notes.is_empty());
        let text = format!(
            "{PREFS_HEADER}\norbit_sensitivity = NaN\ndark = maybe\nrecent = unquoted\nwhat = 1\n{}",
            "recent = \"x\"\n".repeat(50)
        );
        let (p, r, notes) = prefs_from_text(&text);
        assert_eq!(p.orbit_sensitivity, 1.0);
        assert!(p.dark);
        assert_eq!(r.len(), MAX_RECENT);
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn recent_list_moves_to_front_and_caps() {
        let mut r = Vec::new();
        for i in 0..12 {
            push_recent(&mut r, Path::new(&format!("p{i}")));
        }
        push_recent(&mut r, Path::new("p5"));
        assert_eq!(r.len(), MAX_RECENT);
        assert_eq!(r.first().map(PathBuf::as_path), Some(Path::new("p5")));
        assert_eq!(r.iter().filter(|p| p.as_path() == Path::new("p5")).count(), 1);
    }

    #[test]
    fn config_saves_and_loads_from_a_folder() {
        let dir = std::env::temp_dir().join(format!("slicecraft-config-{}", std::process::id()));
        let config = Config {
            prefs: Preferences { dark: false, ..Preferences::default() },
            recent: vec![PathBuf::from("job.scproj")],
            profile: Some(Profile {
                name: "Kept".into(),
                settings: PrintSettings { layer_height: 0.24, ..PrintSettings::default() },
            }),
        };
        assert!(save(&dir, &config).is_ok());
        let (back, notes) = load(&dir);
        assert!(notes.is_empty(), "{notes:?}");
        assert!(!back.prefs.dark);
        assert_eq!(back.recent, config.recent);
        assert_eq!(back.profile, config.profile);
        // a broken profile is ignored with a note, not an error
        let _ = std::fs::write(dir.join(PROFILE_FILE), "not a profile");
        let (back, notes) = load(&dir);
        assert!(back.profile.is_none() && notes.len() == 1);
        let _ = std::fs::remove_dir_all(&dir);
        // a missing folder gives defaults
        let (back, notes) = load(&dir);
        assert!(back.prefs.dark && notes.is_empty());
    }
}
