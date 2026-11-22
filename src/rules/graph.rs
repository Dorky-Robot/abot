//! The graph: entries folded by code into one root per subject, facts and
//! clashes written by a model from their parents only, every model output
//! kept under a fingerprint of its inputs.
//!
//! graph/facts/<project>/<id>.md
//!
//!     fact: session.closes-itself
//!     asks: How does a session end, and who ends it?
//!     from: session.close, process.ownership
//!
//!     ## 3f9a2b1c4d5e · 2026-11-22 · current
//!     A session closes itself at its own land, dead-end or park.
//!
//! The header (what the fact answers and its parents) is proposed by the
//! model when an entry is added and confirmed by the session. Each `##`
//! section is one answer the model gave, under the fingerprint of the
//! inputs it saw; the fact's current answer is the section whose
//! fingerprint matches its inputs now. Sections are only ever added, so a
//! replay of any past day finds its answer without asking again.
//!
//! graph/clashes/<project>/<name>.md
//!
//!     clash: clash/waiting-vs-idle-stop
//!     raised-by: wait.on-felix
//!     a: session.waiting
//!     b: claude-code.idle-stop
//!     asked: 2026-11-04
//!     where: the session that added 20261104-3fa1c2
//!
//!     Keep the session open, or close it and keep the wait in the log?
//!
//! A clash is settled by an entry with `settles: <clash>`; until then it is
//! open while the fact that raised it still says so.

