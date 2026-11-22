//! The compile: every file agents read is a view of the one graph. A layout
//! (graph/layouts/<path>.md, written by the model) says what a file holds;
//! code puts each node's text in word for word through slots:
//!
//!     {{rule <node>}}      the node's rule
//!     {{said <root>}}      Felix's words the root's rule comes from, quoted and dated
//!     {{node <node>}}      the rule, then the words or proof it rests on
//!     {{clashes <project>}} the project's open clashes, one line each
//!     {{include <path>}}   a hand-written page of the log (how/, people/,
//!     {{include <path>#<heading>}}  ecosystem/, docs/…), or one section of it,
//!                          word for word: procedures are compiled files, not
//!                          entries (Felix, 11-22), so they reach agents through
//!                          the same filesystem without a second copy
//!
//! A layout may start with a header between `---` lines; `to: <path>` sends
//! it somewhere other than its own path, and `about: <one line>` lists the
//! file in its folder's README with what it is for.
//!
//! Code also writes, for every project with nodes, knowledge/<p>/README.md
//! (an index) and knowledge/<p>/rules/<node>.md (one note per node), and
//! graph/index.json, the whole graph for overview.

use super::entry::{self, Kind};
use super::graph::Graph;
use super::source::Source;
use anyhow::{Result, anyhow, bail};
use regex::Regex;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const STAMP: &str = "<!-- compiled by abot from the rules graph";

pub struct Layout {
    pub path: String,
    pub to: String,
    pub about: Option<String>,
    pub body: String,
}

pub struct Output {
    /// path -> text
    pub files: BTreeMap<String, String>,
    /// compiled path -> the nodes it shows
    pub shows: BTreeMap<String, Vec<String>>,
    /// compiled path -> the hand-written pages it includes
    pub includes: BTreeMap<String, Vec<String>>,
}

pub fn slot_re() -> Regex {
    Regex::new(r"\{\{\s*(rule|said|node|clashes|include)\s+([A-Za-z0-9_./#~-]+)\s*\}\}").unwrap()
}

pub fn load_layouts(src: &Source) -> Result<Vec<Layout>> {
    let mut v = Vec::new();
    for p in src.list("graph/layouts") {
        if !p.ends_with(".md") {
            continue;
        }
        let text = src.read(&p).unwrap_or_default();
        let natural = p.trim_start_matches("graph/layouts/").to_string();
        let (to, about, body) = match text.strip_prefix("---\n").and_then(|r| r.split_once("\n---\n")) {
            Some((head, body)) => {
                let mut to = natural.clone();
                let mut about = None;
                for line in head.lines() {
                    if let Some(x) = line.strip_prefix("to:") {
                        to = x.trim().to_string();
                    } else if let Some(x) = line.strip_prefix("about:") {
                        about = Some(x.trim().to_string());
                    }
                }
                (to, about, body.to_string())
            }
            None => (natural, None, text),
        };
        if to.starts_with("graph/") || to.starts_with('/') || to.contains("..") {
            bail!("{p}: a layout can't compile into {to}");
        }
        v.push(Layout { path: p, to, about, body });
    }
    Ok(v)
}

fn stamp(from: &str) -> String {
    format!("{STAMP} ({from}); change the graph, never this file -->\n")
}

/// The words a root's rule comes from, as a quote line.
fn said(g: &Graph, id: &str) -> Result<String> {
    let r = g.roots.get(id).ok_or_else(|| anyhow!("{{{{said {id}}}}}: {id} is not a root"))?;
    let e = g.entry(&r.from_entry).ok_or_else(|| anyhow!("root {id} has no entry"))?;
    Ok(e.quote())
}

fn open_clash_lines(g: &Graph, project: &str) -> String {
    let mut s = String::new();
    for c in g.clashes.values().filter(|c| c.status == "open" && c.project == project) {
        s.push_str(&clash_line(c));
        s.push('\n');
    }
    if s.is_empty() {
        return "None open.".into();
    }
    s.trim_end().to_string()
}

