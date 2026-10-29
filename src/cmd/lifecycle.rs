//! start, land, dead-end, park: a task branch from first commit to its end.
//!
//! Every step is a git command you could type. Checks come first and act
//! second, so a refusal leaves nothing half done. Nothing is force-pushed
//! and nothing already on main is rewritten.

use super::{flatten, one_line, sync_main, today};
use crate::branch::{self, TaskBranch};
use crate::ctx::{Ctx, Here, here, require_on_main, sibling};
use crate::git::Git;
use crate::out::clip;
use crate::secrets;
use crate::statefile;
use crate::trailers;
use anyhow::{Result, anyhow, bail};
use std::path::{Path, PathBuf};

const TRUTH_DIRS: [&str; 4] = ["state/", "ecosystem/", "how/", "people/"];

fn display(p: &Path) -> String {
    p.display().to_string()
}

// ---------------------------------------------------------------- start

pub fn start(ctx: &Ctx, name: &str, why: &str, new: bool, repo: Option<&Path>, session: Option<&str>, parent: Option<&str>) -> Result<()> {
    let b = branch::parse(name).map_err(|e| anyhow!(e))?;
    if why.trim().is_empty() {
        bail!("missing WHY: abot start {b} \"<why>\" (one sentence on why this path is worth trying; it becomes the first commit)");
    }
    let log = ctx.require_log()?;
    // Branch from origin's main, the copy of record, not from this clone's
    // main, which may be stale; nothing in the main checkout is touched.
    if let Some(why) = ctx.read_from_origin()? {
        bail!("can't start {b} from origin's main: {why}");
    }
    let base = ctx.base();

    let state = ctx.state_path_at(&base, &b.project);
    match (&state, new) {
        (None, false) => bail!(
            "no state file for `{}`: state/projects/{}.md doesn't exist on main. \
             Check the slug (ls state/projects), or scaffold a new project with --new",
            b.project,
            b.project
        ),
        (Some(f), true) => bail!("`{}` already has {f}; drop --new", b.project),
        _ => {}
    }

    let wt = ctx.log_worktree(&b.task);
    if wt.exists() {
        bail!("{} already exists; pick another task name or remove it", wt.display());
    }
    let bs = b.to_string();
    if log.has_local_branch(&bs) {
        let at = log.worktree_of(&bs).map(|p| format!(" (checked out at {})", p.display())).unwrap_or_default();
        bail!("branch {bs} already exists here{at}");
    }
    if log.has_origin() && log.origin_has_branch(&bs)? {
        bail!(
            "{bs} already exists on origin: someone has that path. Pick another task name, \
             or join it with: git -C {} worktree add {} {bs}",
            ctx.log.display(),
            wt.display()
        );
    }

    let code = ctx.code_repo(&b.project, repo)?;
    let mut code_plan: Option<(Git, PathBuf, String)> = None;
    if let Some(c) = &code {
        let cg = Git::new(c);
        let cwt = sibling(c, &b.task);
        if cwt.exists() {
            bail!("{} already exists; pick another task name or remove it", cwt.display());
        }
        if cg.has_local_branch(&bs) {
            bail!("the code repo {} already has a branch {bs}", c.display());
        }
        if cg.has_origin() && cg.origin_has_branch(&bs)? {
            bail!("{bs} already exists on the code repo's origin ({}): someone has that path", c.display());
        }
        let trunk = cg.trunk();
        code_plan = Some((cg, cwt, trunk));
    }

    // Act.
    log.run(&["worktree", "add", "-q", "--no-track", "-b", &bs, &display(&wt), &base])?;
    let wg = Git::new(&wt);
    if new {
        let f = format!("state/projects/{}.md", b.project);
        let text = match ctx.read_at(&base, "how/project-template.md").and_then(|d| statefile::extract_template(&d)) {
            Some(t) => statefile::fill_template(&t, &b.project),
            None => statefile::template(&b.project),
        };
        std::fs::create_dir_all(wt.join("state/projects"))?;
        std::fs::write(wt.join(&f), text)?;
        wg.run(&["add", "--", &f])?;
    }
    let mut msg = format!("Start {bs}: {}", one_line(why));
    let rest: Vec<&str> = why.trim().lines().skip(1).collect();
    if !rest.is_empty() {
        msg.push_str("\n\n");
        msg.push_str(rest.join("\n").trim());
    }
    if new {
        msg.push_str(&format!("\n\nA new project: state/projects/{}.md is scaffolded from the template.", b.project));
    }
    msg.push_str("\n\n");
    let me = session.map(str::to_string).or_else(session_from_env).map(|id| session_id(&id));
    if let Some(id) = &me {
        msg.push_str(&format!("Session: {id}\n"));
    }
    // who started this session: its spawner's id, never itself
    if let Some(p) = parent.map(str::to_string).or_else(parent_from_env).map(|id| session_id(&id)) {
        if me.as_deref() != Some(p.as_str()) {
            msg.push_str(&format!("Parent: {p}\n"));
        }
    }
    msg.push_str("Outcome: in-flight");
    wg.run(&["commit", "-q", "--allow-empty", "-m", &msg])?;

    let mut lines = vec![format!("started {bs}: {}", one_line(why)), format!("log worktree:  {}", wt.display())];
    if let Some((cg, cwt, trunk)) = &code_plan {
        cg.run(&["worktree", "add", "-q", "-b", &bs, &display(cwt), trunk])?;
        lines.push(format!("code worktree: {} (from {trunk}; not pushed)", cwt.display()));
    } else {
        lines.push(format!(
            "code repo:     none (log only; add a Repo: line or --repo if there is one)"
        ));
    }

    if wg.has_origin() {
        if let Err(e) = wg.run(&["push", "-q", "-u", "origin", &bs]) {
            for l in &lines {
                println!("{l}");
            }
            bail!(
                "started {bs} locally, but the push failed, so nobody else can see the path is taken: {e:#}. \
                 Push it: git -C {} push -u origin {bs}",
                wt.display()
            );
        }
        lines[1].push_str(" (pushed)");
    }
    for l in lines {
        println!("{l}");
    }
    let f = ctx.state_path_at(&base, &b.project).unwrap_or(format!("state/projects/{}.md", b.project));
    println!("next: work in the worktrees; when the truth changes, edit {f} on the branch; then from {}: abot land (or dead-end, or park)", wt.display());
    Ok(())
}