use super::entry::{self, Entry, Kind};
use super::source::Source;
use anyhow::{Result, anyhow, bail};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Bumped when what the model is asked changes, so every fact is worked
/// out again once.
pub const PROMPT_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize)]
pub struct Standing {
    pub id: String,
    /// current, replaced or contradicted
    pub standing: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Root {
    pub id: String,
    pub project: String,
    pub changes: String,
    pub rule: String,
    /// current, or lags (his word is the rule and the boxes still run the old one)
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lags: Option<String>,
    /// The entry the rule comes from.
    pub from_entry: String,
    pub entries: Vec<Standing>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Version {
    pub fingerprint: String,
    pub at: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exception: Option<String>,
    pub rule: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Fact {
    pub id: String,
    pub project: String,
    pub path: String,
    pub asks: String,
    /// Parents as written in the file.
    pub declared: Vec<String>,
    /// Parents, with the roots that correct this fact added by code.
    pub from: Vec<String>,
    pub versions: Vec<Version>,
    /// Filled by resolve: the fingerprint of its inputs now, and the
    /// version that matches it (None while stale).
    #[serde(skip)]
    pub fingerprint: String,
    #[serde(skip)]
    pub current: Option<usize>,
}

impl Fact {
    pub fn now(&self) -> Option<&Version> {
        self.current.map(|i| &self.versions[i])
    }
    pub fn rule(&self) -> String {
        self.now().map(|v| v.rule.clone()).unwrap_or_default()
    }
    pub fn status(&self) -> String {
        self.now().map(|v| v.status.clone()).unwrap_or_else(|| "stale".into())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Clash {
    pub id: String,
    pub project: String,
    pub path: String,
    pub raised_by: String,
    pub a: String,
    pub b: String,
    pub asked: String,
    #[serde(rename = "where")]
    pub where_: String,
    pub question: String,
    /// Computed: open or settled.
    pub status: String,
    /// The entry that settled it, when one did.
    pub answer_entry: Option<String>,
}

pub struct Graph {
    pub as_of: Option<String>,
    pub entries: Vec<Entry>,
    pub roots: BTreeMap<String, Root>,
    pub facts: BTreeMap<String, Fact>,
    pub clashes: BTreeMap<String, Clash>,
    /// Facts in an order where every parent comes first.
    pub order: Vec<String>,
}

pub enum NodeKind {
    Root,
    Fact,
    Clash,
}

impl Graph {
    pub fn kind(&self, id: &str) -> Option<NodeKind> {
        if self.roots.contains_key(id) {
            Some(NodeKind::Root)
        } else if self.facts.contains_key(id) {
            Some(NodeKind::Fact)
        } else if self.clashes.contains_key(id) {
            Some(NodeKind::Clash)
        } else {
            None
        }
    }

    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn project_of(&self, id: &str) -> Option<String> {
        self.roots
            .get(id)
            .map(|r| r.project.clone())
            .or_else(|| self.facts.get(id).map(|f| f.project.clone()))
            .or_else(|| self.clashes.get(id).map(|c| c.project.clone()))
    }

    /// A node's rule as compiled files show it.
    pub fn rule(&self, id: &str) -> Option<String> {
        if let Some(r) = self.roots.get(id) {
            return Some(match &r.lags {
                Some(l) => format!("{} (The boxes still lag: {l})", r.rule),
                None => r.rule.clone(),
            });
        }
        if let Some(f) = self.facts.get(id) {
            return Some(f.rule());
        }
        self.clashes.get(id).map(|c| c.question.clone())
    }

    pub fn status(&self, id: &str) -> Option<String> {
        if let Some(r) = self.roots.get(id) {
            return Some(r.status.clone());
        }
        if let Some(f) = self.facts.get(id) {
            return Some(f.status());
        }
        self.clashes.get(id).map(|c| c.status.clone())
    }

    /// Direct parents of a node.
    pub fn parents(&self, id: &str) -> Vec<String> {
        if let Some(f) = self.facts.get(id) {
            return f.from.clone();
        }
        if let Some(c) = self.clashes.get(id) {
            return vec![c.raised_by.clone(), c.a.clone(), c.b.clone()];
        }
        vec![]
    }

    /// Direct children of a node: facts that name it, clashes on it.
    pub fn children(&self, id: &str) -> Vec<String> {
        let mut v: Vec<String> = self.facts.values().filter(|f| f.from.iter().any(|p| p == id)).map(|f| f.id.clone()).collect();
        v.extend(
            self.clashes
                .values()
                .filter(|c| c.raised_by == id || c.a == id || c.b == id)
                .map(|c| c.id.clone()),
        );
        v.sort();
        v.dedup();
        v
    }

    /// Every node below `id`.
    pub fn descendants(&self, id: &str) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut todo = vec![id.to_string()];
        while let Some(n) = todo.pop() {
            for c in self.children(&n) {
                if seen.insert(c.clone()) {
                    todo.push(c);
                }
            }
        }
        seen
    }

    /// Every root above `id`, with `id` itself if it is one.
    pub fn roots_under(&self, id: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut todo = vec![id.to_string()];
        while let Some(n) = todo.pop() {
            if !seen.insert(n.clone()) {
                continue;
            }
            if self.roots.contains_key(&n) {
                out.insert(n.clone());
            }
            todo.extend(self.parents(&n));
        }
        out
    }
}

// ------------------------------------------------------------------ load

pub fn load(src: &Source, as_of: Option<&str>) -> Result<Graph> {
    let mut entries = Vec::new();
    let mut errs = Vec::new();
    for p in src.list("graph/entries") {
        if !p.ends_with(".md") {
            continue;
        }
        let text = src.read(&p).unwrap_or_default();
        match entry::parse(&p, &text) {
            Ok(e) => entries.push(e),
            Err(e) => errs.push(format!("{e:#}")),
        }
    }
    if !errs.is_empty() {
        bail!("{}", errs.join("\n"));
    }
    let mut ids = BTreeSet::new();
    for e in &entries {
        if !ids.insert(e.id.clone()) {
            bail!("two entries share the id {}", e.id);
        }
    }
    if let Some(d) = as_of {
        entries.retain(|e| e.date.as_str() <= d && e.in_force.as_str() <= d);
    }
    entries.sort_by(|a, b| (&a.in_force, &a.date, &a.id).cmp(&(&b.in_force, &b.date, &b.id)));
    let roots = fold(&entries)?;

    let mut facts = BTreeMap::new();
    for p in src.list("graph/facts") {
        if !p.ends_with(".md") {
            continue;
        }
        let f = parse_fact(&p, &src.read(&p).unwrap_or_default())?;
        if facts.insert(f.id.clone(), f).is_some() {
            bail!("two fact files define the same fact ({p})");
        }
    }
    let mut clashes = BTreeMap::new();
    for p in src.list("graph/clashes") {
        if !p.ends_with(".md") {
            continue;
        }
        let c = parse_clash(&p, &src.read(&p).unwrap_or_default())?;
        if let Some(d) = as_of {
            if c.asked.as_str() > d {
                continue;
            }
        }
        clashes.insert(c.id.clone(), c);
    }

    // Roots whose entries correct a fact become its parents.
    for e in &entries {
        if let Some(target) = &e.corrects {
            if let Some(f) = facts.get_mut(target) {
                if !f.from.contains(&e.about) {
                    f.from.push(e.about.clone());
                }
            }
        }
    }
    let mut g = Graph { as_of: as_of.map(String::from), entries, roots, facts, clashes, order: vec![] };
    // A fact whose parents are not there yet (as of a past day) is not there either.
    if as_of.is_some() {
        loop {
            let gone: Vec<String> = g
                .facts
                .values()
                .filter(|f| f.from.iter().any(|p| g.kind(p).is_none()))
                .map(|f| f.id.clone())
                .collect();
            if gone.is_empty() {
                break;
            }
            for id in gone {
                g.facts.remove(&id);
            }
        }
        let keep: BTreeSet<String> = g.facts.keys().cloned().collect();
        g.clashes.retain(|_, c| keep.contains(&c.raised_by));
    }
    g.order = topo(&g)?;
    Ok(g)
}

/// Code's half: one root per subject, every entry marked current, replaced
/// or contradicted, and one current rule.
pub fn fold(entries: &[Entry]) -> Result<BTreeMap<String, Root>> {
    let by_id: HashMap<&str, &Entry> = entries.iter().map(|e| (e.id.as_str(), e)).collect();
    let mut replaced: HashMap<&str, &str> = HashMap::new();
    let mut contradicted: HashMap<&str, &str> = HashMap::new();
    for e in entries {
        if let Some(r) = &e.replaces {
            if !by_id.contains_key(r.as_str()) {
                bail!("{} replaces {r}, which is not an entry", e.id);
            }
            replaced.insert(r.as_str(), e.id.as_str());
        }
        if let Some(c) = &e.contradicts {
            if !by_id.contains_key(c.as_str()) {
                bail!("{} contradicts {c}, which is not an entry", e.id);
            }
            contradicted.insert(c.as_str(), e.id.as_str());
        }
    }
    let mut subjects: BTreeMap<String, Vec<&Entry>> = BTreeMap::new();
    for e in entries {
        subjects.entry(e.about.clone()).or_default().push(e);
    }
    let mut roots = BTreeMap::new();
    for (subject, es) in subjects {
        let standing = |e: &Entry| -> (String, Option<String>) {
            if let Some(by) = replaced.get(e.id.as_str()) {
                ("replaced".into(), Some(by.to_string()))
            } else if let Some(by) = contradicted.get(e.id.as_str()) {
                ("contradicted".into(), Some(by.to_string()))
            } else {
                ("current".into(), None)
            }
        };
        let list: Vec<Standing> = es
            .iter()
            .map(|e| {
                let (s, by) = standing(e);
                Standing { id: e.id.clone(), standing: s, by }
            })
            .collect();
        let live: Vec<&&Entry> = es.iter().filter(|e| standing(e).0 == "current").collect();
        let top: &Entry = live
            .iter()
            .rev()
            .find(|e| e.kind == Kind::Said)
            .or_else(|| live.last())
            .map(|e| **e)
            .or_else(|| es.last().copied())
            .ok_or_else(|| anyhow!("subject {subject} has no entries"))?;
        // His word against what is seen: the rule is his, and the boxes lag
        // while the seen it contradicts stands unreplaced.
        let mut lags = None;
        for e in &live {
            if let Some(c) = &e.contradicts {
                if let Some(seen) = by_id.get(c.as_str()) {
                    if seen.kind == Kind::Seen && !replaced.contains_key(c.as_str()) {
                        lags = Some(entry::one_para(&seen.body));
                    }
                }
            }
        }
        let changes = es
            .iter()
            .rev()
            .find_map(|e| e.changes.clone())
            .unwrap_or_else(|| "sometimes".into());
        roots.insert(
            subject.clone(),
            Root {
                id: subject,
                project: es[0].project.clone(),
                changes,
                rule: top.sentence(),
                status: if lags.is_some() { "lags".into() } else { "current".into() },
                lags,
                from_entry: top.id.clone(),
                entries: list,
            },
        );
    }
    Ok(roots)
}

fn header_and_body(text: &str) -> (&str, &str) {
    match text.split_once("\n\n") {
        Some((h, b)) => (h, b),
        None => (text, ""),
    }
}

fn header(path: &str, head: &str) -> Result<BTreeMap<String, String>> {
    let mut m = BTreeMap::new();
    for line in head.lines() {
        let Some((k, v)) = line.split_once(':') else {
            bail!("{path}: header line is not `key: value`: {line}");
        };
        m.insert(k.trim().to_string(), v.trim().to_string());
    }
    Ok(m)
}

fn project_of(path: &str, dir: &str) -> String {
    path.strip_prefix(dir).and_then(|r| r.split('/').next()).unwrap_or("org").to_string()
}

pub fn split_list(s: &str) -> Vec<String> {
    s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
}

pub fn parse_fact(path: &str, text: &str) -> Result<Fact> {
    let (head, body) = header_and_body(text);
    let h = header(path, head)?;
    let id = h.get("fact").cloned().ok_or_else(|| anyhow!("{path}: no `fact:` line"))?;
    if !entry::valid_node(&id) || id.starts_with("clash/") {
        bail!("{path}: `{id}` is not a fact id");
    }
    let from = split_list(h.get("from").map(String::as_str).unwrap_or(""));
    if from.is_empty() {
        bail!("{path}: a fact needs `from:`, its parents");
    }
    let mut versions = Vec::new();
    let mut cur: Option<Version> = None;
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            if let Some(v) = cur.take() {
                versions.push(v);
            }
            let parts: Vec<&str> = rest.split(" · ").map(str::trim).collect();
            if parts.len() < 3 {
                bail!("{path}: a version heading is `## <fingerprint> · <date> · <status>[ · clash/…][ · exception: …]`");
            }
            let mut v = Version {
                fingerprint: parts[0].into(),
                at: parts[1].into(),
                status: parts[2].into(),
                clash: None,
                exception: None,
                rule: String::new(),
            };
            for p in &parts[3..] {
                if p.starts_with("clash/") {
                    v.clash = Some(p.to_string());
                } else if let Some(x) = p.strip_prefix("exception: ") {
                    v.exception = Some(x.to_string());
                }
            }
            cur = Some(v);
        } else if let Some(v) = cur.as_mut() {
            if !v.rule.is_empty() || !line.trim().is_empty() {
                if !v.rule.is_empty() {
                    v.rule.push('\n');
                }
                v.rule.push_str(line);
            }
        }
    }
    if let Some(v) = cur.take() {
        versions.push(v);
    }
    for v in &mut versions {
        v.rule = v.rule.trim_end().to_string();
    }
    Ok(Fact {
        id,
        project: project_of(path, "graph/facts/"),
        path: path.into(),
        asks: h.get("asks").cloned().unwrap_or_default(),
        declared: from.clone(),
        from,
        versions,
        fingerprint: String::new(),
        current: None,
    })
}

pub fn render_fact(f: &Fact) -> String {
    let mut s = format!("fact: {}\nasks: {}\nfrom: {}\n", f.id, entry::one_para(&f.asks), f.declared.join(", "));
    for v in &f.versions {
        let mut head = format!("{} · {} · {}", v.fingerprint, v.at, v.status);
        if let Some(c) = &v.clash {
            head.push_str(&format!(" · {c}"));
        }
        if let Some(x) = &v.exception {
            head.push_str(&format!(" · exception: {}", entry::one_para(x)));
        }
        s.push_str(&format!("\n## {head}\n{}\n", v.rule.trim_end()));
    }
    s
}

pub fn parse_clash(path: &str, text: &str) -> Result<Clash> {
    let (head, body) = header_and_body(text);
    let h = header(path, head)?;
    let get = |k: &str| h.get(k).cloned().ok_or_else(|| anyhow!("{path}: no `{k}:` line"));
    let id = get("clash")?;
    if !id.starts_with("clash/") || !entry::valid_node(&id) {
        bail!("{path}: `{id}` is not a clash id");
    }
    Ok(Clash {
        id,
        project: project_of(path, "graph/clashes/"),
        path: path.into(),
        raised_by: get("raised-by")?,
        a: get("a")?,
        b: get("b")?,
        asked: get("asked")?,
        where_: h.get("where").cloned().unwrap_or_default(),
        question: body.trim().to_string(),
        status: "open".into(),
        answer_entry: None,
    })
}

pub fn render_clash(c: &Clash) -> String {
    format!(
        "clash: {}\nraised-by: {}\na: {}\nb: {}\nasked: {}\nwhere: {}\n\n{}\n",
        c.id,
        c.raised_by,
        c.a,
        c.b,
        c.asked,
        entry::one_para(&c.where_),
        c.question.trim()
    )
}

pub fn clash_path(project: &str, id: &str) -> String {
    format!("graph/clashes/{project}/{}.md", id.trim_start_matches("clash/"))
}

pub fn fact_path(project: &str, id: &str) -> String {
    format!("graph/facts/{project}/{id}.md")
}

/// Facts ordered parents first; a cycle or a parent that doesn't exist is refused.
fn topo(g: &Graph) -> Result<Vec<String>> {
    let mut order = Vec::new();
    let mut state: HashMap<String, u8> = HashMap::new();
    fn visit(g: &Graph, id: &str, state: &mut HashMap<String, u8>, order: &mut Vec<String>, path: &mut Vec<String>) -> Result<()> {
        match state.get(id) {
            Some(2) => return Ok(()),
            Some(1) => {
                path.push(id.to_string());
                bail!("the rules graph has a cycle: {}", path.join(" -> "));
            }
            _ => {}
        }
        let f = &g.facts[id];
        state.insert(id.to_string(), 1);
        path.push(id.to_string());
        for p in &f.from {
            if g.facts.contains_key(p) {
                visit(g, p, state, order, path)?;
            } else if !g.roots.contains_key(p) {
                bail!("fact {id} names {p}, which is neither a root nor a fact");
            }
        }
        path.pop();
        state.insert(id.to_string(), 2);
        order.push(id.to_string());
        Ok(())
    }
    for id in g.facts.keys() {
        visit(g, id, &mut state, &mut order, &mut vec![])?;
    }
    Ok(order)
}

// --------------------------------------------------------------- resolve

/// What the model sees for one fact, and nothing else: the question the
/// fact answers, its parents, and any of Felix's answers to clashes
/// between them.
pub fn inputs(g: &Graph, f: &Fact) -> Value {
    let mut parents: Vec<Value> = f
        .from
        .iter()
        .map(|p| {
            if let Some(r) = g.roots.get(p) {
                let e = g.entry(&r.from_entry);
                json!({
                    "id": p,
                    "kind": "root",
                    "rule": r.rule,
                    "status": r.status,
                    "lags": r.lags,
                    "from": e.map(|e| e.quote()),
                    "changes": r.changes,
                })
            } else {
                let pf = &g.facts[p];
                json!({"id": p, "kind": "fact", "rule": pf.rule(), "status": pf.status()})
            }
        })
        .collect();
    parents.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    let names: BTreeSet<&str> = f.from.iter().map(String::as_str).collect();
    let settled: Vec<Value> = g
        .clashes
        .values()
        .filter(|c| c.status == "settled" && c.answer_entry.is_some())
        .filter(|c| c.raised_by == f.id || (names.contains(c.a.as_str()) && names.contains(c.b.as_str())))
        .map(|c| {
            let e = c.answer_entry.as_deref().and_then(|id| g.entry(id));
            json!({
                "clash": c.id, "a": c.a, "b": c.b, "question": c.question,
                "answer": e.map(|e| e.quote()), "rule": e.map(|e| e.sentence()),
            })
        })
        .collect();
    json!({"v": PROMPT_VERSION, "fact": f.id, "asks": f.asks, "parents": parents, "settled": settled})
}

pub fn fingerprint(v: &Value) -> String {
    entry::sha256_hex(&v.to_string())[..12].to_string()
}

/// One answer from the model for one fact.
#[derive(Clone, Debug)]
pub struct Answer {
    pub status: String,
    pub rule: String,
    pub exception: Option<String>,
    pub clash: Option<(String, String, String, String)>, // (name, a, b, question)
}

pub trait Model {
    fn fact(&mut self, input: &Value) -> Result<Answer>;
}

#[derive(Default, Debug)]
pub struct Report {
    pub calls: usize,
    /// Facts worked out again whose answer changed.
    pub changed: Vec<String>,
    /// Facts worked out again that came out the same (the work stops there).
    pub same: Vec<String>,
    /// Facts whose inputs have no stored answer and no model was allowed.
    pub stale: Vec<String>,
    pub new_clashes: Vec<String>,
    /// Files the resolve wrote or would write.
    pub writes: Vec<(String, String)>,
}

/// Clash status from the entries: settled when an entry settles it, else
/// open while the fact that raised it still names it.
fn settle_clashes(g: &mut Graph) {
    let settles: HashMap<String, String> =
        g.entries.iter().filter_map(|e| e.settles.clone().map(|c| (c, e.id.clone()))).collect();
    let ids: Vec<String> = g.clashes.keys().cloned().collect();
    for id in ids {
        let by = settles.get(&id).cloned();
        let raised_now = g.facts.get(&g.clashes[&id].raised_by).and_then(|f| f.now()).and_then(|v| v.clash.clone());
        let c = g.clashes.get_mut(&id).unwrap();
        if let Some(e) = by {
            c.status = "settled".into();
            c.answer_entry = Some(e);
        } else if raised_now.as_deref() == Some(id.as_str()) {
            c.status = "open".into();
            c.answer_entry = None;
        } else {
            // The sides no longer conflict, though nobody answered.
            c.status = "settled".into();
            c.answer_entry = None;
        }
    }
}

/// Work out every fact, parents first. A fact whose inputs have a stored
/// answer reuses it; otherwise the model is asked (when one is given). A
/// fact that comes out the same leaves its children's inputs unchanged,
/// so they reuse theirs: that is the early stop.
pub fn resolve(g: &mut Graph, mut model: Option<&mut dyn Model>, today: &str, where_: &str) -> Result<Report> {
    let mut rep = Report::default();
    settle_clashes(g);
    let order = g.order.clone();
    for id in order {
        let input = inputs(g, &g.facts[&id]);
        let fp = fingerprint(&input);
        let found = g.facts[&id].versions.iter().position(|v| v.fingerprint == fp);
        if let Some(i) = found {
            let f = g.facts.get_mut(&id).unwrap();
            f.fingerprint = fp;
            f.current = Some(i);
            settle_clashes(g);
            continue;
        }
        let Some(m) = model.as_deref_mut() else {
            let f = g.facts.get_mut(&id).unwrap();
            f.fingerprint = fp;
            f.current = None;
            rep.stale.push(id);
            continue;
        };
        rep.calls += 1;
        let ans = m.fact(&input)?;
        let prev = g.facts[&id].versions.last().map(|v| (v.rule.clone(), v.status.clone()));
        let mut clash_id = None;
        if let Some((name, a, b, q)) = &ans.clash {
            clash_id = Some(open_clash(g, &id, name, a, b, q, today, where_, &mut rep)?);
        }
        let status = if clash_id.is_some() && !["open", "gone"].contains(&ans.status.as_str()) {
            "open".to_string()
        } else {
            ans.status.clone()
        };
        let v = Version {
            fingerprint: fp.clone(),
            at: today.into(),
            status,
            clash: clash_id,
            exception: ans.exception.clone(),
            rule: ans.rule.trim().to_string(),
        };
        let same = prev.as_ref().map(|(r, s)| r == &v.rule && s == &v.status).unwrap_or(false);
        let f = g.facts.get_mut(&id).unwrap();
        f.versions.push(v);
        f.current = Some(f.versions.len() - 1);
        f.fingerprint = fp;
        rep.writes.push((f.path.clone(), render_fact(f)));
        if same {
            rep.same.push(id.clone());
        } else {
            rep.changed.push(id.clone());
        }
        settle_clashes(g);
    }
    Ok(rep)
}

/// The clash for two sides: the one already on file for the same pair
/// (open or settled, so a settled one is never asked again), else a new one.
#[allow(clippy::too_many_arguments)]
fn open_clash(g: &mut Graph, fact: &str, name: &str, a: &str, b: &str, q: &str, today: &str, where_: &str, rep: &mut Report) -> Result<String> {
    let pair = |x: &str, y: &str| if x <= y { (x.to_string(), y.to_string()) } else { (y.to_string(), x.to_string()) };
    let want = pair(a, b);
    if let Some(c) = g.clashes.values().find(|c| pair(&c.a, &c.b) == want) {
        return Ok(c.id.clone());
    }
    let mut slug: String = name
        .trim_start_matches("clash/")
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug = slug.trim_matches('-');
    let mut id = format!("clash/{}", if slug.is_empty() { "unnamed" } else { slug });
    let mut n = 2;
    while g.clashes.contains_key(&id) {
        id = format!("clash/{slug}-{n}");
        n += 1;
    }
    let project = g.facts[fact].project.clone();
    let c = Clash {
        id: id.clone(),
        project: project.clone(),
        path: clash_path(&project, &id),
        raised_by: fact.into(),
        a: a.into(),
        b: b.into(),
        asked: today.into(),
        where_: where_.into(),
        question: q.trim().into(),
        status: "open".into(),
        answer_entry: None,
    };
    rep.writes.push((c.path.clone(), render_clash(&c)));
    rep.new_clashes.push(id.clone());
    g.clashes.insert(id.clone(), c);
    Ok(id)
}

// ------------------------------------------------------------ the model

/// The model, as a cheap `claude -p` with no tools, that sees one fact's
/// inputs and answers in JSON. ABOT_RULES_MODEL picks it (default sonnet).
pub struct Claude {
    pub model: String,
}

impl Claude {
    pub fn from_env() -> Claude {
        let model = std::env::var("ABOT_RULES_MODEL").ok().filter(|m| !m.trim().is_empty()).unwrap_or_else(|| "sonnet".into());
        Claude { model }
    }
}

const FACT_SYSTEM: &str = "\
You work out one fact in a graph of rules for the AI agents that work for Felix.
You see only the fact's question and its parents: roots (one subject each, with
the rule from Felix's own words, or what is seen on the machines) and other facts.
Answer the question from the parents and nothing else. Never invent a rule the
parents don't give.

Status:
- current: the parents give a clear answer.
- open: two parents can't both hold. First ask whether one is a narrower
  exception to the other (the specific beats the general): if so, it is not a
  clash; answer `current` and say the exception in `exception`. If not, it is a
  clash: name the two parents in clash.a and clash.b, ask Felix one short
  question that settles it, and write as `rule` what agents do until he answers
  (follow his word, and say the risk).
- gone: the parents no longer give this fact anything to say.
A root whose status is `lags` is Felix's word that the machines have not caught
up with: the rule is his word.
When `settled` lists Felix's answer to a clash between these parents, follow
it; that clash is not raised again.
Write `rule` as one to three plain sentences an agent can follow, in plain
English, no dates or quotes of Felix (those live in the roots).";

fn fact_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "status": {"type": "string", "enum": ["current", "open", "gone"]},
            "rule": {"type": "string"},
            "exception": {"type": "string"},
            "clash": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "a": {"type": "string"},
                    "b": {"type": "string"},
                    "question": {"type": "string"}
                },
                "required": ["name", "a", "b", "question"]
            }
        },
        "required": ["status", "rule"]
    })
}

