//! Tickets: the work a project has ready, written so that someone with no
//! context can read one and know what it is, why it matters, when it is
//! done, and whether it waits on anyone.
//!
//! One file per project, `state/tickets/<project>.md`, written by `abot
//! ticket` and never by hand. It is state: overwritten as the truth
//! changes, with git keeping what it said before. The format is plain
//! markdown, so it reads well on its own:
//!
//! ```text
//! # agent-tooling: tickets
//!
//! ## Open
//!
//! ### local-model-client: One local-model client for every desk
//! Why: …
//! Done when: …
//! Waits on: felix: …
//! Detail: …
//! Filed: 2026-10-16
//! 1. [todo] …
//! 2. [done] … (2026-10-16, Dorky-Robot/abot 1a2b3c4)
//!
//! ## Done
//! - <id>: <title> (2026-10-16, <ref>)
//!
//! ## Retired
//! - <id>: <title>. <why> (2026-10-16)
//! ```
//!
//! Steps are only ever appended, so a step's number is stable and a commit
//! can name it: `Ready: <project>/<id>.<n>`.

use regex::Regex;
use std::sync::OnceLock;

pub const DIR: &str = "state/tickets";

/// How many finished tickets a file keeps under Done; git has the rest.
pub const DONE_KEPT: usize = 10;

pub fn path(project: &str) -> String {
    format!("{DIR}/{project}.md")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waits {
    pub who: String,
    pub what: String,
}

impl Waits {
    /// `felix: grade the insights`, or a bare `felix` (what is then empty).
    pub fn parse(s: &str) -> Option<Waits> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let (who, what) = match s.split_once(':') {
            Some((w, x)) => (w.trim(), x.trim()),
            None => (s, ""),
        };
        Some(Waits { who: who.to_ascii_lowercase(), what: what.to_string() })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub text: String,
    pub done: bool,
    /// When and by what it was marked done: `2026-10-16, Dorky-Robot/abot 1a2b3c4`.
    pub by: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ticket {
    pub id: String,
    pub title: String,
    pub why: String,
    pub done_when: String,
    pub waits: Option<Waits>,
    pub detail: Option<String>,
    pub filed: Option<String>,
    pub steps: Vec<Step>,
}

/// A finished or retired ticket: one line each.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Closed {
    pub id: String,
    pub title: String,
    /// Done: what finished it. Retired: why it was retired.
    pub note: String,
    pub date: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct File {
    pub project: String,
    pub open: Vec<Ticket>,
    pub done: Vec<Closed>,
    pub retired: Vec<Closed>,
}

fn re(cell: &'static OnceLock<Regex>, pat: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pat).unwrap())
}

fn heading_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"^###\s+([a-z0-9][a-z0-9-]*):\s+(.+?)\s*$")
}

fn step_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"^\s*\d+\.\s+\[(todo|done)\]\s+(.+?)\s*$")
}

fn by_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"^(.*\S)\s+\((\d{4}-\d{2}-\d{2}[^()]*)\)$")
}

fn closed_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"^-\s+([a-z0-9][a-z0-9-]*):\s+(.+?)\s*$")
}

/// A Closed line's tail: `<title>. <note> (<date>)` for Retired, `<title>
/// (<date>, <ref>)` for Done. The date is always the last parenthesis.
fn split_closed(rest: &str, retired: bool) -> (String, String, String) {
    let (body, paren) = match by_re().captures(rest) {
        Some(c) => (c[1].to_string(), c[2].to_string()),
        None => (rest.to_string(), String::new()),
    };
    let (date, after) = match paren.split_once(", ") {
        Some((d, a)) => (d.to_string(), a.to_string()),
        None => (paren.clone(), String::new()),
    };
    if retired {
        // The title is the part before the first ". " that the note follows.
        match body.split_once(" — ") {
            Some((t, n)) => (t.trim().to_string(), n.trim().to_string(), date),
            None => (body.trim().to_string(), after, date),
        }
    } else {
        (body.trim().to_string(), after, date)
    }
}

