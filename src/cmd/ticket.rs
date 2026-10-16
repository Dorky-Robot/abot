//! abot ticket: add, answer, step, retire, list. Tickets live in the log at
//! `state/tickets/<project>.md`; every change is one commit on origin's
//! main, made without touching a checkout (onmain.rs).

use super::{flatten, today};
use crate::ctx::Ctx;
use crate::onmain;
use crate::out::clip;
use crate::ticket::{self, Closed, File, Ref, Step, Ticket};
use crate::writer::{self, Draft, Pending, Reply, Round};
use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

/// Exit status for "not filed yet: answer the writer's questions".
pub const NOT_YET: u8 = 2;

fn require_project(ctx: &Ctx, p: &str) -> Result<()> {
    if !crate::branch::valid_slug(p) {
        bail!("`{p}` is not a project slug (a file name in state/projects/, e.g. agent-tooling)");
    }
    if ctx.state_path_at(&ctx.base(), p).is_none() {
        bail!("no project `{p}`: state/projects/{p}.md is not on {}; check the slug (abot map)", ctx.base());
    }
    Ok(())
}

fn file_at(ctx: &Ctx, project: &str) -> File {
    let text = ctx.read_at(&ctx.base(), &ticket::path(project)).unwrap_or_default();
    ticket::parse(project, &text)
}

fn rules(ctx: &Ctx) -> String {
    // The log's how/tickets.md, when it has one, says the rules in its own
    // words; the built-in rules are the floor either way.
    let extra = ctx
        .read_at(&ctx.base(), "how/tickets.md")
        .map(|t| format!("\n\nThe log's own page on tickets (how/tickets.md):\n{t}"))
        .unwrap_or_default();
    format!("{}{extra}", ticket::RULES)
}

fn session_env() -> Option<String> {
    std::env::var("CLAUDE_CODE_BRIDGE_SESSION_ID").ok().filter(|s| !s.trim().is_empty()).map(|s| super::lifecycle::session_id(&s))
}

// ---------------------------------------------------------------- add / answer

pub struct AddArgs {
    pub project: String,
    pub title: String,
    pub why: Option<String>,
    pub done_when: Option<String>,
    pub steps: Vec<String>,
    pub waits_on: Option<String>,
    pub detail: Option<String>,
    pub id: Option<String>,
}

pub fn add(ctx: &Ctx, a: AddArgs) -> Result<bool> {
    ctx.require_log()?;
    if let Some(why) = ctx.read_from_origin()? {
        bail!("not filed: {why}. Tickets are filed on origin, the copy of record");
    }
    require_project(ctx, &a.project)?;
    if a.title.trim().is_empty() {
        bail!("say what the work is: abot ticket add {} \"<title>\" --why … --done-when … --step …", a.project);
    }
    let draft = Draft {
        project: a.project.clone(),
        id: a.id.map(|i| i.trim().to_string()).filter(|i| !i.is_empty()),
        title: a.title.trim().to_string(),
        why: a.why.unwrap_or_default().trim().to_string(),
        done_when: a.done_when.unwrap_or_default().trim().to_string(),
        steps: a.steps.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
        waits_on: a.waits_on.filter(|w| !w.trim().is_empty()),
        detail: a.detail.filter(|d| !d.trim().is_empty()),
    };
    if let Some(id) = &draft.id
        && file_at(ctx, &a.project).ids().any(|x| x == id)
    {
        bail!("{}/{id} is taken (ids are never reused, even retired ones); pick another --id", a.project);
    }
    let state = ctx
        .state_path_at(&ctx.base(), &a.project)
        .and_then(|f| ctx.read_at(&ctx.base(), &f))
        .unwrap_or_default();
    let f = file_at(ctx, &a.project);
    let taken: Vec<String> = f
        .open
        .iter()
        .map(|t| format!("{}: {} (open)", t.id, t.title))
        .chain(f.done.iter().map(|d| format!("{}: {} (done)", d.id, d.title)))
        .chain(f.retired.iter().map(|d| format!("{}: {} (retired)", d.id, d.title)))
        .collect();
    let prompt = writer::first_prompt(&draft, &state, &taken);
    eprintln!("the ticket writer ({}) is reading the draft…", writer::model());
    let t = writer::turn(ctx, None, &rules(ctx), &prompt)?;
    let mut p = Pending { session: t.session.clone(), model: writer::model(), draft, rounds: vec![], filed_by: session_env(), cost_usd: t.cost_usd };
    settle_turn(ctx, &mut p, t.reply)
}