// ---------------------------------------------------------------- sessions

/// The Claude Code session this shell runs in, as the sidebar knows it
/// (`crate::session`); a person at a terminal has none.
fn session_from_env() -> Option<String> {
    crate::session::me().session
}

/// The session that started this one, as its spawner passed it in this session's settings
/// (`--settings '{"env":{"CLAUDE_PARENT_SESSION_ID":"…"}}'`, which carries into a `--bg` session).
fn parent_from_env() -> Option<String> {
    std::env::var("CLAUDE_PARENT_SESSION_ID").ok().filter(|s| !s.trim().is_empty())
}

/// One spelling for an id: the sidebar's `session_…`. The bridge's own
/// `cse_…` is the same session.
pub fn session_id(id: &str) -> String {
    let id = id.trim();
    match id.strip_prefix("cse_") {
        Some(rest) => format!("session_{rest}"),
        None => id.to_string(),
    }
}

/// The `Session:` lines of the branch's Start commit, so the commit that
/// ends the task names who worked it. Read from the Start commit itself, not
/// a range from main: a stale main would pull in other tasks' sessions.
fn session_lines(wt: &Git, bs: &str) -> String {
    let start = format!("^Start {bs}:");
    let log = wt
        .try_run(&["log", "--format=%B%x1e", "--fixed-strings", "--grep", &start[1..], "HEAD"])
        .unwrap_or_default();
    let mut ids: Vec<String> = vec![];
    for m in log.split('\x1e').filter(|m| m.trim_start().starts_with(&start[1..])) {
        for (k, v) in trailers::parse(m) {
            if k.eq_ignore_ascii_case("Session") && !ids.contains(&v) {
                ids.push(v);
            }
        }
    }
    ids.into_iter().map(|i| format!("Session: {i}\n")).collect()
}

