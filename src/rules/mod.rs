//! The rules graph (docs/rules-graph.md in the log). Felix's rules are an
//! append-only log of entries under graph/entries/; code folds them into one
//! root per subject; a model writes facts and clashes from their parents
//! only, each answer kept under a fingerprint of its inputs; code compiles
//! the graph word for word into knowledge/<project>/, the files layouts
//! name, and graph/index.json. Nothing here runs on a clock: the session
//! that adds an entry builds, and `abot land` checks.

pub mod compile;
pub mod entry;
pub mod graph;
pub mod source;

use crate::git::Git;
use anyhow::Result;
use graph::{Graph, Model};
use source::Source;
use std::collections::BTreeSet;

pub struct Built {
    pub graph: Graph,
    pub report: graph::Report,
    /// Compiled files whose text changed (written), and ones removed.
    pub rewrote: Vec<String>,
    pub removed: Vec<String>,
    pub state_files: Vec<String>,
}

/// Build in a directory: work out stale facts (asking the model when one is
/// given), write what changed, compile, and list open clashes under each
/// project's Waits on Felix.
pub fn build(dir: &std::path::Path, model: Option<&mut dyn Model>, today: &str, where_: &str) -> Result<Built> {
    let src = Source::Dir(dir.to_path_buf());
    let mut g = graph::load(&src, None)?;
    let report = graph::resolve(&mut g, model, today, where_)?;
    for (p, text) in &report.writes {
        src.write(p, text)?;
    }
    if !report.stale.is_empty() {
        anyhow::bail!(
            "{} fact(s) have no stored answer for their inputs and no model was allowed: {}",
            report.stale.len(),
            report.stale.join(", ")
        );
    }
    let layouts = compile::load_layouts(&src)?;
    let out = compile::compile(&g, &layouts, &state_projects(&src))?;
    let mut rewrote = vec![];
    for (p, text) in &out.files {
        if src.read(p).as_deref() != Some(text.as_str()) {
            src.write(p, text)?;
            rewrote.push(p.clone());
        }
    }
    // Compiled files nothing produces any more (a fact that is gone).
    let mut removed = vec![];
    for p in src.list("knowledge") {
        if out.files.contains_key(&p) {
            continue;
        }
        if src.read(&p).map(|t| t.starts_with(compile::STAMP)).unwrap_or(false) {
            src.remove(&p)?;
            removed.push(p);
        }
    }
    let state_files = waits_on_felix(&src, &g)?;
    Ok(Built { graph: g, report, rewrote, removed, state_files })
}

/// The projects that have a state file: each gets a knowledge/ folder.
pub fn state_projects(src: &Source) -> Vec<String> {
    src.list("state/projects")
        .iter()
        .filter_map(|p| p.strip_prefix("state/projects/")?.strip_suffix(".md").map(String::from))
        .filter(|p| !p.contains('/'))
        .collect()
}

/// Put each project's open clashes under its Waits on Felix, replacing the
/// lines an earlier build wrote there (they start `- Rules graph clash/`).
fn waits_on_felix(src: &Source, g: &Graph) -> Result<Vec<String>> {
    let mut projects: BTreeSet<String> = g.clashes.values().map(|c| c.project.clone()).collect();
    // Projects that had a line before and have none now.
    for p in src.list("state/projects") {
        if let Some(text) = src.read(&p) {
            if text.contains("- Rules graph clash/") {
                if let Some(slug) = p.strip_prefix("state/projects/").and_then(|s| s.strip_suffix(".md")) {
                    projects.insert(slug.to_string());
                }
            }
        }
    }
    let mut changed = vec![];
    for p in projects {
        let path = format!("state/projects/{p}.md");
        let Some(text) = src.read(&path) else { continue };
        let lines: Vec<String> = g
            .clashes
            .values()
            .filter(|c| c.project == p && c.status == "open")
            .map(compile::clash_line)
            .collect();
        let new = with_waits(&text, &lines);
        if new != text {
            src.write(&path, &new)?;
            changed.push(path);
        }
    }
    Ok(changed)
}