pub fn answer(ctx: &Ctx, session: &str, text: &str) -> Result<bool> {
    ctx.require_log()?;
    if text.trim().is_empty() {
        bail!("say the answers: abot ticket answer {session} \"<answers>\"");
    }
    if let Some(why) = ctx.read_from_origin()? {
        bail!("not filed: {why}. Tickets are filed on origin, the copy of record");
    }
    let mut p = Pending::load(ctx, session)?;
    if let Some(r) = p.rounds.last_mut() {
        r.answered = text.trim().to_string();
    }
    eprintln!("the ticket writer ({}) is reading the answers…", p.model);
    let t = writer::turn(ctx, Some(&p.session), &rules(ctx), &format!("The agent answers:\n{}", text.trim()))?;
    p.cost_usd += t.cost_usd;
    settle_turn(ctx, &mut p, t.reply)
}

/// File what the writer wrote, if the rules hold; otherwise give the agent
/// the questions and keep the conversation for `answer`.
fn settle_turn(ctx: &Ctx, p: &mut Pending, reply: Reply) -> Result<bool> {
    let mut reply = reply;
    if let (Reply::File(t, _), Some(id)) = (&mut reply, &p.draft.id) {
        t.id = id.clone();
    }
    // The rules are checked here too. When the writer files something they
    // refuse, it gets one more turn with the refusal, in the same session.
    if let Reply::File(t, _) = &reply {
        let problems = check_new(ctx, &p.draft.project, t);
        if !problems.is_empty() {
            eprintln!("abot's rules refused the writer's ticket ({}); it gets one more turn…", problems.join("; "));
            let msg = format!(
                "abot's check refused that ticket:\n- {}\nFix it from what you know, or ask the agent.",
                problems.join("\n- ")
            );
            let t2 = writer::turn(ctx, Some(&p.session), &rules(ctx), &msg)?;
            p.cost_usd += t2.cost_usd;
            reply = t2.reply;
            if let Reply::File(t, note) = &reply {
                let again = check_new(ctx, &p.draft.project, t);
                if !again.is_empty() {
                    reply = Reply::Ask(again.iter().map(|x| format!("abot's check: {x}. What should it say?")).collect(), note.clone());
                }
            }
        }
    }
    match reply {
        Reply::Ask(qs, note) => {
            p.rounds.push(Round { asked: qs.clone(), answered: String::new() });
            p.save(ctx)?;
            println!("not filed yet: the ticket writer has questions (session {})", p.session);
            if !note.is_empty() {
                println!("{note}");
            }
            for (i, q) in qs.iter().enumerate() {
                println!("{}. {q}", i + 1);
            }
            println!("answer all of them in one go: abot ticket answer {} \"1. … 2. …\"", short(&p.session));
            Ok(false)
        }
        Reply::File(mut t, note) => {
            // The agent's own --id wins over the writer's.
            if let Some(id) = &p.draft.id {
                t.id = id.clone();
            }
            t.filed = Some(today());
            let project = p.draft.project.clone();
            let msg = filing_message(p, &t, &note);
            let tt = t.clone();
            let sha = onmain::commit_file(ctx, &ticket::path(&project), &msg, |old| {
                let mut f = ticket::parse(&project, old.unwrap_or(""));
                if f.ids().any(|x| x == tt.id) {
                    bail!("{project}/{} was filed by someone else a moment ago; nothing was filed. Run add again with another --id", tt.id);
                }
                f.open.push(tt.clone());
                Ok(ticket::render(&f))
            })?;
            p.forget(ctx);
            println!("filed {project}/{} on main at {sha} (writer: {}, {} round(s), ${:.3})", t.id, p.model, p.rounds.len() + 1, p.cost_usd);
            if !note.is_empty() {
                println!("the writer: {note}");
            }
            println!();
            print!("{}", ticket_block(&t));
            println!("\nwork it on a branch named {project}/{}, or name a step in a commit: Ready: {project}/{}.<n>", t.id, t.id);
            Ok(true)
        }
    }
}