// ---------------------------------------------------------------- code repo

/// The code repo's side of a task, when it has the branch.
struct Code {
    repo: Git,
    wt: Option<PathBuf>,
    trunk: String,
}

fn code_side(ctx: &Ctx, b: &TaskBranch, repo: Option<&Path>) -> Result<Option<Code>> {
    let Some(c) = ctx.code_repo(&b.project, repo)? else { return Ok(None) };
    let g = Git::new(&c);
    if !g.has_local_branch(&b.to_string()) {
        return Ok(None);
    }
    let wt = g.worktree_of(&b.to_string());
    let trunk = g.trunk();
    Ok(Some(Code { repo: g, wt, trunk }))
}

/// The code repo's name for a `Code:` trailer: its origin's `Org/name`, else its folder.
fn code_name(c: &Code) -> String {
    crate::repos::origin_url(&c.repo.dir)
        .and_then(|u| crate::repos::org_name(&u))
        .unwrap_or_else(|| c.repo.dir.display().to_string())
}

/// The kept branch's own commits, `<fork>..<tip>`, for a dead end or a park;
/// None when the branch has nothing past where it left the trunk.
fn branch_range(c: &Code, bs: &str) -> Option<trailers::CodeRange> {
    let fork = c.repo.try_run(&["merge-base", &c.trunk, bs])?;
    let tip = c.repo.try_run(&["rev-parse", bs])?;
    if fork == tip {
        return None;
    }
    let short = |s: &str| s[..7.min(s.len())].to_string();
    Some(trailers::CodeRange { repo: code_name(c), from: short(&fork), to: short(&tip) })
}

fn remove_code_worktree(c: &Code, notes: &mut Vec<String>) {
    let Some(wt) = &c.wt else { return };
    if Git::new(wt).is_dirty().unwrap_or(true) {
        notes.push(format!("code worktree {} has uncommitted changes; left in place", wt.display()));
        return;
    }
    match c.repo.run(&["worktree", "remove", &display(wt)]) {
        Ok(_) => notes.push(format!("removed code worktree {}", wt.display())),
        Err(e) => notes.push(format!("could not remove code worktree {}: {e:#}", wt.display())),
    }
}

// ---------------------------------------------------------------- land

