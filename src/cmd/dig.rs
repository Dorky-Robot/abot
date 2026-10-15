//! dig: stand on one commit. Its message is the mini blog; its `Refs:`
//! point into code repos; diwa holds what was understood about it later.

use crate::ctx::Ctx;
use crate::diwa;
use crate::git::Git;
use crate::out::Out;
use crate::repos;
use crate::trailers;
use anyhow::{Result, bail};

pub fn run(ctx: &Ctx, sha: &str) -> Result<()> {
    if sha.is_empty() || !sha.chars().all(|c| c.is_ascii_hexdigit()) || sha.len() < 4 {
        bail!("`{sha}` is not a commit sha (at least 4 hex digits)");
    }
    let log = ctx.require_log()?;
    let mut o = Out::new(ctx.tty);
    // Fetch first, so a commit that is on origin but not yet here is found.
    if let Some(n) = ctx.read_from_origin()? {
        o.line(format!("({n})"));
    }
    let commit = format!("{sha}^{{commit}}");

    let (g, slug, where_) = if log.has_ref(&commit) {
        (log.clone(), ctx.log_slug(), format!("the log ({})", ctx.log.display()))
    } else {
        let found = ctx.code_repos().into_iter().find(|r| Git::new(&r.path).has_ref(&commit));
        match found {
            Some(r) => {
                let slug = r.origin.clone().unwrap_or_else(|| r.path.display().to_string());
                (Git::new(&r.path), slug, format!("{}", r.path.display()))
            }
            None => bail!(
                "no commit {sha} in the log or in any repo under {}",
                ctx.projects.display()
            ),
        }
    };

    let full = g.run(&["rev-parse", &commit])?;
    o.header(&format!("{} in {where_}", &full[..full.len().min(12)]));
    let show = g.run(&["show", "-s", "--date=short", "--format=%h  %ad  %an%n%n%B", &full])?;
    o.block(show.lines().map(|l| l.trim_end().to_string()), 80, "git show");

    let msg = g.commit_message(&full)?;
    let refs = trailers::refs(&msg);
    if !refs.is_empty() {
        o.header("Refs");
        let all = ctx.code_repos();
        for r in refs {
            let Some((name, rsha)) = r.split_once('@') else {
                o.line(format!("{r}: (not <org/repo>@<sha>)"));
                continue;
            };
            let line = match repos::find_by_origin(&all, name) {
                Some(repo) => Git::new(&repo.path)
                    .try_run(&["show", "-s", "--date=short", "--format=%h %ad %s", &format!("{rsha}^{{commit}}")])
                    .map(|s| format!("{r}: {s}  ({})", repo.path.display()))
                    .unwrap_or_else(|| format!("{r}: not in {} (fetch it?)", repo.path.display())),
                None => format!("{r}: no clone of {name} under {}", ctx.projects.display()),
            };
            o.line(line);
        }
    }

    // Code: the exact commits a task carried (abot land writes it; a dead end
    // and a park name the branch they kept), each with what diwa learned on it.
    let ranges = trailers::code(&msg);
    if !ranges.is_empty() {
        let all = ctx.code_repos();
        for r in ranges {
            o.header(&format!("Code {r}"));
            let Some(repo) = repos::find_by_origin(&all, &r.repo) else {
                o.line(format!("no clone of {} under {}", r.repo, ctx.projects.display()));
                continue;
            };
            let cg = Git::new(&repo.path);
            let span = format!("{}..{}", r.from, r.to);
            let Some(list) = cg.try_run(&["log", "--reverse", "--date=short", "--format=%h%x1f%ad%x1f%s", &span]) else {
                o.line(format!("{span} is not in {} (fetch it?)", repo.path.display()));
                continue;
            };
            let commits: Vec<&str> = list.lines().filter(|l| !l.is_empty()).collect();
            const SHOWN: usize = 20;
            for line in commits.iter().take(SHOWN) {
                let f: Vec<&str> = line.split('\x1f').collect();
                let (h, d, s) = (f[0], f.get(1).copied().unwrap_or(""), f.get(2).copied().unwrap_or(""));
                o.line(format!("{h}  {d}  {s}"));
                let insights = diwa::json(ctx, &["show", &r.repo, h])
                    .and_then(|v| v.get("insights").and_then(|i| i.as_array()).cloned())
                    .unwrap_or_default();
                for i in insights.iter().take(4) {
                    let cat = i.get("category").and_then(|c| c.as_str()).unwrap_or("");
                    let title = i.get("title").and_then(|t| t.as_str()).unwrap_or("");
                    o.line(format!("    ◆ {cat}: {title}"));
                }
            }
            if commits.len() > SHOWN {
                o.line(format!("… {} more: git -C {} log {span}", commits.len() - SHOWN, repo.path.display()));
            }
            o.line(format!("(in {})", repo.path.display()));
        }
    }

    o.header(&format!("diwa show {slug} {}", &full[..7.min(full.len())]));
    o.block(diwa::lines(ctx, &["show", &slug, &full[..12.min(full.len())]]), 60, "diwa show");
    o.print(usize::MAX);
    Ok(())
}