fn short(s: &str) -> &str {
    &s[..8.min(s.len())]
}

/// The rules, plus what only the file knows: the id is free and the
/// project is real.
fn check_new(ctx: &Ctx, project: &str, t: &Ticket) -> Vec<String> {
    let mut out = ticket::lint(t);
    if file_at(ctx, project).ids().any(|x| x == t.id) {
        out.push(format!("id `{}` is already used in {project}; pick another", t.id));
    }
    out
}

pub fn ticket_block(t: &Ticket) -> String {
    let mut f = File { project: String::new(), open: vec![t.clone()], ..File::default() };
    f.open[0].filed = t.filed.clone();
    let r = ticket::render(&f);
    // Just the ticket, without the file's head.
    r.split_once("\n### ").map(|(_, b)| format!("### {b}")).unwrap_or(r)
}

fn filing_message(p: &Pending, t: &Ticket, note: &str) -> String {
    let mut m = format!("Ticket {}/{}: {}\n\n", p.draft.project, t.id, clip(flatten(&t.why).trim_end_matches('.'), 100));
    m.push_str(&ticket_block(t));
    m.push_str(&format!(
        "\nWritten with the ticket writer ({}, session {}, ${:.3}){}.\n",
        p.model,
        p.session,
        p.cost_usd,
        if note.is_empty() { String::new() } else { format!(": {note}") }
    ));
    let d = &p.draft;
    m.push_str("\nThe agent's draft:\n");
    m.push_str(&format!("  title: {}\n", d.title));
    for (k, v) in [("why", &d.why), ("done when", &d.done_when)] {
        m.push_str(&format!("  {k}: {}\n", if v.is_empty() { "(none)" } else { v }));
    }
    for (i, s) in d.steps.iter().enumerate() {
        m.push_str(&format!("  step {}: {s}\n", i + 1));
    }
    if let Some(w) = &d.waits_on {
        m.push_str(&format!("  waits on: {w}\n"));
    }
    for (i, r) in p.rounds.iter().enumerate() {
        m.push_str(&format!("\nAsked (round {}):\n", i + 1));
        for q in &r.asked {
            m.push_str(&format!("  - {q}\n"));
        }
        m.push_str(&format!("Answered: {}\n", r.answered));
    }
    m.push_str(&format!("\nTicket: {}/{}\n", d.project, t.id));
    if let Some(s) = &p.filed_by {
        m.push_str(&format!("Filed-by: {s}\n"));
    }
    m
}

// ---------------------------------------------------------------- step