pub fn land(ctx: &Ctx, no_state_change: Option<&str>, repo: Option<&Path>, ready: &[String]) -> Result<()> {
    let h = here(ctx)?;
    let bs = h.branch.to_string();
    if no_state_change.is_some_and(|w| w.trim().is_empty()) {
        bail!("--no-state-change needs the reason the truth didn't change");
    }
    if h.wt.is_dirty()? {
        bail!("uncommitted changes in {}; commit them (or drop them) first: abot never commits your edits for you", h.path.display());
    }
    let log = ctx.log_git();
    require_on_main(&log, "the log's main checkout")?;

    // What changed on the branch, against where it left main.
    let base = h.wt.run(&["merge-base", "main", "HEAD"])?;
    let changed = h.wt.run(&["diff", "--name-only", &base, "HEAD"])?;
    let truth: Vec<&str> = changed.lines().filter(|f| TRUTH_DIRS.iter().any(|d| f.starts_with(d))).collect();
    // Ticket steps this task finished: `Ready:` lines in its log commits and
    // its code branch's commits, and --ready. They are a change to the
    // truth too: the ticket file is updated on the branch before it lands.
    let mut flagged: Vec<crate::ticket::Ref> = vec![];
    for r in ready {
        flagged.push(crate::ticket::parse_ref(r).ok_or_else(|| anyhow!("--ready {r}: say <project>/<id>.<n>, or <project>/<id> for all of it"))?);
    }
    let log_refs = refs_in(&h.wt, &format!("{base}..HEAD"));
    let code_pre = code_side(ctx, &h.branch, repo)?
        .map(|c| refs_in(&c.repo, &format!("{}..{bs}", c.trunk)))
        .unwrap_or_default();
    let named_ticket = ticket_named_by_branch(&h, &h.branch);
    let ticket_work = !flagged.is_empty() || !log_refs.is_empty() || !code_pre.is_empty() || named_ticket;
    if truth.is_empty() && no_state_change.is_none() && !ticket_work {
        bail!(
            "{bs} changes nothing under state/, ecosystem/, how/ or people/, so landing it tells the next \
             worker nothing. Edit the state file to say what is true now, or, if the truth really didn't \
             change: abot land --no-state-change \"<why>\""
        );
    }
    let diff = h.wt.run(&["diff", "-U0", "--no-color", "--no-ext-diff", &base, "HEAD"])?;
    let hits = secrets::scan_diff(&diff);
    if !hits.is_empty() {
        let list: Vec<String> = hits.iter().map(|h| format!("  {h}")).collect();
        bail!(
            "refusing to land {bs}: the diff looks like it holds secrets or records. The log keeps references, \
             not secrets: move them out, commit, and land again.\n{}",
            list.join("\n")
        );
    }
    for f in &truth {
        if f.starts_with("state/projects/") && h.path.join(f).exists() {
            let text = std::fs::read_to_string(h.path.join(f)).unwrap_or_default();
            for issue in statefile::lint(&text) {
                eprintln!("warning: {f}: {issue}");
            }
        }
    }

    // The code side: check everything before touching anything.
    let code = code_side(ctx, &h.branch, repo)?;
    if let Some(c) = &code {
        let Some(cwt) = &c.wt else {
            bail!(
                "the code repo {} has branch {bs} but no worktree for it; land it by hand, or add one with \
                 git -C {} worktree add {} {bs}",
                c.repo.dir.display(),
                c.repo.dir.display(),
                sibling(&c.repo.dir, &h.branch.task).display()
            );
        };
        if Git::new(cwt).is_dirty()? {
            bail!("uncommitted changes in the code worktree {}; commit them first", cwt.display());
        }
        if c.repo.current_branch().as_deref() != Some(c.trunk.as_str()) {
            bail!(
                "the code repo's main checkout ({}) is not on {}; landing happens there",
                c.repo.dir.display(),
                c.trunk
            );
        }
    }

    let mut notes = vec![];
    let mut landed_code = None;
    if let Some(c) = &code {
        landed_code = land_code(c, &bs, &mut notes)?;
    }

    sync_main(&log, "main", "the log")?;
    rebase_onto(&h.wt, &h.path, "main", &bs)?;

    // The ticket steps, on the rebased branch so they sit on the newest main.
    let date = today();
    let mut marks: Vec<(crate::ticket::Ref, String)> = vec![];
    for r in flagged {
        marks.push((r, format!("{date}, {bs}")));
    }
    for (r, _) in &log_refs {
        marks.push((r.clone(), format!("{date}, {bs}")));
    }
    if let (Some(c), Some(range)) = (&code, &landed_code) {
        for (r, sha) in refs_in(&c.repo, &format!("{}..{}", range.from, range.to)) {
            marks.push((r, format!("{date}, {} {sha}", range.repo)));
        }
    }
    if marks.is_empty() && named_ticket {
        if let Some(r) = single_open_step(&h, &h.branch) {
            marks.push((r, format!("{date}, {bs}")));
        }
    } else if named_ticket && !marks.iter().any(|(r, _)| r.project == h.branch.project && r.id == h.branch.task) {
        notes.push(format!(
            "this branch is named for ticket {bs}, but nothing said which step it finished; mark it with abot ticket step {bs}.<n> --done"
        ));
    }
    if !marks.is_empty() {
        mark_tickets_on_branch(&h, &bs, &marks, &date, &mut notes)?;
    }

    // Mark the outcome, unless the last commit already did. The Land commit
    // also records the code it carried (Code:), so the log's task opens onto
    // its exact commits; when there is code to record, it is always made.
    let last = h.wt.commit_message("HEAD")?;
    let already = trailers::get(&last, "Outcome").is_some_and(|o| o.eq_ignore_ascii_case("landed"));
    if !already || landed_code.is_some() {
        let mut msg = format!("Land {bs}");
        if let Some(w) = no_state_change.filter(|_| !already) {
            msg.push_str(&format!("\n\nNo state change: {}", w.trim()));
        }
        msg.push_str("\n\n");
        msg.push_str(&session_lines(&h.wt, &bs));
        if let Some(r) = &landed_code {
            msg.push_str(&format!("Code: {r}\n"));
        }
        msg.push_str("Outcome: landed");
        h.wt.run(&["commit", "-q", "--allow-empty", "-m", &msg])?;
    } else if let Some(w) = no_state_change {
        notes.push(format!("no state change: {w} (not recorded: the last commit already says landed)"));
    }

    log.run(&["merge", "-q", "--ff-only", &bs])?;
    let head = log.run(&["rev-parse", "--short", "main"])?;
    if log.has_origin()
        && let Err(e) = log.run(&["push", "-q", "origin", "main"])
    {
        bail!(
            "{bs} is on the log's local main ({head}) but the push failed: {e:#}. \
             Run git -C {} pull --rebase origin main && git -C {} push origin main (never force)",
            ctx.log.display(),
            ctx.log.display()
        );
    }
    cleanup_branch(ctx, &log, &h, true)?;

    println!("landed {bs} on main at {head}{}", if log.has_origin() { " and pushed" } else { "" });
    for n in notes {
        println!("{n}");
    }
    println!("removed {} and deleted {bs}", h.path.display());
    println!("the worktree you were in is gone; to continue:");
    println!("cd {}", ctx.log.display());
    Ok(())
}

