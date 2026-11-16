//! Post-processing of the written G-code: the profile's post-processing text,
//! one step per line, in order.
//!
//! Built-in text edits (in braces) change the G-code before it is written;
//! they run no program, so they apply however the export was started:
//!
//! - `{REPLACE "find" "with"}` every occurrence of `find` becomes `with`
//! - `{TOOLnREPLACE "find" "with"}` the same, only in lines printed by tool n
//! - `{DELETE "text"}` every occurrence of `text` is removed
//! - `{STRIP "text"}` every line that starts with `text` is removed
//! - `{PREPEND "text"}` / `{APPEND "text"}` text added at the start / end
//!
//! Inside the quotes `\n` is a line break, `\t` a tab, `\"` a quote and `\\` a
//! backslash. These are the commands Simplify3D profiles use, so their
//! post-processing lines carry over. Any other line is a program to run on the
//! written file, with `[output_filepath]` or `{file}` standing for its path;
//! running programs is the caller's decision (see `engine::Session`).

use std::fmt;

/// Most steps one post-processing text may hold.
pub const MAX_STEPS: usize = 1000;
/// Longest quoted string a step may hold (bytes).
pub const MAX_TEXT: usize = 64 * 1024;
/// The edited file may grow to this many times its size, plus [`GROWTH_SLACK`].
const GROWTH_FACTOR: usize = 4;
const GROWTH_SLACK: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Replace {
        find: String,
        with: String,
        tool: Option<u32>,
    },
    Delete(String),
    Strip(String),
    Prepend(String),
    Append(String),
    /// A program and its arguments, as written.
    Program(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostError {
    pub line: usize,
    pub what: String,
}

impl fmt::Display for PostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "post-processing line {}: {}", self.line, self.what)
    }
}

impl std::error::Error for PostError {}

/// What applying the text edits did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Applied {
    pub gcode: String,
    /// Programs to run on the written file, in order.
    pub programs: Vec<String>,
    /// Steps that changed nothing ("REPLACE found no …"), worth a note: a
    /// script written for another slicer's comments may not match ours.
    pub unmatched: Vec<String>,
}

/// Read the quoted strings of a built-in command's arguments.
fn quoted(args: &str, line: usize) -> Result<Vec<String>, PostError> {
    let err = |what: &str| PostError { line, what: what.to_string() };
    let mut out = Vec::new();
    let mut chars = args.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        match chars.next() {
            None => break,
            Some('"') => {}
            Some(_) => return Err(err("expected a quoted string")),
        }
        let mut s = String::new();
        loop {
            match chars.next() {
                None => return Err(err("a quoted string is not closed")),
                Some('"') => break,
                Some('\\') => match chars.next() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('r') => s.push('\r'),
                    Some('"') => s.push('"'),
                    Some('\\') => s.push('\\'),
                    Some(c) => {
                        s.push('\\');
                        s.push(c);
                    }
                    None => return Err(err("a quoted string is not closed")),
                },
                Some(c) => s.push(c),
            }
            if s.len() > MAX_TEXT {
                return Err(err("a quoted string is too long"));
            }
        }
        out.push(s);
    }
    Ok(out)
}

/// Parse the post-processing text into steps.
pub fn parse(text: &str) -> Result<Vec<Step>, PostError> {
    let mut steps = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        let t = raw.trim();
        if t.is_empty() {
            continue;
        }
        if steps.len() >= MAX_STEPS {
            return Err(PostError { line, what: format!("more than {MAX_STEPS} steps") });
        }
        let Some(inner) = t.strip_prefix('{') else {
            steps.push(Step::Program(t.to_string()));
            continue;
        };
        let Some(inner) = inner.strip_suffix('}') else {
            return Err(PostError { line, what: "a command in braces has no closing }".into() });
        };
        let inner = inner.trim_start();
        let name_len = inner.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(inner.len());
        let (name, rest) = inner.split_at(name_len);
        let name = name.to_ascii_uppercase();
        let args = quoted(rest, line)?;
        let want = |n: usize| -> Result<(), PostError> {
            if args.len() == n {
                Ok(())
            } else {
                Err(PostError {
                    line,
                    what: format!("{name} takes {n} quoted string(s), found {}", args.len()),
                })
            }
        };
        let first = || args.first().cloned().unwrap_or_default();
        let step = match name.as_str() {
            "REPLACE" => {
                want(2)?;
                Step::Replace {
                    find: first(),
                    with: args.get(1).cloned().unwrap_or_default(),
                    tool: None,
                }
            }
            "DELETE" => {
                want(1)?;
                Step::Delete(first())
            }
            "STRIP" => {
                want(1)?;
                Step::Strip(first())
            }
            "PREPEND" => {
                want(1)?;
                Step::Prepend(first())
            }
            "APPEND" => {
                want(1)?;
                Step::Append(first())
            }
            other => match other.strip_prefix("TOOL").and_then(|r| r.strip_suffix("REPLACE")) {
                Some(n) if !n.is_empty() => {
                    let tool = n.parse::<u32>().map_err(|_| PostError {
                        line,
                        what: format!("{other}: the tool number is not a number"),
                    })?;
                    want(2)?;
                    Step::Replace {
                        find: first(),
                        with: args.get(1).cloned().unwrap_or_default(),
                        tool: Some(tool),
                    }
                }
                _ => {
                    return Err(PostError {
                        line, what: format!("unknown command {{{other} …}}")
                    });
                }
            },
        };
        if let Step::Replace { find, .. } | Step::Delete(find) | Step::Strip(find) = &step
            && find.is_empty()
        {
            return Err(PostError { line, what: format!("{name} needs text to look for") });
        }
        steps.push(step);
    }
    Ok(steps)
}

