//! map, check and tree: the log read whole. Everything here is generated
//! from the state files and the branches; nothing is kept by hand.

use crate::branch;
use crate::ctx::Ctx;
use crate::git::Git;
use crate::out::{Out, clip};
use crate::secrets;
use crate::statefile;
use crate::trailers;
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Summary {
    pub name: String,
    pub state: Option<String>,
    pub live: Option<String>,
    pub next: Option<String>,
    pub waits: Vec<String>,
    pub in_flight: Vec<String>,
    pub dead_ends: usize,
    pub last_change: Option<String>,
    pub uses: Vec<String>,
    pub serves: Vec<String>,
}

/// Subjects of dead ends for a project: `Dead end: <p>/...`, merges left out
/// (the lesson is the branch's last commit; the merge only joins it in).
pub fn dead_ends(g: &Git, p: &str) -> Vec<String> {
    let want = format!("dead end: {p}/");
    g.try_run(&[
        "log",
        "main",
        "--no-merges",
        "--format=%h %ad %s",
        "--date=short",
        "-i",
        "-F",
        &format!("--grep={want}"),
    ])
    .unwrap_or_default()
    .lines()
    .filter(|l| {
        // "<sha> <date> <subject>"
        l.splitn(3, ' ').nth(2).is_some_and(|s| s.to_ascii_lowercase().starts_with(&want))
    })
    .map(String::from)
    .collect()
}