pub fn parse(project: &str, text: &str) -> File {
    let mut f = File { project: project.to_string(), ..File::default() };
    let mut section = "";
    let mut cur: Option<Ticket> = None;
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            if let Some(t) = cur.take() {
                f.open.push(t);
            }
            section = match h.trim().to_ascii_lowercase().as_str() {
                "open" => "open",
                "done" => "done",
                "retired" => "retired",
                _ => "",
            };
            continue;
        }
        match section {
            "open" => {
                if let Some(c) = heading_re().captures(line) {
                    if let Some(t) = cur.take() {
                        f.open.push(t);
                    }
                    cur = Some(Ticket {
                        id: c[1].to_string(),
                        title: c[2].to_string(),
                        why: String::new(),
                        done_when: String::new(),
                        waits: None,
                        detail: None,
                        filed: None,
                        steps: vec![],
                    });
                    continue;
                }
                let Some(t) = cur.as_mut() else { continue };
                if let Some(c) = step_re().captures(line) {
                    let (text, by) = match by_re().captures(&c[2]) {
                        Some(b) if &c[1] == "done" => (b[1].to_string(), Some(b[2].to_string())),
                        _ => (c[2].to_string(), None),
                    };
                    t.steps.push(Step { text, done: &c[1] == "done", by });
                    continue;
                }
                if let Some((k, v)) = line.split_once(':') {
                    let v = v.trim().to_string();
                    match k.trim().to_ascii_lowercase().as_str() {
                        "why" => t.why = v,
                        "done when" => t.done_when = v,
                        "waits on" => t.waits = Waits::parse(&v),
                        "detail" => t.detail = (!v.is_empty()).then_some(v),
                        "filed" => t.filed = (!v.is_empty()).then_some(v),
                        _ => {}
                    }
                }
            }
            "done" | "retired" => {
                if let Some(c) = closed_re().captures(line) {
                    let (title, note, date) = split_closed(&c[2], section == "retired");
                    let x = Closed { id: c[1].to_string(), title, note, date };
                    if section == "done" { f.done.push(x) } else { f.retired.push(x) }
                }
            }
            _ => {}
        }
    }
    if let Some(t) = cur.take() {
        f.open.push(t);
    }
    f
}

pub fn render(f: &File) -> String {
    let mut s = format!(
        "# {}: tickets\n\nWritten by `abot ticket`, never by hand (how/tickets.md). A commit names a step with `Ready: {}/<id>.<n>`.\n\n## Open\n",
        f.project, f.project
    );
    for t in &f.open {
        s.push_str(&format!("\n### {}: {}\n", t.id, one(&t.title)));
        s.push_str(&format!("Why: {}\n", one(&t.why)));
        s.push_str(&format!("Done when: {}\n", one(&t.done_when)));
        if let Some(w) = &t.waits {
            s.push_str(&format!("Waits on: {}: {}\n", w.who, one(&w.what)));
        }
        if let Some(d) = &t.detail {
            s.push_str(&format!("Detail: {}\n", one(d)));
        }
        if let Some(d) = &t.filed {
            s.push_str(&format!("Filed: {d}\n"));
        }
        for (i, st) in t.steps.iter().enumerate() {
            let by = match (&st.by, st.done) {
                (Some(b), true) => format!(" ({b})"),
                _ => String::new(),
            };
            s.push_str(&format!("{}. [{}] {}{by}\n", i + 1, if st.done { "done" } else { "todo" }, one(&st.text)));
        }
    }
    if f.open.is_empty() {
        s.push_str("\n(nothing open)\n");
    }
    if !f.done.is_empty() {
        s.push_str("\n## Done\n");
        for d in &f.done {
            let tail = if d.note.is_empty() { d.date.clone() } else { format!("{}, {}", d.date, d.note) };
            s.push_str(&format!("- {}: {} ({tail})\n", d.id, one(&d.title)));
        }
    }
    if !f.retired.is_empty() {
        s.push_str("\n## Retired\n");
        for d in &f.retired {
            s.push_str(&format!("- {}: {} — {} ({})\n", d.id, one(&d.title), one(&d.note), d.date));
        }
    }
    s
}

/// Whitespace collapsed to one line: a field is one line in the file.
pub fn one(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for w in s.to_ascii_lowercase().split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()) {
        if ["a", "an", "the", "of", "for", "to", "and", "on", "in"].contains(&w) {
            continue;
        }
        if out.len() + w.len() + 1 > 40 {
            break;
        }
        if !out.is_empty() {
            out.push('-');
        }
        out.push_str(w);
    }
    if out.is_empty() { "ticket".into() } else { out }
}

impl File {
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.open.iter().map(|t| t.id.as_str()).chain(self.done.iter().map(|d| d.id.as_str())).chain(self.retired.iter().map(|d| d.id.as_str()))
    }

    pub fn find(&self, id: &str) -> Option<&Ticket> {
        self.open.iter().find(|t| t.id == id)
    }

    pub fn find_mut(&mut self, id: &str) -> Option<&mut Ticket> {
        self.open.iter_mut().find(|t| t.id == id)
    }

    /// Move every ticket whose steps are all done to Done, newest first,
    /// keeping DONE_KEPT. Returns the ids moved.
    pub fn settle(&mut self, date: &str, note: &str) -> Vec<String> {
        let (finished, open): (Vec<Ticket>, Vec<Ticket>) =
            std::mem::take(&mut self.open).into_iter().partition(|t| !t.steps.is_empty() && t.steps.iter().all(|s| s.done));
        self.open = open;
        let ids: Vec<String> = finished.iter().map(|t| t.id.clone()).collect();
        for t in finished.into_iter().rev() {
            self.done.insert(0, Closed { id: t.id, title: t.title, note: note.to_string(), date: date.to_string() });
        }
        self.done.truncate(DONE_KEPT);
        ids
    }
}