/// `Ready: <project>/<id>[.<n>]` lines in the commits of `range`, each with
/// its commit's short sha.
fn refs_in(g: &Git, range: &str) -> Vec<(crate::ticket::Ref, String)> {
    let log = g.try_run(&["log", "--format=%h%x1f%B%x1e", range]).unwrap_or_default();
    let mut out = vec![];
    for c in log.split('\x1e') {
        let Some((sha, msg)) = c.trim_start_matches('\n').split_once('\x1f') else { continue };
        for r in crate::ticket::ready_refs(msg) {
            if !out.iter().any(|(x, _): &(crate::ticket::Ref, String)| *x == r) {
                out.push((r, sha.trim().to_string()));
            }
        }
    }
    out
}

fn ticket_text(h: &Here, project: &str) -> String {
    std::fs::read_to_string(h.path.join(crate::ticket::path(project))).unwrap_or_default()
}

/// Whether the branch `<project>/<task>` is named for an open ticket.
fn ticket_named_by_branch(h: &Here, b: &TaskBranch) -> bool {
    crate::ticket::parse(&b.project, &ticket_text(h, &b.project)).find(&b.task).is_some()
}

/// The branch's ticket's one open step, when it has exactly one.
fn single_open_step(h: &Here, b: &TaskBranch) -> Option<crate::ticket::Ref> {
    let f = crate::ticket::parse(&b.project, &ticket_text(h, &b.project));
    let t = f.find(&b.task)?;
    let open: Vec<usize> = t.steps.iter().enumerate().filter(|(_, s)| !s.done).map(|(i, _)| i + 1).collect();
    (open.len() == 1).then(|| crate::ticket::Ref { project: b.project.clone(), id: b.task.clone(), step: Some(open[0]) })
}

/// Mark the steps in the worktree's ticket files and commit them on the branch.
fn mark_tickets_on_branch(h: &Here, bs: &str, marks: &[(crate::ticket::Ref, String)], date: &str, notes: &mut Vec<String>) -> Result<()> {
    let mut projects: Vec<String> = marks.iter().map(|(r, _)| r.project.clone()).collect();
    projects.sort();
    projects.dedup();
    let mut changed_all = vec![];
    let mut files = vec![];
    for p in projects {
        let f = crate::ticket::path(&p);
        let old = ticket_text(h, &p);
        let (new, changed, missing) = super::ticket::apply_marks(&p, &old, marks, date);
        for m in missing {
            notes.push(format!("ticket not marked: {m}"));
        }
        if changed.is_empty() {
            continue;
        }
        std::fs::write(h.path.join(&f), new)?;
        h.wt.run(&["add", "--", &f])?;
        files.push(f);
        changed_all.extend(changed);
    }
    if changed_all.is_empty() {
        return Ok(());
    }
    let mut msg = format!("Ticket steps done by {bs}\n\n");
    for c in &changed_all {
        msg.push_str(&format!("- {c}\n"));
        notes.push(format!("ticket: {c}"));
    }
    let mut args = vec!["commit", "-q", "-m", msg.as_str(), "--"];
    args.extend(files.iter().map(String::as_str));
    h.wt.run(&args)?;
    Ok(())
}