pub fn origin_branches(g: &Git, p: &str) -> Vec<String> {
    g.try_run(&["for-each-ref", "--format=%(refname:lstrip=3)", &format!("refs/remotes/origin/{p}/")])
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

pub fn summarize(ctx: &Ctx, p: &str) -> Summary {
    let g = ctx.log_git();
    let state = ctx.state_path_at("main", p);
    let text = state.as_deref().and_then(|f| ctx.read_at("main", f)).unwrap_or_default();
    let last_change = state.as_deref().and_then(|f| {
        g.try_run(&["log", "-1", "--format=%ad", "--date=short", "main", "--", f])
            .filter(|s| !s.is_empty())
    });
    Summary {
        name: p.to_string(),
        live: statefile::items(&text, "Live now").into_iter().next(),
        next: statefile::items(&text, "Next").into_iter().next(),
        waits: statefile::items(&text, "Waits on Felix"),
        in_flight: origin_branches(&g, p),
        dead_ends: dead_ends(&g, p).len(),
        last_change,
        uses: statefile::list_field(&text, "Uses"),
        serves: statefile::list_field(&text, "Serves"),
        state,
    }
}

fn fetch(g: &Git) -> Option<String> {
    if !g.has_origin() {
        return Some("no origin remote".into());
    }
    g.run(&["fetch", "-q", "--prune", "origin"]).err().map(|e| format!("{e:#}"))
}

pub fn map(ctx: &Ctx, json_out: bool) -> Result<()> {
    let g = ctx.require_log()?;
    let fetched = fetch(&g);
    let projects = ctx.projects_on_main();
    let rows: Vec<Summary> = projects.iter().map(|p| summarize(ctx, p)).collect();
    if json_out {
        let v: Vec<Value> = rows
            .iter()
            .map(|s| {
                json!({
                    "project": s.name,
                    "state": s.state,
                    "live": s.live,
                    "next": s.next,
                    "waits_on_felix": s.waits,
                    "in_flight": s.in_flight,
                    "dead_ends": s.dead_ends,
                    "last_change": s.last_change,
                    "uses": s.uses,
                    "serves": s.serves,
                })
            })
            .collect();
        let doc = json!({
            "log": ctx.log.display().to_string(),
            "main": g.try_run(&["rev-parse", "--short", "main"]),
            "fetched": fetched.is_none(),
            "projects": v,
        });
        println!("{}", serde_json::to_string_pretty(&doc)?);
        return Ok(());
    }
    if let Some(why) = &fetched {
        println!("(origin not fetched: {why}; branches may be stale)");
    }
    if rows.is_empty() {
        println!("(no projects: state/projects/ on main is empty)");
        return Ok(());
    }
    let w = rows.iter().map(|s| s.name.len()).max().unwrap_or(7).max(7);
    println!(
        "{:<w$}  {:<10}  {:>5}  {:>6}  {:>4}  {:<34}  NEXT",
        "PROJECT", "CHANGED", "WAITS", "FLIGHT", "DEAD", "LIVE"
    );
    for s in &rows {
        println!(
            "{:<w$}  {:<10}  {:>5}  {:>6}  {:>4}  {:<34}  {}",
            s.name,
            s.last_change.as_deref().unwrap_or("-"),
            s.waits.len(),
            s.in_flight.len(),
            s.dead_ends,
            clip(s.live.as_deref().unwrap_or("-"), 34),
            clip(s.next.as_deref().unwrap_or("-"), 60),
        );
    }
    let waits: Vec<String> = rows
        .iter()
        .flat_map(|s| s.waits.iter().map(move |x| format!("{}: {x}", s.name)))
        .collect();
    println!("\n== waits on Felix");
    print_or_nothing(&waits);
    let flight: Vec<String> = rows
        .iter()
        .flat_map(|s| s.in_flight.iter().cloned())
        .collect();
    println!("\n== in flight (origin)");
    print_or_nothing(&flight);
    println!("\n== uses / serves");
    for s in &rows {
        println!(
            "{}: uses {} · serves {}",
            s.name,
            if s.uses.is_empty() { "-".into() } else { s.uses.join(", ") },
            if s.serves.is_empty() { "-".into() } else { s.serves.join(", ") }
        );
    }
    Ok(())
}

fn print_or_nothing(v: &[String]) {
    if v.is_empty() {
        println!("(nothing)");
    }
    for l in v {
        println!("{l}");
    }
}

const STALE_DAYS: u64 = 7;

/// Lint the log. Ok(true) when clean.
pub fn check(ctx: &Ctx, json_out: bool) -> Result<bool> {
    let g = ctx.require_log()?;
    let mut problems: Vec<String> = vec![];
    let mut notes: Vec<String> = vec![];
    if let Some(why) = fetch(&g) {
        notes.push(format!("origin not fetched ({why}); branch checks use what is local"));
    }

    // 1. State files keep their shape.
    let projects = ctx.projects_on_main();
    for p in &projects {
        let f = format!("state/projects/{p}.md");
        let text = ctx.read_at("main", &f).unwrap_or_default();
        for issue in statefile::lint(&text) {
            problems.push(format!("{f}: {issue}"));
        }
    }

    // 2. The generated map is current.
    map_current(ctx, &g, &mut problems, &mut notes);

    // 3. Branches on origin: named <project>/<task>, have a project, not stale.
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let refs = g
        .try_run(&["for-each-ref", "--format=%(refname:lstrip=3)\t%(committerdate:unix)\t%(committerdate:short)\t%(objectname)", "refs/remotes/origin/"])
        .unwrap_or_default();
    let mut branches = 0;
    for line in refs.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 4 || parts[0] == "HEAD" || parts[0] == "main" {
            continue;
        }
        branches += 1;
        let (name, unix, date, sha) = (parts[0], parts[1].parse::<u64>().unwrap_or(now), parts[2], parts[3]);
        let b = match branch::parse(name) {
            Ok(b) => b,
            Err(_) => {
                problems.push(format!("origin/{name}: not a <project>/<task> branch"));
                continue;
            }
        };
        if ctx.state_path_at("main", &b.project).is_none() {
            problems.push(format!("origin/{name}: no state file for project `{}` (state/projects/{}.md)", b.project, b.project));
        }
        let days = now.saturating_sub(unix) / 86_400;
        if days > STALE_DAYS {
            let parked = g
                .commit_message(sha)
                .ok()
                .and_then(|m| trailers::get(&m, "Outcome"))
                .is_some_and(|o| o.eq_ignore_ascii_case("parked"));
            let what = if parked { "parked" } else { "in flight" };
            problems.push(format!(
                "origin/{name}: {what} for {days} days (last commit {date}); land it, dead-end it, or say why it waits"
            ));
        }
    }

    // 4. No secrets in what is tracked on main.
    let files = g.run(&["ls-tree", "-r", "--name-only", "main"])?;
    let mut scanned = 0;
    for f in files.lines() {
        if secrets::is_transcript(f) {
            continue;
        }
        let Some(text) = ctx.read_at("main", f) else { continue };
        if text.contains('\0') {
            continue;
        }
        scanned += 1;
        for h in secrets::scan_text(f, &text) {
            problems.push(h.to_string());
        }
    }

    if json_out {
        let doc = json!({
            "ok": problems.is_empty(),
            "problems": problems,
            "notes": notes,
            "state_files": projects.len(),
            "branches": branches,
            "files_scanned": scanned,
        });
        println!("{}", serde_json::to_string_pretty(&doc)?);
    } else {
        for n in &notes {
            eprintln!("note: {n}");
        }
        for p in &problems {
            println!("{p}");
        }
        if problems.is_empty() {
            println!(
                "ok: {} state file(s), {branches} branch(es) on origin, {scanned} file(s) scanned",
                projects.len()
            );
        }
    }
    Ok(problems.is_empty())
}

