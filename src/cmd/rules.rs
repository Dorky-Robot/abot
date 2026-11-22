//! The rules graph's verbs: entry add, fact add, rules build/check, why,
//! affects. See src/rules/ for the graph itself.

use crate::ctx::{Ctx, here};
use crate::git::Git;
use crate::rules::entry::{self, Entry, Kind};
use crate::rules::graph::{self, Graph};
use crate::rules::source::Source;
use crate::rules::{self, compile};
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub const NOT_YET: u8 = 2;

#[derive(Default, Clone)]
pub struct EntryArgs {
    pub said: Option<String>,
    pub seen: Option<String>,
    pub proof: Option<String>,
    pub about: Option<String>,
    pub rule: Option<String>,
    pub by: Option<String>,
    pub date: Option<String>,
    pub in_force: Option<String>,
    pub project: Option<String>,
    pub replaces: Option<String>,
    pub contradicts: Option<String>,
    pub corrects: Option<String>,
    pub settles: Option<String>,
    pub changes: Option<String>,
    pub why: Option<String>,
    pub where_: Option<String>,
    pub id: Option<String>,
    pub accept: bool,
    pub no_build: bool,
    pub no_commit: bool,
    pub no_model: bool,
    pub json: bool,
    pub message: Option<String>,
}

