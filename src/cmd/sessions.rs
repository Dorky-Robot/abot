//! sessions: which Claude Code session worked which task, and how it ended.
//! `abot start` records the session as a `Session:` trailer on the Start
//! commit, and land, dead-end and park carry it onto the commit that ends
//! the task; the newest commit naming a session says where its task stands.
//! The session sweeper reads this to know whose work is finished.

use crate::ctx::Ctx;
use crate::trailers;
use anyhow::Result;
use serde_json::json;

pub struct Row {
    pub session: String,
    /// who started it: the `Parent:` trailer on the commit that names it, when its spawner passed one
    pub parent: Option<String>,
    pub outcome: String,
    pub branch: String,
    pub sha: String,
    pub date: String,
}

/// The branch a commit's subject names: `Start <b>:`, `Land <b>`,
/// `Dead end: <b>.`, `Park <b>:`.
fn branch_of(subject: &str) -> String {
    let rest = ["Start ", "Land ", "Dead end: ", "Park "].iter().find_map(|p| subject.strip_prefix(p)).unwrap_or("");
    rest.split([' ', ':']).next().unwrap_or("").trim_end_matches('.').to_string()
}

pub fn rows(ctx: &Ctx) -> Result<(Vec<Row>, Option<String>)> {
    let g = ctx.require_log()?;
    let note = ctx.read_from_origin()?;
    let base = ctx.base();
    // main and every task branch: a parked branch's Park commit is only there.
    let others = if base.starts_with("origin/") { "--remotes=origin" } else { "--branches" };
    let log = g.run(&["log", "--format=%h%x00%cs%x00%s%x00%B%x1e", &base, others])?;
    let mut out: Vec<Row> = vec![];
    for c in log.split('\x1e') {
        let f: Vec<&str> = c.trim_start_matches('\n').splitn(4, '\0').collect();
        if f.len() < 4 {
            continue;
        }
        let t = trailers::parse(f[3]);
        let outcome = t.iter().rev().find(|(k, _)| k.eq_ignore_ascii_case("Outcome")).map(|(_, v)| v.clone());
        let parent = t.iter().find(|(k, _)| k.eq_ignore_ascii_case("Parent")).map(|(_, v)| v.clone());
        for (k, v) in &t {
            if !k.eq_ignore_ascii_case("Session") {
                continue;
            }
            // the newest commit naming a session says how it stands; its parent is on the Start
            // commit, older, so it is filled in from whichever commit carries it
            if let Some(r) = out.iter_mut().find(|r| &r.session == v) {
                if r.parent.is_none() {
                    r.parent = parent.clone();
                }
                continue;
            }
            out.push(Row {
                session: v.clone(),
                parent: parent.clone(),
                outcome: outcome.clone().unwrap_or_else(|| "in-flight".into()),
                branch: branch_of(f[2]),
                sha: f[0].into(),
                date: f[1].into(),
            });
        }
    }
    Ok((out, note))
}

pub fn run(ctx: &Ctx, json_out: bool) -> Result<()> {
    let (rows, note) = rows(ctx)?;
    if json_out {
        let v: Vec<_> = rows
            .iter()
            .map(|r| json!({"session": r.session, "parent": r.parent, "outcome": r.outcome, "branch": r.branch, "sha": r.sha, "date": r.date}))
            .collect();
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    if let Some(n) = note {
        println!("({n})");
    }
    if rows.is_empty() {
        println!("(nothing: no commit carries a Session: trailer yet)");
    }
    for r in rows {
        println!("{}  {:<9}  {}  {} {}", r.session, r.outcome, r.branch, r.sha, r.date);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::branch_of;

    #[test]
    fn branch_from_subjects() {
        assert_eq!(branch_of("Start org/x: why it matters"), "org/x");
        assert_eq!(branch_of("Land org/x"), "org/x");
        assert_eq!(branch_of("Dead end: org/x. it failed"), "org/x");
        assert_eq!(branch_of("Park org/x: waits"), "org/x");
        assert_eq!(branch_of("something else"), "");
    }
}
