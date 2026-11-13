//! Keyboard shortcuts that can be changed: each action has a default
//! binding; the preferences keep only the ones the user changed.

use eframe::egui::{Context, Key};

/// What a shortcut does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Import,
    OpenProject,
    SaveProject,
    SaveProjectAs,
    NewProject,
    ExportGcode,
    Slice,
    PrintSettings,
    Undo,
    Redo,
    SelectAll,
    Duplicate,
    Remove,
    SelectTool,
    MoveTool,
    RotateTool,
    ScaleTool,
    FitView,
}

/// A key with its modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub key: Key,
    pub cmd: bool,
    pub shift: bool,
}

impl Binding {
    const fn new(key: Key, cmd: bool, shift: bool) -> Self {
        Self { key, cmd, shift }
    }

    /// As shown in menus and saved in preferences: "Ctrl+Shift+O".
    pub fn text(self) -> String {
        let mut s = String::new();
        if self.cmd {
            s.push_str("Ctrl+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        s.push_str(self.key.name());
        s
    }

    /// Read "Ctrl+Shift+O" back; `None` for anything else.
    pub fn parse(text: &str) -> Option<Self> {
        let mut cmd = false;
        let mut shift = false;
        let mut key = None;
        for part in text.split('+').map(str::trim) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "cmd" | "command" => cmd = true,
                "shift" => shift = true,
                _ if key.is_none() => key = Key::from_name(part),
                _ => return None,
            }
        }
        key.map(|key| Self { key, cmd, shift })
    }

    /// Pressed this frame, with exactly these modifiers.
    pub fn pressed(self, ctx: &Context) -> bool {
        ctx.input(|i| {
            i.modifiers.command == self.cmd
                && i.modifiers.shift == self.shift
                && i.key_pressed(self.key)
        })
    }
}

impl Action {
    pub const ALL: [Action; 18] = [
        Action::Import,
        Action::OpenProject,
        Action::SaveProject,
        Action::SaveProjectAs,
        Action::NewProject,
        Action::ExportGcode,
        Action::Slice,
        Action::PrintSettings,
        Action::Undo,
        Action::Redo,
        Action::SelectAll,
        Action::Duplicate,
        Action::Remove,
        Action::SelectTool,
        Action::MoveTool,
        Action::RotateTool,
        Action::ScaleTool,
        Action::FitView,
    ];

    /// The key in the preferences file.
    pub fn name(self) -> &'static str {
        match self {
            Action::Import => "import",
            Action::OpenProject => "open_project",
            Action::SaveProject => "save_project",
            Action::SaveProjectAs => "save_project_as",
            Action::NewProject => "new_project",
            Action::ExportGcode => "export_gcode",
            Action::Slice => "slice",
            Action::PrintSettings => "print_settings",
            Action::Undo => "undo",
            Action::Redo => "redo",
            Action::SelectAll => "select_all",
            Action::Duplicate => "duplicate",
            Action::Remove => "remove",
            Action::SelectTool => "select_tool",
            Action::MoveTool => "move_tool",
            Action::RotateTool => "rotate_tool",
            Action::ScaleTool => "scale_tool",
            Action::FitView => "fit_view",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::Import => "Import models",
            Action::OpenProject => "Open project",
            Action::SaveProject => "Save project",
            Action::SaveProjectAs => "Save project as",
            Action::NewProject => "New project",
            Action::ExportGcode => "Export G-code",
            Action::Slice => "Slice",
            Action::PrintSettings => "Print settings",
            Action::Undo => "Undo",
            Action::Redo => "Redo",
            Action::SelectAll => "Select all",
            Action::Duplicate => "Duplicate",
            Action::Remove => "Remove the selection",
            Action::SelectTool => "Select tool",
            Action::MoveTool => "Move tool",
            Action::RotateTool => "Rotate tool",
            Action::ScaleTool => "Scale tool",
            Action::FitView => "Fit the models in view",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.name() == s)
    }

    pub fn default_binding(self) -> Binding {
        match self {
            Action::Import => Binding::new(Key::O, true, false),
            Action::OpenProject => Binding::new(Key::O, true, true),
            Action::SaveProject => Binding::new(Key::S, true, false),
            Action::SaveProjectAs => Binding::new(Key::S, true, true),
            Action::NewProject => Binding::new(Key::N, true, false),
            Action::ExportGcode => Binding::new(Key::E, true, false),
            Action::Slice => Binding::new(Key::R, true, false),
            Action::PrintSettings => Binding::new(Key::P, true, false),
            Action::Undo => Binding::new(Key::Z, true, false),
            Action::Redo => Binding::new(Key::Y, true, false),
            Action::SelectAll => Binding::new(Key::A, true, false),
            Action::Duplicate => Binding::new(Key::D, true, false),
            Action::Remove => Binding::new(Key::Delete, false, false),
            Action::SelectTool => Binding::new(Key::Q, false, false),
            Action::MoveTool => Binding::new(Key::W, false, false),
            Action::RotateTool => Binding::new(Key::E, false, false),
            Action::ScaleTool => Binding::new(Key::R, false, false),
            Action::FitView => Binding::new(Key::F, false, false),
        }
    }

    /// Works while a text field has focus or a window is open.
    pub fn global(self) -> bool {
        matches!(
            self,
            Action::Import
                | Action::OpenProject
                | Action::SaveProject
                | Action::SaveProjectAs
                | Action::NewProject
                | Action::ExportGcode
                | Action::Slice
                | Action::PrintSettings
        )
    }
}