pub fn clash_line(c: &super::graph::Clash) -> String {
    format!("- Rules graph {}: {} (between {} and {}; asked {})", c.id, entry::one_para(&c.question), c.a, c.b, c.asked)
}

/// What a layout's slots read besides the graph: the hand-written pages
/// it may include, and the files no layout may include (compiled ones).
pub struct Pages<'a> {
    pub src: &'a Source,
    pub compiled: BTreeSet<String>,
}

impl<'a> Pages<'a> {
    pub fn new(src: &'a Source, layouts: &[Layout]) -> Self {
        Pages { src, compiled: layouts.iter().map(|l| l.to.clone()).collect() }
    }
}

/// A heading's anchor, the way GitHub makes one: lower case, letters and
/// digits kept, spaces and dashes as one dash, the rest dropped.
pub fn anchor(heading: &str) -> String {
    let mut s = String::new();
    for ch in heading.trim().chars() {
        if ch.is_alphanumeric() {
            s.extend(ch.to_lowercase());
        } else if (ch == ' ' || ch == '-') && !s.ends_with('-') {
            s.push('-');
        }
    }
    s.trim_matches('-').to_string()
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let n = line.chars().take_while(|&c| c == '#').count();
    (n > 0 && n <= 6 && line[n..].starts_with(' ')).then(|| (n, line[n..].trim()))
}

/// The text `{{include <spec>}}` stands for, and the page it came from.
/// A whole page drops its `# title` (the layout gives the file its own);
/// a section is everything under its heading, down to the next heading of
/// the same or a higher level, without the heading itself.
pub fn include(pages: &Pages, spec: &str) -> Result<(String, String)> {
    let (path, sect) = match spec.split_once('#') {
        Some((p, s)) => (p, Some(s)),
        None => (spec, None),
    };
    if path.starts_with('/') || path.contains("..") || path.starts_with('~') {
        bail!("{{{{include {spec}}}}}: say a path inside the log");
    }
    if path.starts_with("graph/") || path.starts_with("knowledge/") || pages.compiled.contains(path) {
        bail!("{{{{include {spec}}}}}: {path} is compiled; include the hand-written page it comes from");
    }
    let text = pages.src.read(path).ok_or_else(|| anyhow!("{{{{include {spec}}}}}: no file {path} in the log"))?;
    let lines: Vec<&str> = text.lines().collect();
    let body: Vec<&str> = match sect {
        None => {
            let mut v = lines.as_slice();
            if v.first().and_then(|l| heading(l)).is_some_and(|(n, _)| n == 1) {
                v = &v[1..];
            }
            v.to_vec()
        }
        Some(want) => {
            let mut fenced = false;
            let mut start = None;
            let mut out = vec![];
            for (i, l) in lines.iter().enumerate() {
                if l.trim_start().starts_with("```") {
                    fenced = !fenced;
                }
                let h = if fenced { None } else { heading(l) };
                match (start, h) {
                    (None, Some((n, t))) if anchor(t) == want => start = Some((i, n)),
                    (Some((_, n0)), Some((n, _))) if n <= n0 => break,
                    (Some(_), _) => out.push(*l),
                    _ => {}
                }
            }
            if start.is_none() {
                let have: Vec<String> = lines
                    .iter()
                    .filter_map(|l| heading(l))
                    .map(|(_, t)| anchor(t))
                    .collect();
                bail!("{{{{include {spec}}}}}: {path} has no heading #{want} (it has: {})", have.join(", "));
            }
            out
        }
    };
    let joined = body.join("\n");
    Ok((joined.trim_matches('\n').trim_end().to_string(), path.to_string()))
}

/// The level of the last heading in `text` (0 when there is none), so an
/// included page can sit under it.
fn last_heading_level(text: &str) -> usize {
    let mut fenced = false;
    let mut level = 0;
    for l in text.lines() {
        if l.trim_start().starts_with("```") {
            fenced = !fenced;
        } else if !fenced {
            if let Some((n, _)) = heading(l) {
                level = n;
            }
        }
    }
    level
}