// ---------------------------------------------------------------- the rules

/// What a filed ticket must have. These are the floor the model can't
/// talk its way under: a thin ticket is refused with or without it.
pub const RULES: &str = "\
A ticket is read first by Felix, and by agents with no context. Every one has:
- A title that says what the work is, in plain words (not a code name or a label).
- Why: one sentence on what it is for: what gets better, for whom, when it is done.
- Done when: what someone with no context can check from outside to know it is done.
  Not \"it works\" or \"shipped\": what they would see, run or ask.
- Steps: each a plain sentence saying what will be true after it, never a label like
  \"C: views\", \"Docs pass 2\" or \"Phase 3\". Steps are numbered in the order they land.
- Waits on: who must act before it can move (felix, or another person), and what they must
  do, only if someone does. Agents and workers are not who a ticket waits on.
- Detail (optional): where the long version lives: a doc, a state file, a commit.
No secrets, tokens, phone numbers or client records; point at where a thing lives.";

fn words(s: &str) -> usize {
    s.split_whitespace().count()
}

fn label_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(
        &R,
        r"(?i)^(?:[a-z]\d*\s*[:.)]|(?:slice|step|phase|part|stage|epic|milestone|pass)\s*[a-z0-9]{1,3}\b)|\bpass\s*\d+\b|\b(?:tbd|todo|wip|misc|etc\.?)\b|\?\?",
    )
}

/// A step that opens with a one-word label: "Done: …", "Views: …".
fn step_label_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"^[A-Za-z][A-Za-z-]*:\s")
}

fn vague_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(
        &R,
        r"(?i)^(?:it\s+)?(?:is\s+|'s\s+)?(?:done|works|working|shipped|merged|landed|complete|completed|finished|live|tbd|n/?a)[.!]?$",
    )
}

/// What is missing or thin, as sentences a writer can act on. Empty when
/// the ticket is written out.
pub fn lint(t: &Ticket) -> Vec<String> {
    let mut out = vec![];
    if !crate::branch::valid_slug(&t.id) || t.id.len() > 48 {
        out.push(format!("id `{}`: use lowercase words joined by dashes, at most 48 characters", t.id));
    }
    let title = t.title.trim();
    if words(title) < 3 {
        out.push(format!("title \"{title}\": say what the work is in at least three plain words"));
    } else if label_re().is_match(title) {
        out.push(format!("title \"{title}\" reads like a label; say what the work is"));
    }
    if title.chars().count() > 90 {
        out.push("title: keep it under 90 characters; the rest belongs in Why".into());
    }
    let why = t.why.trim();
    if words(why) < 6 {
        out.push("Why: one sentence (six words or more) on what this is for".into());
    } else if why.eq_ignore_ascii_case(title) {
        out.push("Why: says the title again; say what it is for".into());
    }
    let done = t.done_when.trim();
    if words(done) < 6 || vague_re().is_match(done) {
        out.push("Done when: say what someone with no context can check to know it is done (six words or more, not \"it works\")".into());
    }
    if t.steps.is_empty() {
        out.push("steps: at least one, each a plain sentence".into());
    }
    for (i, s) in t.steps.iter().enumerate() {
        let x = s.text.trim();
        if words(x) < 4 {
            out.push(format!("step {} \"{x}\": write it as a plain sentence (four words or more)", i + 1));
        } else if label_re().is_match(x) || step_label_re().is_match(x) {
            out.push(format!("step {} \"{x}\" reads like a label; say what will be true after it", i + 1));
        }
        if x.chars().count() > 200 {
            out.push(format!("step {}: keep it under 200 characters", i + 1));
        }
    }
    if let Some(w) = &t.waits {
        if !crate::branch::valid_slug(&w.who) {
            out.push(format!("Waits on `{}`: one name, like felix", w.who));
        }
        if words(&w.what) < 3 {
            out.push(format!("Waits on {}: say what they must do (three words or more)", w.who));
        }
    }
    for (k, v) in [("Why", why), ("Done when", done)] {
        if v.chars().count() > 400 {
            out.push(format!("{k}: keep it under 400 characters; put the long version in Detail"));
        }
    }
    out
}

/// A `Ready:` reference: `<project>/<id>` (the whole ticket) or
/// `<project>/<id>.<n>` (one step).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ref {
    pub project: String,
    pub id: String,
    pub step: Option<usize>,
}

