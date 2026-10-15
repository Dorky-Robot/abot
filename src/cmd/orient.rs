//! orient: one command instead of asking anyone. What is true (the state
//! file), why (the commits and decisions), what was already tried (dead
//! ends), what is in flight (branches), and the nearest insights.

use super::map;
use crate::branch::valid_slug;
use crate::ctx::Ctx;
use crate::diwa;
use crate::git::Git;
use crate::out::Out;
use anyhow::{Result, bail};

const CAP: usize = 150;
const CAP_PER_EXTRA: usize = 80;

pub fn run(ctx: &Ctx, projects: Vec<String>) -> Result<()> {
    if projects.is_empty() {
        bail!("name a project: abot orient <project> (the slugs are the files in state/projects/)");
    }
    for p in &projects {
        if !valid_slug(p) {
            bail!("`{p}` is not a project slug; a slug is a file name in state/projects/, e.g. everyday-vet");
        }
    }
    let g = ctx.require_log()?;
    let mut o = Out::new(ctx.tty);

    let note = ctx.read_from_origin()?;
    let base = ctx.base();
    let head = g.try_run(&["log", "-1", "--format=%h %ad", "--date=short", &base]).unwrap_or_default();
    o.line(format!("log: {} {base} at {head}", ctx.log_slug()));
    if let Some(n) = note {
        o.line(format!("({n})"));
    }

    ecosystem(ctx, &mut o, &projects);

    if projects.len() > 1 {
        o.header("map: these projects");
        let rows: Vec<String> = projects
            .iter()
            .map(|p| map::summarize(ctx, p))
            .map(|s| {
                format!(
                    "{}: uses {} · serves {} · waits on Felix {} · in flight {}",
                    s.name,
                    or_dash(&s.uses.join(", ")),
                    or_dash(&s.serves.join(", ")),
                    s.waits.len(),
                    s.in_flight.len()
                )
            })
            .collect();
        o.block(rows, 20, "abot map");
    }

    for p in &projects {
        project(ctx, &g, &mut o, p);
    }
    o.print(CAP + CAP_PER_EXTRA * (projects.len() - 1));
    Ok(())
}

fn or_dash(s: &str) -> &str {
    if s.is_empty() { "-" } else { s }
}

fn ecosystem(ctx: &Ctx, o: &mut Out, projects: &[String]) {
    o.header("ecosystem");
    let files = ctx
        .log_git()
        .try_run(&["ls-tree", "--name-only", &ctx.base(), "ecosystem/"])
        .unwrap_or_default();
    if files.trim().is_empty() {
        o.line("(no ecosystem/ yet)");
        return;
    }
    if let Some(obj) = ctx.read_at(&ctx.base(), "ecosystem/objectives.md") {
        let lines: Vec<&str> = obj.lines().filter(|l| !l.trim().is_empty()).collect();
        o.block(lines, 15, "see ecosystem/objectives.md");
    } else {
        o.line("(no ecosystem/objectives.md yet)");
    }
    if let Some(m) = ctx.read_at(&ctx.base(), "ecosystem/map.md") {
        let hits: Vec<&str> = m
            .lines()
            .filter(|l| {
                let low = l.to_ascii_lowercase();
                projects.iter().any(|p| low.contains(p.as_str()))
            })
            .collect();
        if !hits.is_empty() {
            o.line("from ecosystem/map.md:");
            o.block(hits, 10, "see ecosystem/map.md");
        }
    }
    let also: Vec<&str> = files
        .lines()
        .filter(|f| !f.ends_with("objectives.md") && !f.ends_with("map.md"))
        .collect();
    if !also.is_empty() {
        o.line(format!("also read: {}", also.join(", ")));
    }
}