pub fn with_waits(text: &str, lines: &[String]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut in_waits = false;
    let mut placed = false;
    for line in text.lines() {
        if line.starts_with("## ") {
            if in_waits && !placed {
                out.extend(lines.iter().cloned());
                placed = true;
            }
            in_waits = line.trim() == "## Waits on Felix";
            out.push(line.to_string());
            if in_waits {
                out.extend(lines.iter().cloned());
                placed = true;
            }
            continue;
        }
        if line.starts_with("- Rules graph clash/") {
            continue;
        }
        out.push(line.to_string());
    }
    let mut s = out.join("\n");
    if text.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// Everything `abot land` and `abot check` refuse, read from `src`.
pub fn check(src: &Source) -> Vec<String> {
    let mut p = Vec::new();
    if src.list("graph").is_empty() {
        return p;
    }
    let mut g = match graph::load(src, None) {
        Ok(g) => g,
        Err(e) => return vec![format!("rules graph: {e:#}")],
    };
    match graph::resolve(&mut g, None, "", "") {
        Ok(r) => {
            for id in r.stale {
                p.push(format!(
                    "rules graph: fact {id} has no stored answer for its inputs now (fingerprint {}); run `abot rules build`",
                    g.facts[&id].fingerprint
                ));
            }
        }
        Err(e) => p.push(format!("rules graph: {e:#}")),
    }
    for c in g.clashes.values() {
        for side in [&c.a, &c.b, &c.raised_by] {
            if g.kind(side).is_none() {
                p.push(format!("rules graph: {} names {side}, which is not a node", c.id));
            }
        }
    }
    for e in &g.entries {
        if let Some(c) = &e.settles {
            if !g.clashes.contains_key(c) {
                p.push(format!("rules graph: entry {} settles {c}, which is not a clash", e.id));
            }
        }
        if let Some(c) = &e.corrects {
            if g.kind(c).is_none() {
                p.push(format!("rules graph: entry {} corrects {c}, which is not a node", e.id));
            }
        }
    }
    let layouts = match compile::load_layouts(src) {
        Ok(l) => l,
        Err(e) => {
            p.push(format!("rules graph: {e:#}"));
            return p;
        }
    };
    for l in &layouts {
        p.extend(compile::prose_problems(l));
    }
    if !p.is_empty() {
        return p;
    }
    let out = match compile::compile(&g, &layouts, &state_projects(src)) {
        Ok(o) => o,
        Err(e) => return vec![format!("rules graph: {e:#}")],
    };
    for (path, text) in &out.files {
        match src.read(path) {
            None => p.push(format!("{path}: missing; run `abot rules build`")),
            Some(t) if &t != text => p.push(format!(
                "{path}: differs from a fresh compile of the graph (a hand edit, or a build not run); change the graph and run `abot rules build`"
            )),
            _ => {}
        }
    }
    for f in src.list("knowledge") {
        if !out.files.contains_key(&f) {
            p.push(format!("{f}: is not compiled from the graph; a file agents need is added by writing its layout (graph/layouts/{f})"));
        }
    }
    for c in g.clashes.values().filter(|c| c.status == "open") {
        let path = format!("state/projects/{}.md", c.project);
        let line = compile::clash_line(c);
        if let Some(text) = src.read(&path) {
            if !text.contains(&line) {
                p.push(format!("{path}: Waits on Felix doesn't list {}; run `abot rules build`", c.id));
            }
        }
    }
    p
}

/// An entry is written once: between `from` and `to`, files under
/// graph/entries/ may only be added.
pub fn entries_only_added(g: &Git, from: &str, to: &str) -> Vec<String> {
    let out = g
        .try_run(&["diff", "--name-status", "--no-renames", from, to, "--", "graph/entries"])
        .unwrap_or_default();
    out.lines()
        .filter(|l| !l.starts_with('A'))
        .map(|l| format!("{}: an entry is written once and never edited or removed; add a new entry that replaces it", l.split_whitespace().last().unwrap_or(l)))
        .collect()
}

/// The same over all of history up to `rev`: any commit that changed or
/// removed an entry.
pub fn entries_never_changed(g: &Git, rev: &str) -> Vec<String> {
    let out = g
        .try_run(&["log", "--format=commit %h", "--name-status", "--no-renames", "--diff-filter=MDRT", rev, "--", "graph/entries"])
        .unwrap_or_default();
    let mut sha = String::new();
    let mut v = vec![];
    for l in out.lines() {
        if let Some(s) = l.strip_prefix("commit ") {
            sha = s.to_string();
        } else if !l.trim().is_empty() {
            v.push(format!("{}: changed after it landed, in {sha}; entries are written once", l.split_whitespace().last().unwrap_or(l)));
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_lines_replace_old_ones() {
        let t = "# p\n\n## Waits on Felix\n- old ask\n- Rules graph clash/x: q (between a and b; asked 2026-11-04)\n\n## Traps\n- t\n";
        let s = with_waits(t, &["- Rules graph clash/y: q2 (between c and d; asked 2026-11-05)".into()]);
        assert_eq!(
            s,
            "# p\n\n## Waits on Felix\n- Rules graph clash/y: q2 (between c and d; asked 2026-11-05)\n- old ask\n\n## Traps\n- t\n"
        );
        assert_eq!(with_waits(&s, &[]), "# p\n\n## Waits on Felix\n- old ask\n\n## Traps\n- t\n");
    }
}

#[cfg(test)]
mod bench {
    /// `ABOT_BENCH_LOG=<log dir> cargo test --release time_replay -- --ignored --nocapture`:
    /// the replay the engine was chosen on (docs/rules-graph.md).
    #[test]
    #[ignore]
    fn time_replay() {
        let dir = std::path::PathBuf::from(std::env::var("ABOT_BENCH_LOG").expect("ABOT_BENCH_LOG"));
        let src = super::Source::Dir(dir);
        let n = 200;
        let t = std::time::Instant::now();
        let mut roots = 0;
        let mut entries = 0;
        for _ in 0..n {
            let g = super::graph::load(&src, None).unwrap();
            roots = g.roots.len();
            entries = g.entries.len();
        }
        let load = t.elapsed() / n;
        let mut g = super::graph::load(&src, None).unwrap();
        let t = std::time::Instant::now();
        super::graph::resolve(&mut g, None, "", "").unwrap();
        let resolve = t.elapsed();
        let t = std::time::Instant::now();
        let p = super::check(&src);
        let check = t.elapsed();
        println!("abot fold: {entries} entries read and folded to {roots} roots: {load:?} per run; facts from stored answers: {resolve:?}; full land check: {check:?} ({} problems)", p.len());
    }
}
