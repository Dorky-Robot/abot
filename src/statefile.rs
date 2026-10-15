//! A project's state file: `# <project>`, `Uses:` and `Serves:` lines, and
//! six `##` sections. About a screen; detail lives elsewhere.

pub const HEADINGS: [&str; 6] = [
    "What it is",
    "Live now",
    "Next",
    "Waits on Felix",
    "Traps",
    "Parked",
];

pub const MAX_LINES: usize = 60;

/// The `## ` headings that are present, in order.
pub fn headings(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.strip_prefix("## "))
        .map(|h| h.trim().to_string())
        .collect()
}

pub fn missing_headings(text: &str) -> Vec<&'static str> {
    let have = headings(text);
    HEADINGS
        .iter()
        .copied()
        .filter(|h| !have.iter().any(|x| x.eq_ignore_ascii_case(h)))
        .collect()
}

/// The non-blank body lines of a section, or None if the heading is absent.
pub fn section(text: &str, name: &str) -> Option<Vec<String>> {
    let mut inside = false;
    let mut found = false;
    let mut out = vec![];
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            inside = h.trim().eq_ignore_ascii_case(name);
            found |= inside;
            continue;
        }
        if line.starts_with("# ") {
            inside = false;
            continue;
        }
        if inside && !line.trim().is_empty() {
            out.push(line.trim_end().to_string());
        }
    }
    found.then_some(out)
}

/// A header field such as `Uses:` or `Serves:`, from above the first section.
pub fn field(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        if line.starts_with("## ") {
            break;
        }
        if let Some((k, v)) = line.split_once(':')
            && k.trim().eq_ignore_ascii_case(key)
        {
            let v = v.trim();
            return (!v.is_empty()).then(|| v.to_string());
        }
    }
    None
}

/// A comma-separated header field as a list.
pub fn list_field(text: &str, key: &str) -> Vec<String> {
    field(text, key)
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// A body line without its bullet or number.
pub fn strip_bullet(line: &str) -> String {
    let t = line.trim();
    for p in ["- ", "* ", "+ "] {
        if let Some(r) = t.strip_prefix(p) {
            return r.trim().to_string();
        }
    }
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let rest = &t[digits..];
        if let Some(r) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return r.trim().to_string();
        }
    }
    t.to_string()
}

/// Lines that mean "nothing here" rather than an item.
pub fn is_placeholder(line: &str) -> bool {
    let s = strip_bullet(line).to_ascii_lowercase();
    let s = s.trim_matches(|c: char| c == '(' || c == ')' || c == '.' || c == '_' || c == '*');
    matches!(s, "" | "none" | "nothing" | "n/a" | "-" | "none yet" | "nothing yet")
}

/// The items of a section, placeholders and bullets removed.
pub fn items(text: &str, name: &str) -> Vec<String> {
    section(text, name)
        .unwrap_or_default()
        .into_iter()
        .filter(|l| !is_placeholder(l))
        .map(|l| strip_bullet(&l))
        .collect()
}