pub fn step(ctx: &Ctx, target: &str, text: Option<&str>, done: bool, by: Option<&str>) -> Result<bool> {
    ctx.require_log()?;
    if let Some(why) = ctx.read_from_origin()? {
        bail!("nothing changed: {why}");
    }
    let r = ticket::parse_ref(target).ok_or_else(|| anyhow!("`{target}` is not <project>/<id> or <project>/<id>.<n>"))?;
    require_project(ctx, &r.project)?;
    let f = file_at(ctx, &r.project);
    let t = f.find(&r.id).ok_or_else(|| anyhow!("no open ticket {}/{} (abot ticket list {})", r.project, r.id, r.project))?;
    match (r.step, text, done) {
        (None, Some(text), false) => {
            let mut probe = t.clone();
            probe.steps.push(Step { text: text.trim().to_string(), done: false, by: None });
            let n = probe.steps.len();
            let problems: Vec<String> = ticket::lint(&probe).into_iter().filter(|p| p.starts_with(&format!("step {n}"))).collect();
            if !problems.is_empty() {
                bail!("not added: {}", problems.join("; "));
            }
            let msg = format!("Ticket {}/{}: step {n}, {}\n\nTicket: {}/{}\n", r.project, r.id, clip(&flatten(text), 100), r.project, r.id);
            let (id, add) = (r.id.clone(), text.trim().to_string());
            let sha = onmain::commit_file(ctx, &ticket::path(&r.project), &msg, |old| {
                let mut f = ticket::parse(&r.project, old.unwrap_or(""));
                let t = f.find_mut(&id).ok_or_else(|| anyhow!("{}/{id} is no longer open", r.project))?;
                t.steps.push(Step { text: add.clone(), done: false, by: None });
                Ok(ticket::render(&f))
            })?;
            println!("added step {n} to {}/{} at {sha}: {}", r.project, r.id, flatten(text));
            println!("a commit names it with: Ready: {}/{}.{n}", r.project, r.id);
            Ok(true)
        }
        (Some(n), Some(text), false) => {
            // Reword a step: its number, and any commit naming it, stay.
            if n == 0 || n > t.steps.len() {
                bail!("{}/{} has {} step(s), not {n}", r.project, r.id, t.steps.len());
            }
            let mut probe = t.clone();
            probe.steps[n - 1].text = text.trim().to_string();
            let problems: Vec<String> = ticket::lint(&probe).into_iter().filter(|p| p.starts_with(&format!("step {n}"))).collect();
            if !problems.is_empty() {
                bail!("not reworded: {}", problems.join("; "));
            }
            let msg = format!(
                "Ticket {}/{}: step {n} says what it means, {}\n\nWas: {}\n\nTicket: {}/{}\n",
                r.project,
                r.id,
                clip(&flatten(text), 80),
                t.steps[n - 1].text,
                r.project,
                r.id
            );
            let (id, new) = (r.id.clone(), text.trim().to_string());
            let sha = onmain::commit_file(ctx, &ticket::path(&r.project), &msg, |old| {
                let mut f = ticket::parse(&r.project, old.unwrap_or(""));
                let t = f.find_mut(&id).ok_or_else(|| anyhow!("{}/{id} is no longer open", r.project))?;
                let st = t.steps.get_mut(n - 1).ok_or_else(|| anyhow!("{}/{id} no longer has step {n}", r.project))?;
                st.text = new.clone();
                Ok(ticket::render(&f))
            })?;
            println!("reworded {r} at {sha}: {}", flatten(text));
            Ok(true)
        }
        (Some(n), None, true) => {
            let by = format!("{}, {}", today(), by.map(flatten).unwrap_or_else(|| "by hand".into()));
            let sha = mark(ctx, &[(r.clone(), by)], &format!("{}/{}.{n} is done", r.project, r.id), None)?;
            println!("marked {r} done at {sha}");
            Ok(true)
        }
        _ => bail!(
            "say one thing: add a step with abot ticket step {}/{} \"<a plain sentence>\", reword one with abot ticket step {}/{}.<n> \"<a plain sentence>\", or mark one done with abot ticket step {}/{}.<n> --done",
            r.project,
            r.id,
            r.project,
            r.id,
            r.project,
            r.id
        ),
    }
}

/// Apply marks to ticket text: each ref's step (or the whole ticket's open
/// steps) done, with `by`. Returns the new text, what changed (as lines),
/// and what could not be found.
pub fn apply_marks(project: &str, old: &str, marks: &[(Ref, String)], date: &str) -> (String, Vec<String>, Vec<String>) {
    let mut f = ticket::parse(project, old);
    let mut changed = vec![];
    let mut missing = vec![];
    for (r, by) in marks.iter().filter(|(r, _)| r.project == project) {
        let Some(t) = f.find_mut(&r.id) else {
            missing.push(format!("{r}: no open ticket"));
            continue;
        };
        let idx: Vec<usize> = match r.step {
            Some(n) if n >= 1 && n <= t.steps.len() => vec![n - 1],
            Some(n) => {
                missing.push(format!("{r}: {}/{} has {} step(s), not {n}", r.project, r.id, t.steps.len()));
                continue;
            }
            None => (0..t.steps.len()).collect(),
        };
        for i in idx {
            if !t.steps[i].done {
                t.steps[i].done = true;
                t.steps[i].by = Some(by.clone());
                changed.push(format!("{}/{}.{}: {}", r.project, r.id, i + 1, t.steps[i].text));
            }
        }
    }
    let finished = f.settle(date, &marks.first().map(|(_, b)| b.split_once(", ").map(|x| x.1).unwrap_or("").to_string()).unwrap_or_default());
    for id in finished {
        changed.push(format!("{project}/{id}: every step done, moved to Done"));
    }
    (ticket::render(&f), changed, missing)
}