/// Push an included text's headings down so the highest of them is one
/// level below `under`; the words stay as they are.
fn nest(text: &str, under: usize) -> String {
    let mut fenced = false;
    let mut top = 7;
    for l in text.lines() {
        if l.trim_start().starts_with("```") {
            fenced = !fenced;
        } else if !fenced {
            if let Some((n, _)) = heading(l) {
                top = top.min(n);
            }
        }
    }
    if top == 7 || top > under {
        return text.to_string();
    }
    let by = under + 1 - top;
    let mut fenced = false;
    let mut out = Vec::new();
    for l in text.lines() {
        if l.trim_start().starts_with("```") {
            fenced = !fenced;
            out.push(l.to_string());
        } else if !fenced && heading(l).is_some() {
            let n = heading(l).unwrap().0;
            out.push(format!("{}{}", "#".repeat((n + by).min(6)), &l[n..]));
        } else {
            out.push(l.to_string());
        }
    }
    out.join("\n")
}

/// Fill one layout. Returns the text, the nodes it shows and the pages it
/// includes.
pub fn fill(g: &Graph, pages: &Pages, body: &str) -> Result<(String, Vec<String>, Vec<String>)> {
    let re = slot_re();
    let mut shows = Vec::new();
    let mut included = Vec::new();
    let mut err = None;
    let out = re.replace_all(body, |c: &regex::Captures| {
        let (what, id) = (&c[1], &c[2]);
        let r: Result<String> = match what {
            "clashes" => Ok(open_clash_lines(g, id)),
            "include" => include(pages, id).map(|(t, p)| {
                included.push(p);
                let under = last_heading_level(&body[..c.get(0).unwrap().start()]);
                nest(&t, under)
            }),
            _ if g.kind(id).is_none() => Err(anyhow!("{{{{{what} {id}}}}}: no node {id} in the graph")),
            "rule" => Ok(g.rule(id).unwrap_or_default()),
            "said" => said(g, id),
            _ => {
                let mut s = g.rule(id).unwrap_or_default();
                if let Some(e) = g.roots.get(id).and_then(|r| g.entry(&r.from_entry)) {
                    match e.kind {
                        Kind::Said => s.push_str(&format!(" ({})", e.quote())),
                        Kind::Seen => s.push_str(&format!(
                            " (seen {}; proof: {})",
                            entry::short_date(&e.date),
                            e.proof.as_deref().unwrap_or("")
                        )),
                    }
                }
                Ok(s)
            }
        };
        if what != "clashes" && what != "include" {
            shows.push(id.to_string());
        }
        match r {
            Ok(s) => s,
            Err(e) => {
                err.get_or_insert(e);
                String::new()
            }
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    shows.sort();
    shows.dedup();
    included.sort();
    included.dedup();
    Ok((out.into_owned(), shows, included))
}

pub fn note_path(project: &str, id: &str) -> String {
    format!("knowledge/{project}/rules/{id}.md")
}

/// A link from one note to another, relative to knowledge/<p>/rules/<x>.md.
fn link(g: &Graph, from_project: &str, from_id: &str, to: &str) -> String {
    let p = g.project_of(to).unwrap_or_else(|| "org".into());
    let depth = from_id.matches('/').count();
    let up = "../".repeat(depth);
    let rel = if p == from_project { format!("{up}{to}.md") } else { format!("{up}../../{p}/rules/{to}.md") };
    format!("[{to}]({rel})")
}

fn note(g: &Graph, id: &str, shown_in: &[String]) -> String {
    let project = g.project_of(id).unwrap_or_default();
    let depth = id.matches('/').count();
    let to_root = "../".repeat(3 + depth);
    let mut s = stamp("code, one note per node");
    s.push_str(&format!("# {id}\n\n"));
    if let Some(r) = g.roots.get(id) {
        s.push_str(&format!("Root · {project} · changes {} · {}\n\n", r.changes, r.status));
        s.push_str(&format!("**Rule:** {}\n\n", r.rule));
        if let Some(l) = &r.lags {
            s.push_str(&format!("**The boxes lag:** they still run what was seen: {l}\n\n"));
        }
        s.push_str("## Entries\n\n");
        for st in r.entries.iter().rev() {
            let e = g.entry(&st.id).unwrap();
            let kind = if e.kind == Kind::Said { "said" } else { "seen" };
            let standing = match &st.by {
                Some(b) => format!("{} by {b}", st.standing),
                None => st.standing.clone(),
            };
            s.push_str(&format!("- **{standing}** · {kind} {} · {} · [{}]({to_root}{})\n", e.date, e.by, e.id, e.path));
            for line in e.body.lines() {
                s.push_str(&format!("  > {line}\n"));
            }
            if let Some(p) = &e.proof {
                s.push_str(&format!("  proof: {p}\n"));
            }
            if e.kind == Kind::Said {
                s.push_str(&format!("  rule: {}\n", e.sentence()));
            }
        }
        s.push('\n');
    } else if let Some(f) = g.facts.get(id) {
        s.push_str(&format!("Fact · {project} · {}\n\n", f.status()));
        s.push_str(&format!("**Asks:** {}\n\n**Rule:** {}\n\n", f.asks, f.rule()));
        if let Some(v) = f.now() {
            if let Some(x) = &v.exception {
                s.push_str(&format!("**Exception, not a clash:** {x}\n\n"));
            }
            if let Some(c) = &v.clash {
                s.push_str(&format!("**Open clash:** {}\n\n", link(g, &project, id, c)));
            }
        }
        s.push_str("## From\n\n");
        for p in &f.from {
            s.push_str(&format!("- {} — {}\n", link(g, &project, id, p), g.rule(p).unwrap_or_default()));
        }
        s.push_str("\n## Rests on\n\n");
        for r in g.roots_under(id) {
            if let Some(e) = g.roots.get(&r).and_then(|r| g.entry(&r.from_entry)) {
                s.push_str(&format!("- {}: {}\n", link(g, &project, id, &r), e.quote()));
            }
        }
        s.push('\n');
    } else if let Some(c) = g.clashes.get(id) {
        s.push_str(&format!("Clash · {project} · {}\n\n", c.status));
        s.push_str(&format!("**Question:** {}\n\n", c.question));
        s.push_str(&format!("- a: {} — {}\n", link(g, &project, id, &c.a), g.rule(&c.a).unwrap_or_default()));
        s.push_str(&format!("- b: {} — {}\n", link(g, &project, id, &c.b), g.rule(&c.b).unwrap_or_default()));
        s.push_str(&format!("- raised by: {}\n", link(g, &project, id, &c.raised_by)));
        s.push_str(&format!("- asked: {}, {}\n", c.asked, c.where_));
        if let Some(e) = c.answer_entry.as_deref().and_then(|x| g.entry(x)) {
            s.push_str(&format!("- answer: {} ({})\n", e.quote(), e.id));
            s.push_str(&format!("- why: {}\n", e.why.clone().unwrap_or_else(|| e.sentence())));
        } else if c.status == "settled" {
            s.push_str("- settled: its sides no longer conflict\n");
        }
        s.push('\n');
    }
    let below = g.children(id);
    if !below.is_empty() {
        s.push_str("## Below it\n\n");
        for c in below {
            s.push_str(&format!("- {}\n", link(g, &project, id, &c)));
        }
        s.push('\n');
    }
    if !shown_in.is_empty() {
        s.push_str("## Shown in\n\n");
        for f in shown_in {
            s.push_str(&format!("- `{f}`\n"));
        }
        s.push('\n');
    }
    s.trim_end().to_string() + "\n"
}

fn readme(g: &Graph, project: &str, layouts: &[Layout]) -> String {
    let mut s = stamp("code, the index of this folder");
    s.push_str(&format!("# {project}: what the rules graph knows\n\n"));
    s.push_str(
        "Every file here is compiled from the rules graph in `graph/` and is rebuilt by \
`abot rules build`. Each note in `rules/` is one node: a root (one subject, its rule and \
every entry behind it), a fact (a rule built from other nodes) or a clash (a question \
for Felix). `abot why <node>` walks up to his words; `abot affects <entry>` walks down \
to the files.\n\n",
    );
    let prefix = format!("knowledge/{project}/");
    // A file is listed by what its layout's about: line says it is for.
    let mut mine: Vec<(&str, &str)> = layouts
        .iter()
        .filter(|l| l.to.starts_with(&prefix) && l.to != format!("{prefix}README.md"))
        .filter_map(|l| Some((&l.to[prefix.len()..], l.about.as_deref()?)))
        .collect();
    mine.sort();
    if !mine.is_empty() {
        s.push_str("## Files\n\nOne file per need: how to do a thing, with the rules it rests on.\n\n");
        for (rel, about) in mine {
            s.push_str(&format!("- [{rel}]({rel}) — {about}\n"));
        }
        s.push('\n');
    }
    let open: Vec<_> = g.clashes.values().filter(|c| c.project == project && c.status == "open").collect();
    if !open.is_empty() {
        s.push_str("## Open clashes\n\n");
        for c in open {
            s.push_str(&format!("- [{}](rules/{}.md) — {}\n", c.id, c.id, entry::one_para(&c.question)));
        }
        s.push('\n');
    }
    if !g.roots.values().any(|r| r.project == project) {
        s.push_str("Nothing in the graph is about this project yet. Its rules are still in its state file's Traps, its how/ pages and its repo's CLAUDE.md, until they are entries.\n\n");
    }
    let roots: Vec<_> = g.roots.values().filter(|r| r.project == project).collect();
    if !roots.is_empty() {
        s.push_str("## Roots\n\n");
        for r in roots {
            let lag = if r.status == "lags" { " (the boxes lag)" } else { "" };
            s.push_str(&format!("- [{}](rules/{}.md) — {}{lag}\n", r.id, r.id, entry::one_para(&r.rule)));
        }
        s.push('\n');
    }
    let facts: Vec<_> = g.facts.values().filter(|f| f.project == project && f.status() != "gone").collect();
    if !facts.is_empty() {
        s.push_str("## Facts\n\n");
        for f in facts {
            s.push_str(&format!("- [{}](rules/{}.md) — {}\n", f.id, f.id, entry::one_para(&f.rule())));
        }
        s.push('\n');
    }
    let settled: Vec<_> = g.clashes.values().filter(|c| c.project == project && c.status == "settled").collect();
    if !settled.is_empty() {
        s.push_str("## Settled clashes\n\n");
        for c in settled {
            s.push_str(&format!("- [{}](rules/{}.md)\n", c.id, c.id));
        }
        s.push('\n');
    }
    s.trim_end().to_string() + "\n"
}

/// Every project with a state file gets a folder, nodes or not.
pub fn compile(g: &Graph, src: &Source, layouts: &[Layout], projects_with_state: &[String]) -> Result<Output> {
    let pages = Pages::new(src, layouts);
    let mut files = BTreeMap::new();
    let mut shows = BTreeMap::new();
    let mut includes = BTreeMap::new();
    let mut layout_of = BTreeMap::new();
    let mut shown_in: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for l in layouts {
        let (text, nodes, inc) = fill(g, &pages, &l.body).map_err(|e| anyhow!("{}: {e:#}", l.path))?;
        if !inc.is_empty() {
            includes.insert(l.to.clone(), inc);
        }
        let text = collapse_blank(&text);
        let stamped = if l.to.ends_with(".md") { format!("{}{}", stamp(&l.path), text) } else { text };
        if files.insert(l.to.clone(), stamped).is_some() {
            bail!("two layouts compile into {}", l.to);
        }
        for n in &nodes {
            shown_in.entry(n.clone()).or_default().push(l.to.clone());
        }
        shows.insert(l.to.clone(), nodes);
        layout_of.insert(l.to.clone(), l.path.clone());
    }
    let mut projects: BTreeSet<String> = projects_with_state.iter().cloned().collect();
    let ids: Vec<String> = g.roots.keys().chain(g.facts.keys()).chain(g.clashes.keys()).cloned().collect();
    for id in &ids {
        if g.facts.get(id).map(|f| f.status() == "gone").unwrap_or(false) {
            continue;
        }
        let p = g.project_of(id).unwrap_or_default();
        projects.insert(p.clone());
        let path = note_path(&p, id);
        files.insert(path.clone(), note(g, id, shown_in.get(id).map(Vec::as_slice).unwrap_or(&[])));
        shows.insert(path, vec![id.clone()]);
    }
    for p in &projects {
        let path = format!("knowledge/{p}/README.md");
        if !files.contains_key(&path) {
            files.insert(path.clone(), readme(g, p, layouts));
            shows.insert(path, ids.iter().filter(|i| g.project_of(i).as_deref() == Some(p)).cloned().collect());
        }
    }
    let index = index_json(g, &shows, &layout_of, &includes);
    files.insert("graph/index.json".into(), serde_json::to_string_pretty(&index)? + "\n");
    Ok(Output { files, shows, includes })
}

fn collapse_blank(s: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in s.lines() {
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The whole graph in one file, the shape overview's /try/rules reads.
pub fn index_json(
    g: &Graph,
    shows: &BTreeMap<String, Vec<String>>,
    layout_of: &BTreeMap<String, String>,
    includes: &BTreeMap<String, Vec<String>>,
) -> Value {
    let built = g
        .entries
        .iter()
        .map(|e| e.date.clone())
        .chain(g.facts.values().flat_map(|f| f.versions.iter().map(|v| v.at.clone())))
        .max()
        .unwrap_or_default();
    let entries: Vec<Value> = g
        .entries
        .iter()
        .map(|e| {
            let mut v = json!({
                "id": e.id, "project": e.project, "path": e.path,
                "kind": if e.kind == Kind::Said { "said" } else { "seen" },
                "by": e.by, "date": e.date, "in_force": e.in_force, "about": e.about,
            });
            let m = v.as_object_mut().unwrap();
            match e.kind {
                Kind::Said => {
                    m.insert("words".into(), json!(e.body));
                    m.insert("rule".into(), json!(e.rule));
                }
                Kind::Seen => {
                    m.insert("what".into(), json!(e.body));
                    m.insert("proof".into(), json!(e.proof));
                }
            }
            for (k, x) in [
                ("replaces", &e.replaces),
                ("contradicts", &e.contradicts),
                ("corrects", &e.corrects),
                ("settles", &e.settles),
                ("changes", &e.changes),
                ("why", &e.why),
                ("where", &e.where_),
            ] {
                if let Some(x) = x {
                    m.insert(k.into(), json!(x));
                }
            }
            v
        })
        .collect();
    let roots: Vec<Value> = g.roots.values().map(|r| serde_json::to_value(r).unwrap()).collect();
    let facts: Vec<Value> = g
        .facts
        .values()
        .map(|f| {
            let now = f.now();
            json!({
                "id": f.id, "project": f.project, "path": f.path, "asks": f.asks,
                "rule": f.rule(), "status": f.status(), "from": f.from,
                "fingerprint": f.fingerprint,
                "clash": now.and_then(|v| v.clash.clone()),
                "exception": now.and_then(|v| v.exception.clone()),
            })
        })
        .collect();
    let side = |id: &str| -> String {
        let rule = g.rule(id).unwrap_or_default();
        match g.roots.get(id).and_then(|r| g.entry(&r.from_entry)) {
            Some(e) => format!("{id}, {} {}: {rule}", if e.kind == Kind::Said { "said" } else { "seen" }, entry::short_date(&e.date)),
            None => format!("{id}, fact: {rule}"),
        }
    };
    let clashes: Vec<Value> = g
        .clashes
        .values()
        .map(|c| {
            let ans = c.answer_entry.as_deref().and_then(|x| g.entry(x));
            json!({
                "id": c.id, "project": c.project, "path": c.path, "status": c.status,
                "from": [c.raised_by], "sides": [c.a, c.b],
                "a": side(&c.a), "b": side(&c.b), "question": c.question,
                "asked": c.asked, "where": c.where_,
                "answer": ans.map(|e| e.quote()),
                "why": ans.map(|e| e.why.clone().unwrap_or_else(|| e.sentence())),
                "answer_entry": c.answer_entry,
            })
        })
        .collect();
    let files: Vec<Value> = shows
        .iter()
        .map(|(p, n)| {
            let mut v = json!({"path": p, "layout": layout_of.get(p), "shows": n});
            if let Some(inc) = includes.get(p) {
                v["includes"] = json!(inc);
            }
            v
        })
        .collect();
    json!({
        "version": 1,
        "built": built,
        "as_of": g.as_of,
        "entries": entries, "roots": roots, "facts": facts, "clashes": clashes, "files": files,
    })
}

/// Prose a layout writes outside its slots must only orient the reader: a
/// sentence there that quotes Felix or states a rule has no entry behind it
/// and could drift. Returns the offending lines.
pub fn prose_problems(layout: &Layout) -> Vec<String> {
    let slot = slot_re();
    let quote = Regex::new(r#"(?i)\bFelix\b[^.\n]{0,40}(\d{1,2}-\d{1,2}|said|:\s*["“])"#).unwrap();
    let rule = Regex::new(r"(?i)\b(must|never|always|should|shall|don't|do not|may not|is not allowed|no one may)\b").unwrap();
    let mut out = Vec::new();
    let mut fenced = false;
    for (i, line) in layout.body.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let bare = slot.replace_all(line, "");
        if quote.is_match(&bare) {
            out.push(format!("{}:{}: quotes Felix outside a slot: {}", layout.path, i + 1, line.trim()));
        } else if rule.is_match(&bare) {
            out.push(format!("{}:{}: states a rule outside a slot: {}", layout.path, i + 1, line.trim()));
        }
    }
    out
}

fn norm_quotes(s: &str) -> String {
    let s = s.replace(['\u{201c}', '\u{201d}'], "\"");
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Felix's words are entries, so a page a layout includes may quote him
/// only with words an entry already holds: otherwise his word would live
/// in a hand-written page with nothing in the graph behind it. Returns one
/// line per quote that is no entry's words.
pub fn include_quote_problems(g: &Graph, pages: &Pages, layout: &Layout) -> Vec<String> {
    let said: Vec<String> = g.entries.iter().filter(|e| e.kind == Kind::Said).map(|e| norm_quotes(&e.body)).collect();
    let quote = Regex::new(r#"Felix[^."]{0,80}?:\s*"([^"]{15,})""#).unwrap();
    let mut out = Vec::new();
    for c in slot_re().captures_iter(&layout.body) {
        if &c[1] != "include" {
            continue;
        }
        let Ok((text, _)) = include(pages, &c[2]) else { continue };
        for q in quote.captures_iter(&norm_quotes(&text)) {
            let words = q[1].trim_end_matches(['.', ',', ' ']);
            if !said.iter().any(|b| b.contains(words)) {
                out.push(format!(
                    "{}: {{{{include {}}}}} quotes Felix with words no entry holds: \"{}\"; add them with `abot entry add --said \"…\"`, or quote the entry's words",
                    layout.path,
                    &c[2],
                    entry::one_para(words)
                ));
            }
        }
    }
    out
}