/// Insert `line` as the first item under `## <name>` (newest first). If the
/// heading is missing it is added where the fixed shape puts it. Placeholder
/// lines such as "(none)" under the heading are dropped.
pub fn add_line(text: &str, name: &str, line: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 3);
    let pos = lines
        .iter()
        .position(|l| l.strip_prefix("## ").is_some_and(|h| h.trim().eq_ignore_ascii_case(name)));
    match pos {
        Some(i) => {
            let mut j = i + 1;
            out.extend(lines[..=i].iter().map(|s| s.to_string()));
            out.push(line.to_string());
            // Drop placeholders directly under the heading.
            while j < lines.len()
                && !lines[j].trim().is_empty()
                && !lines[j].starts_with("## ")
                && is_placeholder(lines[j])
            {
                j += 1;
            }
            out.extend(lines[j..].iter().map(|s| s.to_string()));
        }
        None => {
            // Place it before the first heading that comes after it in HEADINGS.
            let rank = HEADINGS.iter().position(|h| h.eq_ignore_ascii_case(name));
            let later: Vec<&str> = match rank {
                Some(r) => HEADINGS[r + 1..].to_vec(),
                None => vec![],
            };
            let before = lines.iter().position(|l| {
                l.strip_prefix("## ")
                    .is_some_and(|h| later.iter().any(|x| x.eq_ignore_ascii_case(h.trim())))
            });
            let block = [format!("## {name}"), line.to_string(), String::new()];
            match before {
                Some(b) => {
                    out.extend(lines[..b].iter().map(|s| s.to_string()));
                    out.extend(block);
                    out.extend(lines[b..].iter().map(|s| s.to_string()));
                }
                None => {
                    out.extend(lines.iter().map(|s| s.to_string()));
                    while out.last().is_some_and(|l| l.trim().is_empty()) {
                        out.pop();
                    }
                    if !out.is_empty() {
                        out.push(String::new());
                    }
                    out.extend(block[..2].iter().cloned());
                }
            }
        }
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// The built-in template, used by `start --new` when the log has no
/// `how/project-template.md`.
pub fn template(project: &str) -> String {
    format!(
        "# {project}\n\
         Uses: \n\
         Serves: \n\
         \n\
         ## What it is\n\
         (two lines: what it is and who it is for)\n\
         \n\
         ## Live now\n\
         (where it runs, what version, what machine)\n\
         \n\
         ## Next\n\
         (ranked, three lines at most)\n\
         \n\
         ## Waits on Felix\n\
         \n\
         ## Traps\n\
         \n\
         ## Parked\n"
    )
}

fn indent(l: &str) -> Option<&str> {
    l.strip_prefix("    ").or_else(|| l.strip_prefix('\t'))
}

/// The log's `how/project-template.md` is prose around an indented example
/// block. The block is the template: the first indented run that starts
/// with `# <`. A file that is itself a bare template (starts `# <`) is used
/// whole.
pub fn extract_template(doc: &str) -> Option<String> {
    if doc.starts_with("# <") || doc.starts_with("# {") {
        return Some(doc.to_string());
    }
    let lines: Vec<&str> = doc.lines().collect();
    let start = lines.iter().position(|l| indent(l).is_some_and(|x| x.starts_with("# <") || x.starts_with("# {")))?;
    let mut out = vec![];
    for (i, l) in lines.iter().enumerate().skip(start) {
        match indent(l) {
            Some(x) => out.push(x.to_string()),
            // A blank line inside the block, when the block goes on after it.
            None if l.trim().is_empty() && lines.get(i + 1).is_some_and(|n| indent(n).is_some()) => out.push(String::new()),
            None => break,
        }
    }
    Some(out.join("\n"))
}

/// Fill a template for a project: `<project>`, `<slug>` and `{project}`
/// become the slug, and the example values on `Uses:` and `Serves:` are
/// cleared, since an example is not a fact about this project. Each `##`
/// heading gets a blank line before it so the file reads as sections.
pub fn fill_template(tpl: &str, project: &str) -> String {
    let filled = tpl
        .replace("<project>", project)
        .replace("<slug>", project)
        .replace("{project}", project);
    let mut out: Vec<String> = vec![];
    for line in filled.lines() {
        let key = line.split_once(':').map(|(k, _)| k.trim());
        if key.is_some_and(|k| k.eq_ignore_ascii_case("Uses") || k.eq_ignore_ascii_case("Serves")) && !line.starts_with('#') {
            out.push(format!("{}: ", key.unwrap()));
            continue;
        }
        if line.starts_with("## ") && out.last().is_some_and(|l| !l.trim().is_empty()) {
            out.push(String::new());
        }
        out.push(line.to_string());
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// Problems with a state file's shape: missing headings, too long.
pub fn lint(text: &str) -> Vec<String> {
    let mut out = vec![];
    let missing = missing_headings(text);
    if !missing.is_empty() {
        out.push(format!("missing headings: {}", missing.join(", ")));
    }
    for key in ["Uses", "Serves"] {
        if !text.lines().take_while(|l| !l.starts_with("## ")).any(|l| {
            l.split_once(':').is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case(key))
        }) {
            out.push(format!("no `{key}:` line"));
        }
    }
    let n = text.lines().count();
    if n > MAX_LINES {
        out.push(format!("{n} lines, over {MAX_LINES}: move detail to the code repo's docs/HANDOFF.md"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# demo\nUses: pocket-id, kita\nServes: Sara\n\n## What it is\nA demo.\n\n## Live now\n- dr1, v3\n\n## Next\n1. ship the thing\n2. then this\n\n## Waits on Felix\n- pick a name\n- (none)\n\n## Traps\n- 2026-10-01 x: don't\n\n## Parked\n(none)\n";

    #[test]
    fn complete_file_lints_clean() {
        assert!(lint(SAMPLE).is_empty(), "{:?}", lint(SAMPLE));
        assert!(missing_headings(SAMPLE).is_empty());
    }

    #[test]
    fn lint_finds_missing_and_long() {
        let t = "# x\n## Next\n";
        let l = lint(t);
        assert!(l[0].contains("What it is") && l[0].contains("Parked"));
        assert!(l.iter().any(|x| x.contains("Uses")));
        let long = format!("{SAMPLE}{}", "x\n".repeat(60));
        assert!(lint(&long).iter().any(|x| x.contains("over 60")));
    }

    #[test]
    fn sections_fields_items() {
        assert_eq!(section(SAMPLE, "live now").unwrap(), vec!["- dr1, v3"]);
        assert_eq!(items(SAMPLE, "Next"), vec!["ship the thing", "then this"]);
        assert_eq!(items(SAMPLE, "Waits on Felix"), vec!["pick a name"]);
        assert!(items(SAMPLE, "Parked").is_empty());
        assert!(section(SAMPLE, "Nope").is_none());
        assert_eq!(list_field(SAMPLE, "Uses"), vec!["pocket-id", "kita"]);
        assert_eq!(field(SAMPLE, "Serves").as_deref(), Some("Sara"));
        assert_eq!(field(SAMPLE, "Repo"), None);
    }

    #[test]
    fn add_line_goes_first_and_drops_placeholder() {
        let t = add_line(SAMPLE, "Traps", "- 2026-10-15 y: do z");
        let traps = section(&t, "Traps").unwrap();
        assert_eq!(traps, vec!["- 2026-10-15 y: do z", "- 2026-10-01 x: don't"]);
        let p = add_line(SAMPLE, "Parked", "- 2026-10-15 demo/q: waits");
        assert_eq!(section(&p, "Parked").unwrap(), vec!["- 2026-10-15 demo/q: waits"]);
        assert!(p.ends_with('\n'));
    }

    #[test]
    fn add_line_creates_missing_heading_in_place() {
        let t = "# org\nsome text\n\n## Parked\n- a\n";
        let out = add_line(t, "Traps", "- new");
        let pos_t = out.find("## Traps").unwrap();
        let pos_p = out.find("## Parked").unwrap();
        assert!(pos_t < pos_p, "{out}");
        assert_eq!(section(&out, "Traps").unwrap(), vec!["- new"]);
        let bare = add_line("d-1. something\n", "Traps", "- new");
        assert_eq!(bare, "d-1. something\n\n## Traps\n- new\n");
    }

    #[test]
    fn template_has_the_shape() {
        let t = template("demo");
        assert!(missing_headings(&t).is_empty());
        assert!(lint(&t).is_empty(), "{:?}", lint(&t));
        assert_eq!(fill_template("# <project>\nUses: x", "demo"), "# demo\nUses: \n");
        let doc = "# Project state file: the template\n\nCopy the block below.\n\n    # <slug>\n    Uses: pocket-id, kita\n    Serves: Sara\n    ## What it is\n    ## Live now\n    ## Next\n    ## Waits on Felix\n    ## Traps\n    ## Parked\n\n## The two lines\n\nprose\n";
        let t = fill_template(&extract_template(doc).unwrap(), "demo");
        assert!(t.starts_with("# demo\nUses: \nServes: \n\n## What it is\n\n## Live now\n"), "{t}");
        assert!(missing_headings(&t).is_empty() && !t.contains("prose") && !t.contains("pocket-id"));
        assert!(extract_template("no block here").is_none());
    }

    #[test]
    fn bullets_strip() {
        assert_eq!(strip_bullet("  - a b"), "a b");
        assert_eq!(strip_bullet("12. thing"), "thing");
        assert_eq!(strip_bullet("2026-10-15 x"), "2026-10-15 x");
        assert!(is_placeholder("(none)"));
        assert!(is_placeholder("- nothing"));
        assert!(!is_placeholder("- something"));
    }
}