/// Mark steps done on origin's main, one commit per ticket file touched.
fn mark(ctx: &Ctx, marks: &[(Ref, String)], why: &str, extra: Option<&str>) -> Result<String> {
    let mut projects: Vec<String> = marks.iter().map(|(r, _)| r.project.clone()).collect();
    projects.sort();
    projects.dedup();
    let mut last = String::new();
    for p in projects {
        let mut said = vec![];
        let msg_of = |changed: &[String]| {
            let mut m = format!("Ticket {why}\n\n");
            for c in changed {
                m.push_str(&format!("- {c}\n"));
            }
            if let Some(e) = extra {
                m.push_str(&format!("\n{e}\n"));
            }
            m
        };
        // The message depends on what the edit finds, so the edit is run
        // first on the current text to know, then again inside the commit.
        let cur = ctx.read_at(&ctx.base(), &ticket::path(&p)).unwrap_or_default();
        let (_, changed, missing) = apply_marks(&p, &cur, marks, &today());
        if !missing.is_empty() {
            bail!("nothing marked: {}", missing.join("; "));
        }
        if changed.is_empty() {
            bail!("nothing to mark: already done");
        }
        said.extend(changed.clone());
        let msg = msg_of(&said);
        last = onmain::commit_file(ctx, &ticket::path(&p), &msg, |old| Ok(apply_marks(&p, old.unwrap_or(""), marks, &today()).0))?;
    }
    Ok(last)
}

// ---------------------------------------------------------------- retire

pub fn retire(ctx: &Ctx, target: &str, why: &str, title: Option<&str>) -> Result<bool> {
    ctx.require_log()?;
    if why.trim().is_empty() {
        bail!("say why it is retired: abot ticket retire {target} \"<why>\"");
    }
    if let Some(n) = ctx.read_from_origin()? {
        bail!("nothing changed: {n}");
    }
    let r = ticket::parse_ref(target).filter(|r| r.step.is_none()).ok_or_else(|| anyhow!("`{target}` is not <project>/<id>"))?;
    require_project(ctx, &r.project)?;
    let f = file_at(ctx, &r.project);
    let open_title = f.find(&r.id).map(|t| t.title.clone());
    if open_title.is_none() {
        if f.ids().any(|x| x == r.id) {
            bail!("{}/{} is already done or retired", r.project, r.id);
        }
        if title.is_none_or(|t| t.trim().is_empty()) {
            bail!(
                "{}/{} is not an open ticket. To record the retirement of one from before abot tickets (an old READY item), give its title: --title \"<title>\"",
                r.project,
                r.id
            );
        }
    }
    let name = open_title.clone().or(title.map(flatten)).unwrap_or_default();
    let msg = format!(
        "Retire {}/{}: {}\n\n{}: {}\n\nTicket: {}/{}\n",
        r.project,
        r.id,
        clip(&flatten(why), 100),
        if open_title.is_some() { "It was open" } else { "From before abot tickets; recorded so the id is never reused" },
        name,
        r.project,
        r.id
    );
    let (id, note) = (r.id.clone(), flatten(why));
    let sha = onmain::commit_file(ctx, &ticket::path(&r.project), &msg, |old| {
        let mut f = ticket::parse(&r.project, old.unwrap_or(""));
        f.open.retain(|t| t.id != id);
        if f.retired.iter().any(|x| x.id == id) {
            bail!("{}/{id} was retired a moment ago by someone else", r.project);
        }
        f.retired.push(Closed { id: id.clone(), title: name.clone(), note: note.clone(), date: today() });
        Ok(ticket::render(&f))
    })?;
    println!("retired {}/{} at {sha}: {}", r.project, r.id, flatten(why));
    Ok(true)
}

// ---------------------------------------------------------------- list

pub fn files(ctx: &Ctx) -> Vec<File> {
    let names = ctx.log_git().try_run(&["ls-tree", "--name-only", &ctx.base(), &format!("{}/", ticket::DIR)]).unwrap_or_default();
    names
        .lines()
        .filter_map(|l| l.strip_prefix(&format!("{}/", ticket::DIR)))
        .filter_map(|l| l.strip_suffix(".md"))
        .map(|p| file_at(ctx, p))
        .collect()
}