/// Land the code branch, and return the range it put on the trunk
/// (`<trunk before>..<trunk after>`), None when the branch carried nothing.
fn land_code(c: &Code, bs: &str, notes: &mut Vec<String>) -> Result<Option<trailers::CodeRange>> {
    let cwt = c.wt.as_ref().expect("checked");
    let wg = Git::new(cwt);
    sync_main(&c.repo, &c.trunk, "the code repo")?;
    let before = c.repo.run(&["rev-parse", "--short=7", &c.trunk])?;
    rebase_onto(&wg, cwt, &c.trunk, bs)?;
    c.repo.run(&["merge", "-q", "--ff-only", bs])?;
    let head = c.repo.run(&["rev-parse", "--short=7", &c.trunk])?;
    if c.repo.has_origin() {
        c.repo
            .run(&["push", "-q", "origin", &c.trunk])
            .map_err(|e| anyhow!("{bs} is on the code repo's local {} ({head}) but the push failed: {e:#}. Nothing in the log was changed.", c.trunk))?;
    }
    c.repo.run(&["worktree", "remove", &display(cwt)])?;
    delete_merged(&c.repo, bs, &c.trunk)?;
    if c.repo.has_origin() && c.repo.origin_has_branch(bs).unwrap_or(false) {
        let _ = c.repo.run(&["push", "-q", "origin", "--delete", bs]);
    }
    notes.push(format!(
        "landed the code branch on {} {} at {head}, removed {}",
        c.repo.dir.display(),
        c.trunk,
        cwt.display()
    ));
    Ok((before != head).then(|| trailers::CodeRange { repo: code_name(c), from: before, to: head }))
}

fn rebase_onto(wt: &Git, path: &Path, onto: &str, bs: &str) -> Result<()> {
    if let Err(e) = wt.run(&["rebase", "-q", onto]) {
        let conflicts = wt.try_run(&["diff", "--name-only", "--diff-filter=U"]).unwrap_or_default();
        let _ = wt.run(&["rebase", "--abort"]);
        bail!(
            "{bs} doesn't rebase cleanly on {onto}{}: {e:#}. Resolve it by hand in {}: \
             git rebase {onto}, keep what is true now (not what is newer), git rebase --continue, then land again",
            if conflicts.is_empty() { String::new() } else { format!(" (conflicts in {})", conflicts.replace('\n', ", ")) },
            path.display()
        );
    }
    Ok(())
}

/// Remove the log worktree, and delete the branch locally and on origin
/// (`delete`) or keep it (park).
fn cleanup_branch(ctx: &Ctx, log: &Git, h: &Here, delete: bool) -> Result<()> {
    // We may be standing in the worktree we are about to remove.
    let _ = std::env::set_current_dir(&ctx.log);
    log.run(&["worktree", "remove", &display(&h.path)])?;
    if delete {
        let bs = h.branch.to_string();
        delete_merged(log, &bs, "main")?;
        if log.has_origin() && log.origin_has_branch(&bs).unwrap_or(false) {
            log.run(&["push", "-q", "origin", "--delete", &bs])?;
        }
    }
    Ok(())
}

/// Delete a local branch that main now contains. `git branch -d` would
/// also want it merged into its upstream, which a landed branch never is,
/// so the containment check is made here and the delete is then plain.
fn delete_merged(g: &Git, bs: &str, trunk: &str) -> Result<()> {
    if !g.ok(&["merge-base", "--is-ancestor", bs, trunk]) {
        bail!("{bs} is not contained in {trunk} in {}; not deleting it", g.dir.display());
    }
    g.run(&["branch", "-q", "-D", bs])?;
    Ok(())
}