/// The tool a G-code line selects (`T1`, `T1 ; comment`), if it is a tool change.
fn tool_change(line: &str) -> Option<u32> {
    let code = line.split(';').next().unwrap_or("").trim();
    code.strip_prefix('T').and_then(|n| n.trim().parse().ok())
}

/// Apply the text edits of `text` to `gcode`. Programs are returned, not run.
pub fn apply(gcode: &str, text: &str) -> Result<Applied, PostError> {
    let steps = parse(text)?;
    let limit = gcode.len().saturating_mul(GROWTH_FACTOR).saturating_add(GROWTH_SLACK);
    let mut out = Applied { gcode: gcode.to_string(), ..Applied::default() };
    for (k, step) in steps.iter().enumerate() {
        let before = out.gcode.len();
        let mut matched = true;
        match step {
            Step::Replace { find, with, tool: None } => {
                matched = out.gcode.contains(find.as_str());
                if matched {
                    let count = out.gcode.matches(find.as_str()).count();
                    let grown = before
                        .saturating_add(count.saturating_mul(with.len()))
                        .saturating_sub(count.saturating_mul(find.len()));
                    if grown > limit {
                        return Err(grows(k));
                    }
                    out.gcode = out.gcode.replace(find.as_str(), with);
                }
            }
            Step::Replace { find, with, tool: Some(t) } => {
                let mut tool = 0u32;
                let mut result = String::with_capacity(before);
                matched = false;
                for l in out.gcode.split_inclusive('\n') {
                    if let Some(n) = tool_change(l) {
                        tool = n;
                    }
                    if tool == *t && l.contains(find.as_str()) {
                        matched = true;
                        result.push_str(&l.replace(find.as_str(), with));
                    } else {
                        result.push_str(l);
                    }
                    if result.len() > limit {
                        return Err(grows(k));
                    }
                }
                out.gcode = result;
            }
            Step::Delete(find) => {
                matched = out.gcode.contains(find.as_str());
                out.gcode = out.gcode.replace(find.as_str(), "");
            }
            Step::Strip(start) => {
                let kept: String = out
                    .gcode
                    .split_inclusive('\n')
                    .filter(|l| !l.starts_with(start.as_str()))
                    .collect();
                matched = kept.len() != before;
                out.gcode = kept;
            }
            Step::Prepend(s) => {
                if before.saturating_add(s.len()) > limit {
                    return Err(grows(k));
                }
                out.gcode.insert_str(0, s);
            }
            Step::Append(s) => {
                if before.saturating_add(s.len()) > limit {
                    return Err(grows(k));
                }
                out.gcode.push_str(s);
            }
            Step::Program(p) => out.programs.push(p.clone()),
        }
        if !matched {
            out.unmatched.push(describe(step));
        }
    }
    Ok(out)
}

fn grows(step: usize) -> PostError {
    PostError { line: step + 1, what: "the edit makes the file far larger than the G-code".into() }
}

fn describe(step: &Step) -> String {
    let short = |s: &str| {
        let one = s.replace('\n', "\\n");
        if one.chars().count() > 40 {
            format!("{}…", one.chars().take(40).collect::<String>())
        } else {
            one
        }
    };
    match step {
        Step::Replace { find, tool: None, .. } => format!("REPLACE found no \"{}\"", short(find)),
        Step::Replace { find, tool: Some(t), .. } => {
            format!("TOOL{t}REPLACE found no \"{}\"", short(find))
        }
        Step::Delete(f) => format!("DELETE found no \"{}\"", short(f)),
        Step::Strip(f) => format!("STRIP found no line starting with \"{}\"", short(f)),
        Step::Prepend(_) | Step::Append(_) | Step::Program(_) => String::new(),
    }
}