fn ticket_json(project: &str, t: &Ticket) -> Value {
    let problems = ticket::lint(t);
    json!({
        "project": project,
        "id": t.id,
        "ref": format!("{project}/{}", t.id),
        "title": t.title,
        "why": t.why,
        "done_when": t.done_when,
        "waits_on": t.waits.as_ref().map(|w| json!({ "who": w.who, "what": w.what })),
        "detail": t.detail,
        "filed": t.filed,
        "steps": t.steps.iter().enumerate().map(|(i, s)| json!({
            "n": i + 1,
            "ready": format!("{project}/{}.{}", t.id, i + 1),
            "text": s.text,
            "done": s.done,
            "by": s.by,
        })).collect::<Vec<_>>(),
        "written_out": problems.is_empty(),
        "problems": problems,
    })
}

pub fn list(ctx: &Ctx, project: Option<&str>, json_out: bool) -> Result<bool> {
    let g = ctx.require_log()?;
    let note = ctx.read_from_origin()?;
    if let Some(p) = project {
        require_project(ctx, p)?;
    }
    let all: Vec<File> = files(ctx).into_iter().filter(|f| project.is_none_or(|p| f.project == p)).collect();
    if json_out {
        let closed = |f: &File, xs: &[Closed]| -> Vec<Value> {
            xs.iter().map(|d| json!({ "project": f.project, "id": d.id, "ref": format!("{}/{}", f.project, d.id), "title": d.title, "note": d.note, "date": d.date })).collect()
        };
        let doc = json!({
            "log": ctx.log.display().to_string(),
            "read_from": ctx.base(),
            "main": g.try_run(&["rev-parse", "--short", &ctx.base()]),
            "fetched": note.is_none(),
            "note": note,
            "tickets": all.iter().flat_map(|f| f.open.iter().map(move |t| ticket_json(&f.project, t))).collect::<Vec<_>>(),
            "done": all.iter().flat_map(|f| closed(f, &f.done)).collect::<Vec<_>>(),
            "retired": all.iter().flat_map(|f| closed(f, &f.retired)).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&doc)?);
        return Ok(true);
    }
    if let Some(n) = &note {
        println!("({n})");
    }
    if all.iter().all(|f| f.open.is_empty()) {
        println!("(no open tickets{})", project.map(|p| format!(" in {p}")).unwrap_or_default());
    }
    for f in &all {
        if project.is_some() {
            for t in &f.open {
                print!("{}", ticket_block(t));
                println!();
            }
            if !f.done.is_empty() {
                println!("done: {}", f.done.iter().map(|d| d.id.as_str()).collect::<Vec<_>>().join(", "));
            }
            if !f.retired.is_empty() {
                println!("retired: {}", f.retired.iter().map(|d| d.id.as_str()).collect::<Vec<_>>().join(", "));
            }
            continue;
        }
        for t in &f.open {
            let n = t.steps.iter().filter(|s| s.done).count();
            let waits = t.waits.as_ref().map(|w| format!("  · waits on {}", w.who)).unwrap_or_default();
            let thin = if ticket::lint(t).is_empty() { "" } else { "  · NOT WRITTEN OUT" };
            println!("{}/{}  {}  [{n}/{}]{waits}{thin}", f.project, t.id, clip(&t.title, 70), t.steps.len());
        }
    }
    Ok(true)
}

/// Problems with every ticket file on the read base, for `abot check`.
pub fn check(ctx: &Ctx) -> Vec<String> {
    let mut out = vec![];
    let projects = ctx.projects();
    for f in files(ctx) {
        let path = ticket::path(&f.project);
        if !projects.contains(&f.project) {
            out.push(format!("{path}: no project `{}` (state/projects/{}.md)", f.project, f.project));
        }
        let mut seen: Vec<&str> = vec![];
        for id in f.ids() {
            if seen.contains(&id) {
                out.push(format!("{path}: id `{id}` used twice"));
            }
            seen.push(id);
        }
        for t in &f.open {
            for p in ticket::lint(t) {
                out.push(format!("{path}: {}: {p}", t.id));
            }
        }
    }
    out
}