/// The main checkout's copy of the state file must be clean to commit to it.
fn state_on_main(ctx: &Ctx, log: &Git, project: &str) -> Result<Option<String>> {
    let f = ctx.state_path_at("main", project);
    if let Some(f) = &f
        && log.path_dirty(f)?
    {
        bail!("the log's main checkout has uncommitted changes to {f}; that is someone's work in the wrong place, sort it out first");
    }
    Ok(f)
}

/// Add one line under a section of the state file on main, and commit just
/// that path.
fn add_to_state_on_main(ctx: &Ctx, log: &Git, f: &str, section: &str, line: &str, msg: &str) -> Result<()> {
    let path = ctx.log.join(f);
    let text = std::fs::read_to_string(&path)?;
    std::fs::write(&path, statefile::add_line(&text, section, line))?;
    log.run(&["add", "--", f])?;
    log.run(&["commit", "-q", "-m", msg, "--", f])?;
    Ok(())
}

// ---------------------------------------------------------------- dead-end

pub fn dead_end(ctx: &Ctx, tried: &str, failed: &str, instead: &str, repo: Option<&Path>) -> Result<()> {
    for (k, v) in [("--tried", tried), ("--failed", failed), ("--instead", instead)] {
        if v.trim().is_empty() {
            bail!("{k} is empty: a dead end lands with what we tried, why it failed, and what to do instead");
        }
    }
    let h = here(ctx)?;
    let bs = h.branch.to_string();
    if h.wt.is_dirty()? {
        bail!(
            "uncommitted changes in {}; commit what belongs in the record, or drop it, then run dead-end again",
            h.path.display()
        );
    }
    let log = ctx.log_git();
    require_on_main(&log, "the log's main checkout")?;
    let state = state_on_main(ctx, &log, &h.branch.project)?;
    let code = code_side(ctx, &h.branch, repo)?;

    // The lesson is the branch's last commit: diwa indexes it, and skips merges.
    // It names the code that was tried (Code:), which the branch keeps.
    let tried_code = code.as_ref().and_then(|c| branch_range(c, &bs)).map(|r| format!("Code: {r}\n")).unwrap_or_default();
    let msg = format!(
        "Dead end: {bs}. {}\n\nTried: {}\n\nFailed: {}\n\nInstead: {}\n\n{}{tried_code}Outcome: dead-end",
        clip(one_line(failed).trim_end_matches('.'), 100),
        tried.trim(),
        failed.trim(),
        instead.trim(),
        session_lines(&h.wt, &bs),
    );
    h.wt.run(&["commit", "-q", "--allow-empty", "-m", &msg])?;
    let lesson = h.wt.run(&["rev-parse", "--short", "HEAD"])?;

    sync_main(&log, "main", "the log")?;
    log.run(&["merge", "-q", "-s", "ours", "--no-ff", &bs, "-m", &format!("Dead end: {bs}")])?;
    let mut notes = vec![];
    match &state {
        Some(f) => {
            let line = format!("- {} {}: {}", today(), h.branch.task, flatten(instead));
            let m = format!(
                "Trap from the {bs} dead end: {}\n\nThe lesson is {lesson}, the branch's last commit, merged into main with -s ours.",
                clip(&flatten(instead), 100)
            );
            add_to_state_on_main(ctx, &log, f, "Traps", &line, &m)?;
            notes.push(format!("added under Traps in {f}: {line}"));
        }
        None => notes.push(format!("no state file for `{}`, so no Traps line", h.branch.project)),
    }
    let head = log.run(&["rev-parse", "--short", "main"])?;
    if log.has_origin()
        && let Err(e) = log.run(&["push", "-q", "origin", "main"])
    {
        bail!(
            "the dead end is on the log's local main ({head}) but the push failed: {e:#}. \
             Run git -C {} pull --rebase origin main && git -C {} push origin main (never force), \
             then remove {} and delete {bs}",
            ctx.log.display(),
            ctx.log.display(),
            h.path.display()
        );
    }
    cleanup_branch(ctx, &log, &h, true)?;

    if let Some(c) = &code {
        if c.repo.has_origin() {
            match c.repo.run(&["push", "-q", "-u", "origin", &bs]) {
                Ok(_) => notes.push(format!("code branch {bs} pushed to {}'s origin and kept, as it was", c.repo.dir.display())),
                Err(e) => notes.push(format!("code branch {bs} kept, but the push failed: {e:#}")),
            }
        } else {
            notes.push(format!("code branch {bs} kept in {}", c.repo.dir.display()));
        }
        remove_code_worktree(c, &mut notes);
    }

    println!("dead end {bs}: lesson {lesson} merged into main with -s ours at {head}{}", if log.has_origin() { ", pushed" } else { "" });
    for n in notes {
        println!("{n}");
    }
    println!("removed {} and deleted {bs}", h.path.display());
    println!("the worktree you were in is gone; to continue:");
    println!("cd {}", ctx.log.display());
    Ok(())
}