/// The session working: its name, for `where:` and the clash's `asked`.
fn session_name() -> String {
    std::env::var("ABOT_SESSION_NAME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("CLAUDE_CODE_SESSION_NAME").ok().filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| "a session".into())
}

/// Where a verb reads the graph: the task worktree it stands in, else origin's main.
fn read_source(ctx: &Ctx) -> Result<Source> {
    if let Ok(h) = here(ctx) {
        return Ok(Source::Dir(h.path));
    }
    if let Some(why) = ctx.read_from_origin()? {
        eprintln!("note: {why}");
    }
    Ok(Source::Rev(ctx.log_git(), ctx.base()))
}

fn model_for(no_model: bool) -> Option<graph::Claude> {
    (!no_model).then(graph::Claude::from_env)
}

// ------------------------------------------------------------- entry add

pub fn entry_add(ctx: &Ctx, a: EntryArgs) -> Result<bool> {
    let (kind, body) = match (&a.said, &a.seen) {
        (Some(w), None) => (Kind::Said, w.clone()),
        (None, Some(w)) => (Kind::Seen, w.clone()),
        _ => bail!("say what the entry is: --said \"<his words, verbatim>\" or --seen \"<what is on the boxes>\" --proof …"),
    };
    if body.trim().is_empty() {
        bail!("the words are empty");
    }
    let today = super::today();
    let date = a.date.clone().unwrap_or_else(|| today.clone());
    let by = a.by.clone().unwrap_or_else(|| if kind == Kind::Said { "Felix".into() } else { session_name() });
    let mut e = Entry {
        id: String::new(),
        project: String::new(),
        path: String::new(),
        kind: kind.clone(),
        by,
        date: date.clone(),
        in_force: a.in_force.clone().unwrap_or_else(|| date.clone()),
        about: a.about.clone().unwrap_or_default(),
        body,
        rule: a.rule.clone(),
        proof: a.proof.clone(),
        replaces: a.replaces.clone(),
        contradicts: a.contradicts.clone(),
        corrects: a.corrects.clone(),
        settles: a.settles.clone(),
        changes: a.changes.clone(),
        why: a.why.clone(),
        where_: Some(a.where_.clone().unwrap_or_else(session_name)),
    };

    // What the model proposes, when the session left something out.
    let src = read_source(ctx)?;
    let g = graph::load(&src, None)?;
    let needs = e.about.is_empty() || (kind == Kind::Said && e.rule.is_none());
    let mut new_facts: Vec<(String, String, Vec<String>, String)> = vec![];
    if needs {
        if a.no_model {
            bail!("--about (and, for a said entry, --rule) are needed when the model isn't asked");
        }
        let p = propose(&g, &e)?;
        let s = |v: &Value| v.as_str().unwrap_or("").trim().to_string();
        if e.about.is_empty() {
            e.about = s(&p["about"]);
        }
        if e.rule.is_none() && kind == Kind::Said {
            e.rule = Some(s(&p["rule"])).filter(|r| !r.is_empty());
        }
        let rel = s(&p["relation"]);
        let target = s(&p["entry"]);
        if !target.is_empty() && e.replaces.is_none() && e.contradicts.is_none() {
            match rel.as_str() {
                "replaces" => e.replaces = Some(target),
                "contradicts" => e.contradicts = Some(target),
                _ => {}
            }
        }
        if e.changes.is_none() && !g.roots.contains_key(&e.about) {
            e.changes = Some(s(&p["changes"])).filter(|c| entry::CHANGES.contains(&c.as_str()));
        }
        for f in p["facts"].as_array().cloned().unwrap_or_default() {
            let from: Vec<String> = f["from"].as_array().map(|x| x.iter().map(s).collect()).unwrap_or_default();
            new_facts.push((s(&f["id"]), s(&f["asks"]), from, s(&f["project"])));
        }
        if !a.accept {
            print_proposal(&e, &new_facts, a.json);
            return Ok(false);
        }
    }
    // The project: --project, else the subject's root, else its first word
    // when that is a project, else org.
    e.project = a
        .project
        .clone()
        .or_else(|| g.roots.get(&e.about).map(|r| r.project.clone()))
        .or_else(|| {
            let first = e.about.split('.').next().unwrap_or("");
            ctx.projects().contains(&first.to_string()).then(|| first.to_string())
        })
        .unwrap_or_else(|| "org".into());
    entry::lint(&e).map_err(|w| anyhow!("{w}"))?;
    for (k, r) in [("replaces", &e.replaces), ("contradicts", &e.contradicts)] {
        if let Some(r) = r {
            if g.entry(r).is_none() {
                bail!("--{k} {r}: no such entry");
            }
        }
    }
    if let Some(c) = &e.corrects {
        if g.kind(c).is_none() {
            bail!("--corrects {c}: no such node in the graph");
        }
    }
    if let Some(c) = &e.settles {
        if !g.clashes.contains_key(c) {
            bail!("--settles {c}: no such clash");
        }
    }
    let draft = entry::render(&Entry { id: String::new(), ..e.clone() });
    e.id = a.id.clone().unwrap_or_else(|| entry::mint(&e.date, &draft));
    if g.entry(&e.id).is_some() {
        bail!("an entry {} is already in the graph", e.id);
    }
    e.path = entry::path_for(&e.project, &e.date, &e.id);
    let text = entry::render(&e);
    let msg = a.message.clone().unwrap_or_else(|| commit_subject(&e));

    let write = |dir: &Path| -> Result<Option<rules::Built>> {
        let s = Source::Dir(dir.to_path_buf());
        s.write(&e.path, &text)?;
        for (id, asks, from, project) in &new_facts {
            define_fact(&s, id, asks, from, if project.is_empty() { &e.project } else { project })?;
        }
        if a.no_build {
            return Ok(None);
        }
        let mut m = model_for(a.no_model);
        let where_ = format!("{}, adding {}", session_name(), e.id);
        Ok(Some(rules::build(dir, m.as_mut().map(|m| m as &mut dyn graph::Model), &today, &where_)?))
    };

    if let Ok(h) = here(ctx) {
        let built = write(&h.path)?;
        if !a.no_commit {
            commit_all(&h.wt, &msg, &e, built.as_ref())?;
        }
        report(&e, built.as_ref(), a.json, None);
        return Ok(true);
    }
    // Not in a task: the entry and its rebuild land on origin's main in one
    // commit, made in a private worktree that is removed afterwards.
    let sha = on_main(ctx, |dir, wt| {
        let built = write(dir)?;
        commit_all(wt, &msg, &e, built.as_ref())?;
        Ok(built)
    })?;
    report(&e, sha.1.as_ref(), a.json, Some(&sha.0));
    Ok(true)
}

fn commit_subject(e: &Entry) -> String {
    let who = match e.kind {
        Kind::Said => format!("{}, {}", e.by, entry::short_date(&e.date)),
        Kind::Seen => format!("Seen {}", entry::short_date(&e.date)),
    };
    let s = format!("{who}, on {}: {}", e.about, entry::one_para(&e.sentence()));
    crate::out::clip(&s, 120)
}

fn commit_all(wt: &Git, msg: &str, e: &Entry, built: Option<&rules::Built>) -> Result<()> {
    // Exactly what this entry wrote: the entry, fact definitions, and what
    // the build wrote or removed; nothing else in the worktree.
    let mut paths: Vec<String> = vec![e.path.clone(), "graph/facts".into()];
    if let Some(b) = built {
        paths.extend(b.report.writes.iter().map(|(p, _)| p.clone()));
        paths.extend(b.rewrote.iter().cloned());
        paths.extend(b.removed.iter().cloned());
        paths.extend(b.state_files.iter().cloned());
    }
    paths.sort();
    paths.dedup();
    let top = wt.toplevel().unwrap_or_else(|| wt.dir.clone());
    let mut args: Vec<&str> = vec!["add", "-A", "--"];
    let keep: Vec<&String> = paths.iter().filter(|p| top.join(p).exists() || wt.ok(&["ls-files", "--error-unmatch", p])).collect();
    args.extend(keep.iter().map(|p| p.as_str()));
    wt.run(&args)?;
    let mut body = format!("{msg}\n\nEntry {} ({}):\n", e.id, e.path);
    for line in e.body.lines() {
        body.push_str(&format!("> {line}\n"));
    }
    if let Some(b) = built {
        body.push_str(&format!(
            "\nModel calls: {}. Worked out again: {}. Came out the same: {}. New clashes: {}. Files rebuilt: {}.\n",
            b.report.calls,
            list_or_none(&b.report.changed),
            list_or_none(&b.report.same),
            list_or_none(&b.report.new_clashes),
            b.rewrote.len()
        ));
    }
    let trailer = std::env::var("ABOT_COMMIT_TRAILER").unwrap_or_default();
    if !trailer.trim().is_empty() {
        body.push_str(&format!("\n{}\n", trailer.trim()));
    }
    wt.run_input(&["commit", "-q", "-F", "-"], &[], &body)?;
    Ok(())
}

fn list_or_none(v: &[String]) -> String {
    if v.is_empty() { "none".into() } else { v.join(", ") }
}

fn report(e: &Entry, b: Option<&rules::Built>, json_out: bool, sha: Option<&str>) {
    if json_out {
        let v = json!({
            "id": e.id, "path": e.path, "about": e.about, "commit": sha,
            "calls": b.map(|b| b.report.calls), "changed": b.map(|b| &b.report.changed),
            "same": b.map(|b| &b.report.same), "new_clashes": b.map(|b| &b.report.new_clashes),
            "rebuilt": b.map(|b| &b.rewrote),
        });
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return;
    }
    println!("entry {} on {} ({})", e.id, e.about, e.path);
    if let Some(b) = b {
        print_built(b);
    }
    if let Some(s) = sha {
        println!("landed on origin/main as {s}");
    }
}

pub fn print_built(b: &rules::Built) {
    let r = &b.report;
    println!("model calls: {}", r.calls);
    if !r.changed.is_empty() {
        println!("worked out again, changed: {}", r.changed.join(", "));
    }
    if !r.same.is_empty() {
        println!("worked out again, same answer (stops there): {}", r.same.join(", "));
    }
    for c in &r.new_clashes {
        let q = b.graph.clashes.get(c).map(|c| c.question.clone()).unwrap_or_default();
        println!("NEW CLASH {c}: {q}\n  ask Felix in this session today; it is listed under Waits on Felix");
    }
    println!("files rebuilt: {}{}", b.rewrote.len(), if b.rewrote.is_empty() { String::new() } else { format!(" ({})", b.rewrote.join(", ")) });
    if !b.removed.is_empty() {
        println!("files removed: {}", b.removed.join(", "));
    }
    if !b.state_files.is_empty() {
        println!("Waits on Felix updated: {}", b.state_files.join(", "));
    }
}

fn print_proposal(e: &Entry, facts: &[(String, String, Vec<String>, String)], json_out: bool) {
    if json_out {
        let v = json!({
            "proposed": true, "about": e.about, "rule": e.rule, "replaces": e.replaces,
            "contradicts": e.contradicts, "changes": e.changes,
            "facts": facts.iter().map(|(id, asks, from, p)| json!({"id": id, "asks": asks, "from": from, "project": p})).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return;
    }
    println!("The model proposes (nothing written yet):");
    println!("  about:       {}", e.about);
    if let Some(r) = &e.rule {
        println!("  rule:        {r}");
    }
    if let Some(r) = &e.replaces {
        println!("  replaces:    {r}");
    }
    if let Some(r) = &e.contradicts {
        println!("  contradicts: {r}");
    }
    if let Some(c) = &e.changes {
        println!("  changes:     {c}");
    }
    for (id, asks, from, _) in facts {
        println!("  fact:        {id} ({}) asks: {asks}", from.join(", "));
    }
    println!("Check it. Run again with --accept to write it as proposed, or give --about/--rule/--replaces yourself.");
}

const PROPOSE_SYSTEM: &str = "\
You file one new entry into a graph of rules for the AI agents that work for Felix.
An entry is Felix's own words (said) or something seen on the machines (seen). Each
entry belongs to one subject (a root): dotted lowercase words, org-wide ones bare
(session.waiting), a project's prefixed with its slug (everyday-vet.ship). You see
the existing subjects with their current rule and current entry ids, and the facts
(rules built from roots and other facts) with their parents.
Propose:
- about: an existing subject when the entry is about the same thing (prefer one),
  else a new one.
- rule: for a said entry, one plain sentence an agent can follow, true to his words.
- relation: replaces (his later word on the same subject, or a later seen on the
  same subject), contradicts (his word against what is seen, same subject), or none;
  entry: the id of the current entry it replaces or contradicts.
- changes: rarely (a principle), sometimes (how we work), often (a fact about the
  machines or tools), for a new subject.
- facts: facts that should now take this subject as a parent: an existing fact id
  with its parents including this subject, or a new fact (id, asks: the question
  it answers, from: its parents). Only when the subject truly bears on it; [] is fine.";

fn propose(g: &Graph, e: &Entry) -> Result<Value> {
    let subjects: Vec<Value> = g
        .roots
        .values()
        .map(|r| {
            let cur: Vec<&str> = r.entries.iter().filter(|s| s.standing == "current").map(|s| s.id.as_str()).collect();
            json!({"id": r.id, "rule": r.rule, "current_entries": cur})
        })
        .collect();
    let facts: Vec<Value> = g.facts.values().map(|f| json!({"id": f.id, "asks": f.asks, "from": f.from, "project": f.project})).collect();
    let prompt = serde_json::to_string_pretty(&json!({
        "new_entry": {
            "kind": if e.kind == Kind::Said { "said" } else { "seen" },
            "date": e.date, "by": e.by, "words": e.body, "proof": e.proof,
            "about_hint": e.about, "rule_hint": e.rule,
        },
        "subjects": subjects, "facts": facts,
    }))?;
    let schema = json!({
        "type": "object",
        "properties": {
            "about": {"type": "string"}, "rule": {"type": "string"},
            "relation": {"type": "string", "enum": ["replaces", "contradicts", "none"]},
            "entry": {"type": "string"}, "changes": {"type": "string", "enum": ["rarely", "sometimes", "often"]},
            "facts": {"type": "array", "items": {"type": "object", "properties": {
                "id": {"type": "string"}, "asks": {"type": "string"},
                "from": {"type": "array", "items": {"type": "string"}}, "project": {"type": "string"}
            }, "required": ["id", "asks", "from"]}}
        },
        "required": ["about", "relation", "facts"]
    });
    graph::ask_json(&graph::Claude::from_env().model, PROPOSE_SYSTEM, &schema, &prompt)
}

/// Write or extend a fact's definition (its question and parents). Its
/// answers are kept; the build works it out again for the new inputs.
fn define_fact(src: &Source, id: &str, asks: &str, from: &[String], project: &str) -> Result<String> {
    if !entry::valid_node(id) || id.starts_with("clash/") {
        bail!("`{id}` is not a fact id (dotted lowercase words)");
    }
    if from.is_empty() {
        bail!("fact {id} needs its parents (--from)");
    }
    // An existing fact keeps its file and its answers.
    let existing = src
        .list("graph/facts")
        .into_iter()
        .find(|p| src.read(p).and_then(|t| graph::parse_fact(p, &t).ok()).map(|f| f.id == id).unwrap_or(false));
    let path = existing.clone().unwrap_or_else(|| graph::fact_path(project, id));
    let mut f = match &existing {
        Some(p) => graph::parse_fact(p, &src.read(p).unwrap_or_default())?,
        None => graph::Fact {
            id: id.into(),
            project: project.into(),
            path: path.clone(),
            asks: asks.into(),
            declared: vec![],
            from: vec![],
            versions: vec![],
            fingerprint: String::new(),
            current: None,
        },
    };
    if !asks.trim().is_empty() {
        f.asks = asks.trim().into();
    }
    for p in from {
        if !f.declared.contains(p) {
            f.declared.push(p.clone());
        }
    }
    src.write(&path, &graph::render_fact(&f))?;
    Ok(path)
}

pub fn fact_add(ctx: &Ctx, id: &str, asks: &str, from: &[String], project: Option<&str>, no_build: bool, no_model: bool) -> Result<bool> {
    let h = here(ctx).map_err(|e| anyhow!("{e:#}\n(a fact is defined inside a log task worktree, then landed)"))?;
    let src = Source::Dir(h.path.clone());
    let g = graph::load(&src, None)?;
    for p in from {
        if g.kind(p).is_none() && p != id {
            bail!("--from {p}: no such root or fact");
        }
    }
    let project = project.map(String::from).unwrap_or_else(|| {
        from.iter().find_map(|p| g.project_of(p)).unwrap_or_else(|| "org".into())
    });
    let path = define_fact(&src, id, asks, from, &project)?;
    println!("fact {id} defined in {path}");
    if !no_build {
        let mut m = model_for(no_model);
        let b = rules::build(&h.path, m.as_mut().map(|m| m as &mut dyn graph::Model), &super::today(), &format!("{}, defining {id}", session_name()))?;
        print_built(&b);
    }
    Ok(true)
}

// --------------------------------------------------------- build, check

pub fn build(ctx: &Ctx, no_model: bool, as_of: Option<&str>, out: Option<&Path>) -> Result<bool> {
    if let Some(d) = as_of {
        if !entry::valid_date(d) {
            bail!("--as-of {d}: say YYYY-MM-DD");
        }
        let src = read_source(ctx)?;
        let mut g = graph::load(&src, Some(d))?;
        let mut m = model_for(no_model);
        let rep = graph::resolve(&mut g, m.as_mut().map(|m| m as &mut dyn graph::Model), &super::today(), "an as-of build")?;
        // Answers worked out for that day are kept, so the next replay of it is free.
        if src.root().is_some() {
            for (p, t) in &rep.writes {
                src.write(p, t)?;
            }
        }
        let layouts = compile::load_layouts(&src)?;
        let o = compile::compile(&g, &src, &layouts, &rules::state_projects(&src))?;
        let dir = out.map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join(format!("abot-rules-as-of-{d}")));
        for (p, t) in &o.files {
            let f = dir.join(p);
            if let Some(par) = f.parent() {
                std::fs::create_dir_all(par)?;
            }
            std::fs::write(&f, t)?;
        }
        println!("the rules as of {d}: {} files in {}", o.files.len(), dir.display());
        println!("model calls: {}{}", rep.calls, if rep.stale.is_empty() { String::new() } else { format!("; no stored answer for {} (run without --no-model to work them out)", rep.stale.join(", ")) });
        return Ok(true);
    }
    let h = here(ctx).map_err(|e| anyhow!("{e:#}\n(build inside a log task worktree; on main, `abot entry add` builds for you)"))?;
    let mut m = model_for(no_model);
    let b = rules::build(&h.path, m.as_mut().map(|m| m as &mut dyn graph::Model), &super::today(), &session_name())?;
    print_built(&b);
    Ok(true)
}

pub fn check(ctx: &Ctx) -> Result<bool> {
    let src = read_source(ctx)?;
    let mut p = rules::check(&src);
    if let Ok(h) = here(ctx) {
        if let Ok(base) = h.wt.run(&["merge-base", "main", "HEAD"]) {
            p.extend(rules::entries_only_added(&h.wt, &base, "HEAD"));
        }
        let dirty = h.wt.try_run(&["status", "--porcelain", "--", "graph/entries"]).unwrap_or_default();
        for l in dirty.lines().filter(|l| !l.starts_with("??") && !l.starts_with("A ")) {
            p.push(format!("{}: an entry is written once; this one is changed in the worktree", l.get(3..).unwrap_or(l)));
        }
    } else {
        p.extend(rules::entries_never_changed(&ctx.log_git(), &ctx.base()));
    }
    for x in export_drift(ctx, &src) {
        println!("note: {x}");
    }
    if p.is_empty() {
        println!("rules graph: ok");
        return Ok(true);
    }
    for x in &p {
        println!("{x}");
    }
    Ok(false)
}

// ----------------------------------------------------------- why, affects

pub fn why(ctx: &Ctx, node: &str, json_out: bool) -> Result<bool> {
    let src = read_source(ctx)?;
    let mut g = graph::load(&src, None)?;
    graph::resolve(&mut g, None, "", "")?;
    if g.kind(node).is_none() {
        bail!("no node {node} in the graph");
    }
    let roots = g.roots_under(node);
    if json_out {
        let v: Vec<Value> = roots
            .iter()
            .map(|r| {
                let root = &g.roots[r];
                json!({"root": r, "rule": root.rule, "entries": root.entries.iter().map(|s| {
                    let e = g.entry(&s.id).unwrap();
                    json!({"id": e.id, "standing": s.standing, "by": s.by, "quote": e.quote(), "path": e.path})
                }).collect::<Vec<_>>()})
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json!({"node": node, "rule": g.rule(node), "status": g.status(node), "rests_on": v}))?);
        return Ok(true);
    }
    println!("{node} ({}): {}", g.status(node).unwrap_or_default(), g.rule(node).unwrap_or_default());
    print_up(&g, node, 1, &mut Default::default());
    Ok(true)
}

fn print_up(g: &Graph, id: &str, depth: usize, seen: &mut std::collections::BTreeSet<String>) {
    let pad = "  ".repeat(depth);
    if let Some(r) = g.roots.get(id) {
        for s in r.entries.iter().rev() {
            let e = g.entry(&s.id).unwrap();
            println!("{pad}{} · {} {}", s.standing, e.id, e.quote());
        }
        return;
    }
    for p in g.parents(id) {
        if !seen.insert(format!("{id}>{p}")) {
            continue;
        }
        println!("{pad}{p} ({}): {}", g.status(&p).unwrap_or_default(), g.rule(&p).unwrap_or_default());
        print_up(g, &p, depth + 1, seen);
    }
}

pub fn affects(ctx: &Ctx, entry_id: &str, json_out: bool) -> Result<bool> {
    let src = read_source(ctx)?;
    let mut g = graph::load(&src, None)?;
    graph::resolve(&mut g, None, "", "")?;
    let e = g.entry(entry_id).ok_or_else(|| anyhow!("no entry {entry_id}"))?.clone();
    let mut nodes = vec![e.about.clone()];
    nodes.extend(g.descendants(&e.about));
    if let Some(c) = &e.corrects {
        nodes.push(c.clone());
        nodes.extend(g.descendants(c));
    }
    nodes.sort();
    nodes.dedup();
    let layouts = compile::load_layouts(&src)?;
    let out = compile::compile(&g, &src, &layouts, &rules::state_projects(&src))?;
    let files: Vec<String> = out
        .shows
        .iter()
        .filter(|(_, ns)| ns.iter().any(|n| nodes.contains(n)))
        .map(|(p, _)| p.clone())
        .collect();
    if json_out {
        println!("{}", serde_json::to_string_pretty(&json!({"entry": entry_id, "nodes": nodes, "files": files}))?);
        return Ok(true);
    }
    println!("{entry_id} is on {}; a change to it works out again:", e.about);
    for n in &nodes {
        println!("  {n} ({})", g.status(n).unwrap_or_default());
    }
    println!("and rebuilds {} file(s):", files.len());
    for f in &files {
        println!("  {f}");
    }
    Ok(true)
}

// ------------------------------------------------------------ on main

/// Run `f` in a private worktree of origin's main, then push its commit to
/// main; if someone landed first, do it again on top of theirs.
fn on_main<F>(ctx: &Ctx, mut f: F) -> Result<(String, Option<rules::Built>)>
where
    F: FnMut(&Path, &Git) -> Result<Option<rules::Built>>,
{
    let g = ctx.require_log()?;
    if !g.has_origin() {
        bail!("the log at {} has no origin, so the entry can't land where everyone reads it", ctx.log.display());
    }
    let common = g.common_dir().unwrap_or_else(|| ctx.log.join(".git"));
    let dir = common.join(format!("abot-entry-{}", std::process::id()));
    let dir_s = dir.display().to_string();
    let mut last = String::new();
    for _ in 0..4 {
        g.run(&["fetch", "-q", "origin", "main"]).map_err(|e| anyhow!("origin can't be reached, so nothing was added: {e:#}"))?;
        let _ = g.run(&["worktree", "remove", "--force", &dir_s]);
        g.run(&["worktree", "add", "-q", "--detach", &dir_s, "refs/remotes/origin/main"])?;
        let wt = Git::new(&dir);
        let r = (|| -> Result<(String, Option<rules::Built>)> {
            let built = f(&dir, &wt)?;
            let sha = wt.run(&["rev-parse", "--short", "HEAD"])?;
            wt.run(&["push", "-q", "origin", "HEAD:refs/heads/main"])?;
            Ok((sha, built))
        })();
        let _ = g.run(&["worktree", "remove", "--force", &dir_s]);
        match r {
            Ok(x) => {
                let _ = g.run(&["fetch", "-q", "origin", "main"]);
                if g.current_branch().as_deref() == Some("main") && !g.is_dirty().unwrap_or(true) {
                    let _ = g.run(&["merge", "-q", "--ff-only", "refs/remotes/origin/main"]);
                }
                return Ok(x);
            }
            Err(e) => {
                last = format!("{e:#}");
                if !last.contains("push") {
                    return Err(e);
                }
            }
        }
    }
    bail!("could not push the entry to origin's main after 4 tries: {last}")
}

// ---------------------------------------------------------------- export

/// The log commit a source reads at, for a commit message.
fn log_rev(src: &Source) -> String {
    match src {
        Source::Dir(p) => Git::new(p).try_run(&["rev-parse", "--short", "HEAD"]),
        Source::Rev(g, r) => g.try_run(&["rev-parse", "--short", r]),
    }
    .unwrap_or_else(|| "?".into())
    .trim()
    .to_string()
}

/// Compile the graph and give back the exported layouts of `project` with
/// their text, refusing a graph with a fact that has no stored answer.
fn exported(src: &Source, project: &str) -> Result<Vec<(compile::Layout, String)>> {
    let mut g = graph::load(src, None)?;
    let r = graph::resolve(&mut g, None, "", "")?;
    if !r.stale.is_empty() {
        bail!("fact(s) with no stored answer for their inputs: {}; run `abot rules build` first", r.stale.join(", "));
    }
    let layouts = compile::load_layouts(src)?;
    let out = compile::compile(&g, src, &layouts, &rules::state_projects(src))?;
    Ok(layouts
        .into_iter()
        .filter(|l| l.export_project() == Some(project))
        .map(|l| {
            let t = out.files.get(&l.to).cloned().unwrap_or_default();
            (l, t)
        })
        .collect())
}

/// Write a project's exported files (its repo's CLAUDE.md, say) into a
/// worktree of its code repo, and commit them there: the compiled file
/// lands in that repo's own commit, and the repo's check refuses a hand
/// edit by the hash in its stamp.
pub fn export(ctx: &Ctx, project: &str, into: &Path, no_commit: bool) -> Result<bool> {
    let src = read_source(ctx)?;
    let files = exported(&src, project)?;
    if files.is_empty() {
        bail!("no layout under graph/layouts/knowledge/{project}/ has an `export:` line, so nothing compiles into {project}'s repo");
    }
    let Some(repo) = ctx.code_repo(project, None)? else {
        bail!("{project} has no code repo here (a `Repo:` line in its state file names it)");
    };
    let wt = Git::new(into);
    let Some(top) = wt.toplevel() else {
        bail!("{} is not inside a git worktree", into.display());
    };
    if wt.common_dir() != Git::new(&repo).common_dir() {
        bail!("{} is not a worktree of {project}'s repo ({})", top.display(), repo.display());
    }
    if crate::git::canon(&top) == crate::git::canon(&repo) {
        bail!(
            "{} is {project}'s main checkout, where landing happens; export into a worktree of a branch and land it",
            top.display()
        );
    }
    let mut changed = vec![];
    for (l, text) in &files {
        let rel = l.export.as_deref().unwrap_or_default();
        let dest = top.join(rel);
        if std::fs::read_to_string(&dest).ok().as_deref() == Some(text.as_str()) {
            println!("{rel}: up to date with {}", l.to);
            continue;
        }
        if let Some(d) = dest.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&dest, text)?;
        println!("{rel}: written from {}", l.to);
        changed.push(rel.to_string());
    }
    if changed.is_empty() || no_commit {
        return Ok(true);
    }
    let mut add = vec!["add", "--"];
    add.extend(changed.iter().map(String::as_str));
    wt.run(&add)?;
    let msg = format!(
        "Compile {} from the rules graph at ecosystem-log {}, because this file is a view of the graph and a hand edit here would drift from it",
        changed.join(", "),
        log_rev(&src)
    );
    let mut commit = vec!["commit", "-q", "-m", &msg, "--"];
    commit.extend(changed.iter().map(String::as_str));
    wt.run(&commit)?;
    println!("committed in {}: {msg}", top.display());
    Ok(true)
}

/// Exported files whose copy on their repo's trunk (as this box has it)
/// is not the log's compile: a note for `abot rules check` and land, never
/// a refusal, since the repo lands on its own and may not be on this box.
pub fn export_drift(ctx: &Ctx, src: &Source) -> Vec<String> {
    let mut out = vec![];
    let Ok(layouts) = compile::load_layouts(src) else { return out };
    let projects: std::collections::BTreeSet<String> =
        layouts.iter().filter_map(|l| l.export_project().map(String::from)).collect();
    for p in projects {
        let Ok(Some(repo)) = ctx.code_repo(&p, None) else { continue };
        let Ok(files) = exported(src, &p) else { continue };
        let g = Git::new(&repo);
        let trunk = g.trunk();
        for (l, text) in files {
            let rel = l.export.unwrap_or_default();
            let have = g.try_raw(&["show", &format!("{trunk}:{rel}")]);
            if have.as_deref() != Some(text.as_str()) {
                out.push(format!(
                    "{rel} on {}'s {trunk} is not the compile of {}; run `abot rules export {p} --into <a worktree of it>` and land it there",
                    repo.display(),
                    l.to
                ));
            }
        }
    }
    out
}