pub fn ask_json(model: &str, system: &str, schema: &Value, prompt: &str) -> Result<Value> {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let bin = std::env::var("ABOT_CLAUDE").ok().filter(|m| !m.trim().is_empty()).unwrap_or_else(|| "claude".into());
    let dir = std::env::temp_dir();
    let mut c = Command::new(&bin);
    c.current_dir(&dir)
        .args(["-p", "--model", model, "--effort", "low", "--tools", "", "--setting-sources", "", "--strict-mcp-config"])
        .args(["--system-prompt", system, "--output-format", "json", "--json-schema", &schema.to_string()])
        .arg(prompt)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = c.spawn().map_err(|e| anyhow!("could not run `{bin}` for the rules model: {e}"))?;
    let start = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if start.elapsed() > Duration::from_secs(240) {
            let _ = child.kill();
            bail!("the rules model took over 240s and was stopped");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let o = child.wait_with_output()?;
    let out = String::from_utf8_lossy(&o.stdout);
    let v: Value = serde_json::from_str(out.trim()).map_err(|_| {
        anyhow!(
            "the rules model's answer was not JSON (exit {}): {}",
            o.status,
            crate::out::clip(&format!("{} {}", out.trim(), String::from_utf8_lossy(&o.stderr).trim()), 400)
        )
    })?;
    if v["is_error"].as_bool() == Some(true) {
        bail!("the rules model failed: {}", crate::out::clip(&v["result"].to_string(), 400));
    }
    Ok(if v["structured_output"].is_object() {
        v["structured_output"].clone()
    } else {
        serde_json::from_str(v["result"].as_str().unwrap_or("")).unwrap_or(v)
    })
}

impl Model for Claude {
    fn fact(&mut self, input: &Value) -> Result<Answer> {
        let prompt = format!("The fact and its parents:\n{}", serde_json::to_string_pretty(input)?);
        let v = ask_json(&self.model, FACT_SYSTEM, &fact_schema(), &prompt)?;
        answer_of(&v, input)
    }
}

pub fn answer_of(v: &Value, input: &Value) -> Result<Answer> {
    let s = |x: &Value| x.as_str().unwrap_or("").trim().to_string();
    let status = s(&v["status"]);
    if !["current", "open", "gone"].contains(&status.as_str()) {
        bail!("the rules model gave no status: {}", crate::out::clip(&v.to_string(), 300));
    }
    let parents: BTreeSet<String> =
        input["parents"].as_array().map(|a| a.iter().map(|p| s(&p["id"])).collect()).unwrap_or_default();
    let clash = if v["clash"].is_object() {
        let (a, b) = (s(&v["clash"]["a"]), s(&v["clash"]["b"]));
        if !parents.contains(&a) || !parents.contains(&b) || a == b {
            bail!("the rules model named a clash between {a} and {b}, which are not two of the fact's parents");
        }
        Some((s(&v["clash"]["name"]), a, b, s(&v["clash"]["question"])))
    } else {
        None
    };
    let exception = Some(s(&v["exception"])).filter(|x| !x.is_empty());
    Ok(Answer { status, rule: s(&v["rule"]), exception, clash })
}