/// A program line split into the program and its arguments, honouring double
/// quotes (so paths with spaces work), with `[output_filepath]` and `{file}`
/// replaced by `file`. The file is added last when no argument names it.
pub fn program_args(line: &str, file: &str) -> Option<(String, Vec<String>)> {
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut any = false;
    for c in line.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                any = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if any {
                    words.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => {
                cur.push(c);
                any = true;
            }
        }
    }
    if any {
        words.push(cur);
    }
    let named = words.iter().any(|w| w.contains("[output_filepath]") || w.contains("{file}"));
    let mut words: Vec<String> = words
        .into_iter()
        .map(|w| w.replace("[output_filepath]", file).replace("{file}", file))
        .collect();
    if !named {
        words.push(file.to_string());
    }
    let mut it = words.into_iter();
    let exe = it.next()?;
    Some((exe, it.collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: &str =
        "; header\nT0\n;LAYER:57 Z:11.400\nG1 X1 E1\nT1\nG1 X2 E2\n;LAYER:58 Z:11.600\nG1 X3 E3\n";

    /// The edits of `text` on `G`, which must apply.
    fn ok(gcode: &str, text: &str) -> Applied {
        let r = apply(gcode, text);
        assert!(r.is_ok(), "{r:?}");
        r.unwrap_or_default()
    }

    fn args(line: &str, file: &str) -> (String, Vec<String>) {
        let a = program_args(line, file);
        assert!(a.is_some());
        a.unwrap_or_default()
    }

    #[test]
    fn replace_changes_every_match_and_reads_escapes() {
        let r = ok(G, r#"{REPLACE ";LAYER:58 Z:11.600\n" ";LAYER:58 Z:11.600\nM221 S60\n"}"#);
        assert!(r.gcode.contains(";LAYER:58 Z:11.600\nM221 S60\nG1 X3"));
        assert!(r.unmatched.is_empty() && r.programs.is_empty());
    }

    #[test]
    fn several_steps_run_in_order() {
        let text = concat!(
            r#"{PREPEND "; first\n"}"#,
            "\n",
            r#"{STRIP ";LAYER"}"#,
            "\n",
            r#"{DELETE " E2"}"#,
            "\n",
            r#"{APPEND "M84\n"}"#,
            "\n"
        );
        let r = ok(G, text);
        assert!(r.gcode.starts_with("; first\n; header\n"));
        assert!(!r.gcode.contains(";LAYER"));
        assert!(r.gcode.contains("G1 X2\n"));
        assert!(r.gcode.ends_with("M84\n"));
    }

    #[test]
    fn tool_replace_touches_only_that_tool() {
        let r = ok(G, r#"{TOOL1REPLACE "E" "A"}"#);
        assert!(r.gcode.contains("G1 X1 E1"), "tool 0 unchanged");
        assert!(r.gcode.contains("G1 X2 A2") && r.gcode.contains("G1 X3 A3"));
    }

    #[test]
    fn a_step_that_matches_nothing_is_reported() {
        let r = ok(G, r#"{REPLACE "; layer 58," "x"}"#);
        assert_eq!(r.gcode, G);
        assert_eq!(r.unmatched, vec![r#"REPLACE found no "; layer 58,""#.to_string()]);
    }

    #[test]
    fn other_lines_are_programs_and_bad_commands_are_errors() {
        let r = ok(G, "python \"C:\\My Scripts\\fix.py\" [output_filepath]\n");
        assert_eq!(r.programs.len(), 1);
        let (exe, a) = args(r.programs.first().map_or("", String::as_str), "out.gcode");
        assert_eq!(exe, "python");
        assert_eq!(a, vec![r"C:\My Scripts\fix.py".to_string(), "out.gcode".to_string()]);
        let (_, a) = args("tool --fast", "f.gcode");
        assert_eq!(a, vec!["--fast".to_string(), "f.gcode".to_string()]);
        for bad in [
            r#"{REPLACE "a"}"#,
            r#"{REPLACE "a" "b""#,
            r#"{FROB "a"}"#,
            r#"{STRIP ""}"#,
            r#"{DELETE "open}"#,
        ] {
            assert!(apply(G, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_replace_that_would_explode_the_file_is_refused() {
        let big = format!("{{REPLACE \"G\" \"{}\"}}", "x".repeat(60_000));
        let gcode = "G1\n".repeat(10_000);
        assert!(apply(&gcode, &big).is_err());
    }
}