/// Run bin/map (if the log has one) in a throwaway worktree of main, so a
/// generator that writes in place never touches the main checkout.
fn map_current(_ctx: &Ctx, g: &Git, problems: &mut Vec<String>, notes: &mut Vec<String>) {
    if !g.ok(&["cat-file", "-e", "main:bin/map"]) {
        notes.push("no bin/map yet; map.md not checked".into());
        return;
    }
    let tmp = std::env::temp_dir().join(format!("abot-check-map-{}", std::process::id()));
    let tmp_s = tmp.display().to_string();
    if g.run(&["worktree", "add", "-q", "--detach", &tmp_s, "main"]).is_err() {
        notes.push("could not make a scratch worktree to run bin/map".into());
        return;
    }
    let out = std::process::Command::new(tmp.join("bin/map"))
        .current_dir(&tmp)
        .env("ABOT_LOG", &tmp)
        .env("ECOSYSTEM_LOG", &tmp)
        .output();
    let wt = Git::new(&tmp);
    match out {
        Ok(o) if o.status.success() => {
            // bin/map writes ecosystem/map.md where it stands. If running it
            // on a clean copy of main changes the file, main's map is stale.
            let changed = wt
                .try_run(&["status", "--porcelain", "--", "ecosystem/map.md"])
                .is_some_and(|s| !s.is_empty());
            if changed {
                problems.push("ecosystem/map.md: not what bin/map generates from main; run bin/map on a branch and land it".into());
            }
        }
        Ok(o) => problems.push(format!(
            "bin/map failed: {}",
            String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("no message")
        )),
        Err(e) => problems.push(format!("bin/map could not run: {e}")),
    }
    let _ = g.run(&["worktree", "remove", "--force", &tmp_s]);
}

/// The discovery tree: main and `<p>/*`, with outcomes from the trailers.
pub fn tree(ctx: &Ctx, p: &str, limit: usize, json_out: bool) -> Result<()> {
    if !branch::valid_slug(p) {
        bail!("`{p}` is not a project slug");
    }
    let g = ctx.require_log()?;
    let mut refs: Vec<String> = vec!["main".into()];
    let more = g
        .try_run(&["for-each-ref", "--format=%(refname)", &format!("refs/heads/{p}/"), &format!("refs/remotes/origin/{p}/")])
        .unwrap_or_default();
    refs.extend(more.lines().map(String::from));
    let n = limit.to_string();
    let fmt = "%x1e%h%x1f%ad%x1f%s%x1f%(trailers:key=Outcome,valueonly,separator=%x2C)%x1f%D%x1f%P%x1f%H";
    let mut args: Vec<&str> = vec!["log", "--date=short", "-n", &n];
    if !json_out {
        args.push("--graph");
    }
    let fmt_arg = format!("--format={fmt}");
    args.push(&fmt_arg);
    args.extend(refs.iter().map(String::as_str));
    let raw = g.run(&args)?;

    if json_out {
        let nodes: Vec<Value> = raw
            .split('\x1e')
            .filter(|c| !c.trim().is_empty())
            .map(|c| {
                let f: Vec<&str> = c.trim_end().split('\x1f').collect();
                let subject = f.get(2).copied().unwrap_or("");
                json!({
                    "sha": f.get(6).copied().unwrap_or(""),
                    "parents": f.get(5).map(|s| s.split_whitespace().collect::<Vec<_>>()).unwrap_or_default(),
                    "date": f.get(1).copied().unwrap_or(""),
                    "subject": subject,
                    "outcome": outcome(subject, f.get(3).copied().unwrap_or(""), p),
                    "refs": f.get(4).copied().unwrap_or(""),
                    "project": mentions(subject, p),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json!({ "project": p, "nodes": nodes }))?);
        return Ok(());
    }

    let mut o = Out::new(ctx.tty);
    for line in raw.lines() {
        // Graph prefix, then \x1e and the fields.
        match line.split_once('\x1e') {
            Some((graph, rest)) => {
                let f: Vec<&str> = rest.split('\x1f').collect();
                let (h, d, s, t, dec) = (f[0], f.get(1).copied().unwrap_or(""), f.get(2).copied().unwrap_or(""), f.get(3).copied().unwrap_or(""), f.get(4).copied().unwrap_or(""));
                let tag = outcome(s, t, p).map(|o| format!("[{o}] ")).unwrap_or_default();
                let dec = if dec.is_empty() { String::new() } else { format!(" ({dec})") };
                let mark = if mentions(s, p) && ctx.tty { ("\x1b[1m", "\x1b[0m") } else { ("", "") };
                o.line(format!("{graph}{h} {d} {}{tag}{s}{}{dec}", mark.0, mark.1));
            }
            None => o.line(line.to_string()),
        }
    }
    o.print(usize::MAX);
    Ok(())
}

fn mentions(subject: &str, p: &str) -> bool {
    subject.contains(&format!("{p}/"))
}

/// The outcome of a commit: its trailer, or a dead-end merge's subject.
fn outcome(subject: &str, trailer: &str, p: &str) -> Option<&'static str> {
    if let Some(t) = trailers::outcome_tag(trailer.split(',').next_back().unwrap_or("")) {
        return Some(t.trim_matches(|c| c == '[' || c == ']'));
    }
    let s = subject.to_ascii_lowercase();
    if s.starts_with(&format!("dead end: {p}/")) {
        return Some("dead-end");
    }
    None
}