impl std::fmt::Display for Ref {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.step {
            Some(n) => write!(f, "{}/{}.{n}", self.project, self.id),
            None => write!(f, "{}/{}", self.project, self.id),
        }
    }
}

pub fn parse_ref(s: &str) -> Option<Ref> {
    static R: OnceLock<Regex> = OnceLock::new();
    let c = re(&R, r"^([a-z0-9][a-z0-9-]*)/([a-z0-9][a-z0-9-]*)(?:\.(\d+))?$").captures(s.trim())?;
    Some(Ref { project: c[1].to_string(), id: c[2].to_string(), step: c.get(3).and_then(|m| m.as_str().parse().ok()) })
}

/// Every `Ready: <ref>` line in a commit message (anywhere in the body:
/// sessions put Co-Authored-By after it, outside git's trailer block).
pub fn ready_refs(msg: &str) -> Vec<Ref> {
    msg.lines()
        .filter_map(|l| l.trim().strip_prefix("Ready:"))
        .filter_map(|r| parse_ref(r.trim()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> Ticket {
        Ticket {
            id: "local-model-client".into(),
            title: "One local-model client for every desk".into(),
            why: "Monica's and bahay's desks can answer from our own model without copying Everyday Vet's code.".into(),
            done_when: "Monica's desk and Everyday Vet's desk both call the same client module, and a stalled model times out instead of hanging.".into(),
            waits: None,
            detail: Some("state/projects/agent-tooling.md".into()),
            filed: Some("2026-10-16".into()),
            steps: vec![
                Step { text: "Lift the queue, timeout and health check out of Everyday Vet's desk chat".into(), done: true, by: Some("2026-10-16, Dorky-Robot/abot 1a2b3c4".into()) },
                Step { text: "Monica's desk answers through the shared client".into(), done: false, by: None },
            ],
        }
    }

    #[test]
    fn a_written_out_ticket_lints_clean_and_round_trips() {
        let t = good();
        assert!(lint(&t).is_empty(), "{:?}", lint(&t));
        let mut f = File { project: "agent-tooling".into(), open: vec![t.clone()], ..File::default() };
        f.open[0].waits = Some(Waits { who: "felix".into(), what: "pick which desk goes first".into() });
        f.retired.push(Closed { id: "diwa-hub".into(), title: "diwa: one hub on the mini".into(), note: "landed (0e76944), Felix 10-15".into(), date: "2026-10-16".into() });
        f.done.push(Closed { id: "old-one".into(), title: "An old finished ticket".into(), note: "Dorky-Robot/abot 1a2b3c4".into(), date: "2026-10-15".into() });
        let text = render(&f);
        let back = parse("agent-tooling", &text);
        assert_eq!(back, f, "{text}");
        assert!(text.contains("1. [done] Lift the queue"), "{text}");
        assert!(text.contains("2. [todo] Monica's desk"));
    }

    #[test]
    fn thin_tickets_are_named_for_what_they_lack() {
        let mut t = good();
        t.title = "C: views".into();
        t.why = "views".into();
        t.done_when = "it works".into();
        t.steps = vec![
            Step { text: "Docs pass 2".into(), done: false, by: None },
            Step { text: "Phase 3: wire up the views to the store".into(), done: false, by: None },
            Step { text: "Done: the desk chat answers from dr1's model".into(), done: false, by: None },
        ];
        t.waits = Some(Waits { who: "felix".into(), what: String::new() });
        let l = lint(&t).join("\n");
        for want in ["title", "Why", "Done when", "step 1", "step 2", "step 3", "Waits on felix"] {
            assert!(l.contains(want), "missing {want} in:\n{l}");
        }
    }

    #[test]
    fn refs_and_slugs() {
        assert_eq!(parse_ref("agent-tooling/local-model-client.2"), Some(Ref { project: "agent-tooling".into(), id: "local-model-client".into(), step: Some(2) }));
        assert_eq!(parse_ref("mesh/stays-up").unwrap().step, None);
        assert!(parse_ref("nope").is_none());
        let msg = "Do a thing\n\nbody\nReady: mesh/stays-up.1\n\nCo-Authored-By: x\n";
        assert_eq!(ready_refs(msg).len(), 1);
        assert_eq!(slug("The mesh stays up without us!"), "mesh-stays-up-without-us");
    }

    #[test]
    fn settle_moves_finished_tickets_to_done() {
        let mut t = good();
        t.steps[1].done = true;
        let mut f = File { project: "p".into(), open: vec![t, Ticket { id: "other".into(), ..good() }], ..File::default() };
        f.open[1].steps[1].done = false;
        let moved = f.settle("2026-10-16", "abot 123");
        assert_eq!(moved, vec!["local-model-client"]);
        assert_eq!(f.open.len(), 1);
        assert_eq!(f.done[0].id, "local-model-client");
    }
}