fn project(ctx: &Ctx, g: &Git, o: &mut Out, p: &str) {
    let base = ctx.base();
    let state = ctx.state_path_at(&base, p);
    match &state {
        Some(f) => {
            o.header(&format!("{p}: {f}"));
            let text = ctx.read_at(&base, f).unwrap_or_default();
            o.block(text.lines().map(String::from), 70, &format!("see {f}"));
        }
        None => {
            o.header(&format!("{p}: no state file"));
            o.line(format!(
                "(no state/projects/{p}.md yet: the first worker on {p} writes it; abot start {p}/<task> \"<why>\" --new)"
            ));
        }
    }

    if let Some(f) = &state {
        o.header("how it got here");
        let log = g
            .try_run(&["log", &base, "--format=%h %ad %s", "--date=short", "-n", "10", "--", f])
            .unwrap_or_default();
        o.block(log.lines().map(String::from), 10, &format!("git log -- {f}"));
    }

    o.header(&format!("decisions about {p}"));
    let re = format!("^About: (.*, *)?{}( *,|$)", regex::escape(p));
    let dec = g
        .try_run(&["log", &base, "--format=%h %ad %s", "--date=short", "-n", "12", "-E", "-i", &format!("--grep={re}")])
        .unwrap_or_default();
    o.block(dec.lines().map(String::from), 12, "git log --grep About:");

    o.header("dead ends");
    let dead = map::dead_ends(g, &base, p);
    o.block(dead, 10, &format!("abot tree {p}"));

    o.header(&format!("in flight: {p}/*"));
    o.block(in_flight(g, p, base == "main"), 15, "git branch -r");

    o.header(&format!("diwa: {} \"{p}\"", ctx.log_slug()));
    o.block(diwa::lines(ctx, &["search", &ctx.log_slug(), p, "-n", "5"]), 30, "diwa search");

    match ctx.code_repo(p, None) {
        Ok(Some(c)) => {
            let slug = crate::repos::origin_url(&c).and_then(|u| crate::repos::org_name(&u));
            o.header(&format!("code repo: {}", c.display()));
            let handoff = c.join("docs/HANDOFF.md");
            if handoff.exists() {
                o.line(format!("read next: {}", handoff.display()));
            } else {
                o.line("(no docs/HANDOFF.md)");
            }
            let cg = Git::new(&c);
            let br = cg
                .try_run(&["for-each-ref", "--format=%(refname:short)  %(committerdate:short)  %(subject)", &format!("refs/heads/{p}/")])
                .unwrap_or_default();
            if !br.trim().is_empty() {
                o.line("code branches:");
                o.block(br.lines().map(String::from), 8, "git branch");
            }
            if let Some(s) = slug {
                o.header(&format!("diwa: {s} \"{p}\""));
                o.block(diwa::lines(ctx, &["search", &s, p, "-n", "3"]), 20, "diwa search");
            }
        }
        Ok(None) => {
            o.header("code repo");
            o.line(format!(
                "(none found under {} for {p}; a `Repo: Org/name` line in the state file names it)",
                ctx.projects.display()
            ));
        }
        Err(e) => {
            o.header("code repo");
            o.line(format!("({e:#})"));
        }
    }
}

/// `<p>/*` branches on origin, newest first, one line each; local ones too
/// only when origin couldn't be reached.
pub fn in_flight(g: &Git, p: &str, with_local: bool) -> Vec<String> {
    let remote = format!("refs/remotes/origin/{p}/");
    let local = format!("refs/heads/{p}/");
    let mut args = vec!["for-each-ref", "--sort=-committerdate", "--format=%(refname)\t%(committerdate:short)\t%(subject)", remote.as_str()];
    if with_local {
        args.push(local.as_str());
    }
    let raw = g.try_run(&args).unwrap_or_default();
    let mut seen: Vec<(String, String, String, Vec<&str>)> = vec![];
    for line in raw.lines() {
        let mut it = line.splitn(3, '\t');
        let (r, d, s) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""));
        let (name, place) = match r.strip_prefix("refs/heads/") {
            Some(n) => (n, "local"),
            None => (r.strip_prefix("refs/remotes/origin/").unwrap_or(r), "origin"),
        };
        match seen.iter_mut().find(|x| x.0 == name) {
            Some(x) => x.3.push(place),
            None => seen.push((name.into(), d.into(), s.into(), vec![place])),
        }
    }
    seen.into_iter()
        .map(|(n, d, s, w)| format!("{n}  {d}  [{}]  {s}", w.join("+")))
        .collect()
}