// ---------------------------------------------------------------- park

pub fn park(ctx: &Ctx, why: &str, repo: Option<&Path>) -> Result<()> {
    if why.trim().is_empty() {
        bail!("say why it waits: abot park \"<why>\"");
    }
    let h = here(ctx)?;
    let bs = h.branch.to_string();
    if h.wt.is_dirty()? {
        bail!("uncommitted changes in {}; commit them first, so the parked branch holds them", h.path.display());
    }
    let log = ctx.log_git();
    require_on_main(&log, "the log's main checkout")?;
    let state = state_on_main(ctx, &log, &h.branch.project)?;
    let code = code_side(ctx, &h.branch, repo)?;

    let kept = code.as_ref().and_then(|c| branch_range(c, &bs)).map(|r| format!("Code: {r}\n")).unwrap_or_default();
    let msg = format!("Park {bs}: {}\n\n{}{kept}Outcome: parked", flatten(why), session_lines(&h.wt, &bs));
    h.wt.run(&["commit", "-q", "--allow-empty", "-m", &msg])?;
    if h.wt.has_origin() {
        h.wt.run(&["push", "-q", "-u", "origin", &bs])
            .map_err(|e| anyhow!("could not push {bs}, so parking it would leave it on this machine only: {e:#}"))?;
    }

    sync_main(&log, "main", "the log")?;
    let mut notes = vec![];
    match &state {
        Some(f) => {
            let line = format!("- {} {bs}: {}", today(), flatten(why));
            let m = format!("Park {bs}: {}\n\nThe branch is on origin; it waits until someone lands it or calls it a dead end.", clip(&flatten(why), 100));
            add_to_state_on_main(ctx, &log, f, "Parked", &line, &m)?;
            notes.push(format!("added under Parked in {f}: {line}"));
            if log.has_origin() {
                log.run(&["push", "-q", "origin", "main"]).map_err(|e| {
                    anyhow!("the Parked line is on the log's local main but the push failed: {e:#}. Run git -C {} push origin main", ctx.log.display())
                })?;
            }
        }
        None => notes.push(format!("no state file for `{}`, so no Parked line", h.branch.project)),
    }
    cleanup_branch(ctx, &log, &h, false)?;

    if let Some(c) = &code {
        if c.repo.has_origin() {
            match c.repo.run(&["push", "-q", "-u", "origin", &bs]) {
                Ok(_) => notes.push(format!("code branch {bs} pushed to {}'s origin and kept", c.repo.dir.display())),
                Err(e) => notes.push(format!("code branch {bs} kept, but the push failed: {e:#}")),
            }
        }
        remove_code_worktree(c, &mut notes);
    }

    println!("parked {bs}: {}", flatten(why));
    for n in notes {
        println!("{n}");
    }
    println!("removed {}; the branch is kept{}", h.path.display(), if log.has_origin() { " and on origin" } else { "" });
    println!("pick it up again: git -C {} worktree add {} {bs}", ctx.log.display(), h.path.display());
    println!("the worktree you were in is gone; to continue:");
    println!("cd {}", ctx.log.display());
    Ok(())
}