/// The user's changes to the default bindings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shortcuts {
    pub changed: Vec<(Action, Binding)>,
}

impl Shortcuts {
    pub fn get(&self, a: Action) -> Binding {
        self.changed.iter().find(|(x, _)| *x == a).map_or_else(|| a.default_binding(), |(_, b)| *b)
    }

    pub fn text(&self, a: Action) -> String {
        self.get(a).text()
    }

    pub fn set(&mut self, a: Action, b: Binding) {
        self.changed.retain(|(x, _)| *x != a);
        if b != a.default_binding() {
            self.changed.push((a, b));
        }
    }

    /// Other actions already on this binding.
    pub fn clashes(&self, a: Action, b: Binding) -> Vec<Action> {
        Action::ALL.into_iter().filter(|x| *x != a && self.get(*x) == b).collect()
    }

    /// As preference lines: `action=Ctrl+O`.
    pub fn to_lines(&self) -> Vec<String> {
        self.changed.iter().map(|(a, b)| format!("{}={}", a.name(), b.text())).collect()
    }

    /// Read one preference line; false when it does not read.
    pub fn read_line(&mut self, line: &str) -> bool {
        let Some((a, b)) = line.split_once('=') else { return false };
        match (Action::from_name(a.trim()), Binding::parse(b)) {
            (Some(a), Some(b)) => {
                self.set(a, b);
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_round_trip_as_text() {
        for a in Action::ALL {
            let b = a.default_binding();
            assert_eq!(Binding::parse(&b.text()), Some(b), "{}", b.text());
        }
        assert!(Binding::parse("Ctrl+Nope").is_none());
        assert!(Binding::parse("Ctrl+A+B").is_none());
    }

    #[test]
    fn changes_are_kept_and_clashes_found() {
        let mut s = Shortcuts::default();
        let f5 = Binding::new(Key::F5, false, false);
        s.set(Action::Slice, f5);
        assert_eq!(s.get(Action::Slice), f5);
        assert_eq!(s.to_lines(), vec!["slice=F5".to_string()]);
        let mut t = Shortcuts::default();
        assert!(t.read_line("slice=F5"));
        assert!(!t.read_line("slice=Ctrl+Nope"));
        assert_eq!(t, s);
        // back to the default: nothing to keep
        s.set(Action::Slice, Action::Slice.default_binding());
        assert!(s.changed.is_empty());
        assert_eq!(s.clashes(Action::Slice, Binding::new(Key::Z, true, false)), vec![Action::Undo]);
        let mut seen = std::collections::HashSet::new();
        for a in Action::ALL {
            assert!(
                seen.insert(a.default_binding().text()),
                "two defaults on {}",
                a.default_binding().text()
            );
        }
    }
}
