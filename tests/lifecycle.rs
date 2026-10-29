//! A scratch world: a log and a code repo, each with a bare "origin", in a
//! temp dir. Every test drives the real binary against it.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    log: PathBuf,
    code: PathBuf,
    log_origin: PathBuf,
    code_origin: PathBuf,
}

const STATE: &str = "# demo\nUses: kita, pocket-id\nServes: Sara\n\n## What it is\nA demo project.\n\n## Live now\n- dr1, v1\n\n## Next\n1. ship\n\n## Waits on Felix\n- pick a name\n\n## Traps\n- 2026-01-01 old: an old trap\n\n## Parked\n";

/// abot with nothing of the Claude session running the tests: land, dead-end
/// and park would otherwise close that session (a `claude stop` on the job in
/// $CLAUDE_JOB_DIR, or on the job whose state.json names $CLAUDE_CODE_SESSION_ID).
fn abot_cmd() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_abot"));
    for k in ["CLAUDE_JOB_DIR", "CLAUDECODE", "CLAUDE_CODE_SESSION_ID", "CLAUDE_CODE_BRIDGE_SESSION_ID", "CLAUDE_PARENT_SESSION_ID", "CLAUDE_CONFIG_DIR"] {
        c.env_remove(k);
    }
    c
}

fn git_env(c: &mut Command, home: &Path) {
    c.env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@example.com");
}

fn git(dir: &Path, home: &Path, args: &[&str]) -> String {
    let mut c = Command::new("git");
    c.arg("-C").arg(dir).args(args);
    git_env(&mut c, home);
    let o = c.output().unwrap();
    assert!(o.status.success(), "git {:?} in {}: {}", args, dir.display(), String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim_end().to_string()
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let projects = root.join("Projects");
        let remotes = root.join("remotes");
        std::fs::create_dir_all(&projects).unwrap();
        std::fs::create_dir_all(&remotes).unwrap();
        let log_origin = remotes.join("ecosystem-log.git");
        let code_origin = remotes.join("demo.git");
        for o in [&log_origin, &code_origin] {
            git(&root, &root, &["init", "-q", "--bare", "-b", "main", o.to_str().unwrap()]);
        }
        let log = projects.join("ecosystem-log");
        let code = projects.join("dev").join("demo");
        std::fs::create_dir_all(code.parent().unwrap()).unwrap();
        git(&root, &root, &["clone", "-q", log_origin.to_str().unwrap(), log.to_str().unwrap()]);
        git(&root, &root, &["clone", "-q", code_origin.to_str().unwrap(), code.to_str().unwrap()]);
        for r in [&log, &code] {
            git(r, &root, &["checkout", "-q", "-B", "main"]);
        }
        std::fs::create_dir_all(log.join("state/projects")).unwrap();
        std::fs::write(log.join("state/projects/demo.md"), STATE).unwrap();
        std::fs::write(log.join("state/org.md"), "d-1. the org\n").unwrap();
        git(&log, &root, &["add", "."]);
        git(&log, &root, &["commit", "-q", "-m", "Seed the log"]);
        git(&log, &root, &["push", "-q", "-u", "origin", "main"]);
        std::fs::write(code.join("README.md"), "demo\n").unwrap();
        git(&code, &root, &["add", "."]);
        git(&code, &root, &["commit", "-q", "-m", "Seed the code"]);
        git(&code, &root, &["push", "-q", "-u", "origin", "main"]);
        World { _tmp: tmp, root, log, code, log_origin, code_origin }
    }

    fn abot_in(&self, cwd: &Path, args: &[&str]) -> Output {
        self.abot_env(cwd, args, &[])
    }

    fn abot_env(&self, cwd: &Path, args: &[&str], extra: &[(&str, &str)]) -> Output {
        let mut c = abot_cmd();
        c.args(args)
            .current_dir(cwd)
            .env("ABOT_LOG", &self.log)
            .env("ABOT_PROJECTS", self.root.join("Projects"))
            .env("ABOT_DECISIONS", self.root.join("decisions.json"))
            .env("ABOT_DIWA", "")
            .env("NO_COLOR", "1")
            .env("ABOT_CLAUDE", self.root.join("fake-claude"))
            .env("FAKE_DIR", &self.root)
            .env_remove("ABOT_TICKET_MODEL")
            .env_remove("CLAUDE_CODE_BRIDGE_SESSION_ID");
        git_env(&mut c, &self.root);
        for (k, v) in extra {
            c.env(k, v);
        }
        c.output().unwrap()
    }

    fn abot(&self, args: &[&str]) -> Output {
        self.abot_in(&self.log, args)
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        git(dir, &self.root, args)
    }

    fn wt(&self, task: &str) -> PathBuf {
        self.root.join("Projects").join(format!("ecosystem-log-{task}"))
    }

    fn code_wt(&self, task: &str) -> PathBuf {
        self.root.join("Projects/dev").join(format!("demo-{task}"))
    }

    fn origin_branches(&self, origin: &Path) -> String {
        self.git(origin, &["for-each-ref", "--format=%(refname:short)", "refs/heads/"])
    }

    fn commit(&self, dir: &Path, file: &str, text: &str, msg: &str) {
        let p = dir.join(file);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
        self.git(dir, &["add", file]);
        self.git(dir, &["commit", "-q", "-m", msg]);
    }
}

fn ok(o: &Output) -> String {
    assert!(
        o.status.success(),
        "expected success\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn fails(o: &Output) -> String {
    assert!(!o.status.success(), "expected failure\nstdout: {}", String::from_utf8_lossy(&o.stdout));
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn start_then_land() {
    let w = World::new();
    let out = ok(&w.abot(&["start", "demo/feat", "try the feature"]));
    assert!(out.contains("started demo/feat"), "{out}");
    let wt = w.wt("feat");
    let cwt = w.code_wt("feat");
    assert!(wt.exists() && cwt.exists());
    assert!(w.origin_branches(&w.log_origin).contains("demo/feat"));
    let first = w.git(&wt, &["log", "-1", "--format=%B"]);
    assert!(first.starts_with("Start demo/feat: try the feature"), "{first}");
    assert!(first.contains("Outcome: in-flight"));

    // Work in both repos.
    let state = std::fs::read_to_string(wt.join("state/projects/demo.md")).unwrap();
    w.commit(&wt, "state/projects/demo.md", &state.replace("- dr1, v1", "- dr1, v2"), "v2 is live");
    w.commit(&cwt, "feature.txt", "x\n", "Add the feature");

    let out = ok(&w.abot_in(&wt, &["land"]));
    assert!(out.contains("landed demo/feat"), "{out}");
    assert_eq!(out.lines().last().unwrap(), format!("cd {}", w.log.display()), "the last line is runnable");
    assert!(out.contains("landed the code branch"), "{out}");

    // Log: main has start, change and land; pushed; branch and worktree gone.
    let log = w.git(&w.log_origin, &["log", "--format=%s", "main"]);
    let subjects: Vec<&str> = log.lines().collect();
    assert_eq!(subjects[..3], ["Land demo/feat", "v2 is live", "Start demo/feat: try the feature"]);
    let land = w.git(&w.log_origin, &["log", "-1", "--format=%B", "main"]);
    assert!(land.contains("Outcome: landed"));
    assert!(!wt.exists() && !cwt.exists());
    assert!(!w.origin_branches(&w.log_origin).contains("demo/feat"));
    assert!(w.git(&w.log, &["branch", "--list", "demo/feat"]).is_empty());
    // Code: main has the feature on origin, branch gone.
    let code = w.git(&w.code_origin, &["log", "--format=%s", "main"]);
    assert!(code.starts_with("Add the feature"), "{code}");
    assert!(w.git(&w.code, &["branch", "--list", "demo/feat"]).is_empty());
}

#[test]
fn start_refuses_what_it_should() {
    let w = World::new();
    assert!(fails(&w.abot(&["start", "demo", "why"])).contains("bare project"));
    assert!(fails(&w.abot(&["start", "nope/x", "why"])).contains("no state file"));
    assert!(fails(&w.abot(&["start", "demo/x", " "])).contains("missing WHY"));
    assert!(fails(&w.abot(&["start", "demo/x"])).contains("missing WHY: abot start demo/x \"<why>\""));
    let help = ok(&w.abot(&["start", "--help"]));
    assert!(help.contains("Usage: abot start [OPTIONS] <project>/<task> \"<why>\""), "{help}");
    // Someone else has the branch on origin.
    w.git(&w.log, &["branch", "demo/taken"]);
    w.git(&w.log, &["push", "-q", "origin", "demo/taken"]);
    w.git(&w.log, &["branch", "-D", "demo/taken"]);
    assert!(fails(&w.abot(&["start", "demo/taken", "why"])).contains("already exists on origin"));
    assert!(!w.wt("taken").exists());
    // --new scaffolds a state file and refuses an existing one.
    assert!(fails(&w.abot(&["start", "demo/y", "why", "--new"])).contains("drop --new"));
    let out = ok(&w.abot(&["start", "fresh/first", "a new project", "--new"]));
    assert!(out.contains("code repo:     none (log only; add a Repo: line or --repo if there is one)"), "{out}");
    let scaffold = std::fs::read_to_string(w.wt("first").join("state/projects/fresh.md")).unwrap();
    assert!(scaffold.starts_with("# fresh\nUses:"), "{scaffold}");
    assert!(scaffold.contains("## Waits on Felix"));
}

#[test]
fn land_refuses_without_state_change_and_with_secrets() {
    let w = World::new();
    ok(&w.abot(&["start", "demo/quiet", "nothing to say"]));
    let wt = w.wt("quiet");
    let e = fails(&w.abot_in(&wt, &["land"]));
    assert!(e.contains("changes nothing under state/"), "{e}");
    assert!(fails(&w.abot(&["land"])).contains("main checkout"));

    // A secret in the diff is refused.
    let state = std::fs::read_to_string(wt.join("state/projects/demo.md")).unwrap();
    let leaked = state.replace("- dr1, v1", "- dr1, token ghp_0123456789abcdefghijklmnopqrstuvwxyzAB");
    w.commit(&wt, "state/projects/demo.md", &leaked, "oops");
    let e = fails(&w.abot_in(&wt, &["land"]));
    assert!(e.contains("refusing to land") && e.contains("GitHub token") && !e.contains("0123456789abc"), "{e}");

    // Dirty worktree is refused; then fix and land with a reason.
    std::fs::write(wt.join("scratch.txt"), "x").unwrap();
    assert!(fails(&w.abot_in(&wt, &["land"])).contains("uncommitted"));
    std::fs::remove_file(wt.join("scratch.txt")).unwrap();
    w.commit(&wt, "state/projects/demo.md", &state, "Take the token back out");
    // Branch now nets to no state change.
    let e = fails(&w.abot_in(&wt, &["land"]));
    assert!(e.contains("changes nothing"), "{e}");
    // ...but the token is still in the branch's history, which the diff
    // against main no longer shows; landing with a reason is allowed.
    let out = ok(&w.abot_in(&wt, &["land", "--no-state-change", "only checking the tool"]));
    assert!(out.contains("landed demo/quiet"));
    let body = w.git(&w.log_origin, &["log", "-1", "--format=%B", "main"]);
    assert!(body.contains("No state change: only checking the tool") && body.contains("Outcome: landed"), "{body}");
}

#[test]
fn start_then_dead_end() {
    let w = World::new();
    ok(&w.abot(&["start", "demo/hunch", "maybe this works"]));
    let wt = w.wt("hunch");
    let cwt = w.code_wt("hunch");
    w.commit(&wt, "state/projects/demo.md", "# demo\nchanged on the branch\n", "Rewrite state as if it worked");
    w.commit(&cwt, "hunch.txt", "x\n", "Try the hunch");

    let e = fails(&w.abot_in(&wt, &["dead-end", "--tried", "a", "--failed", "b", "--instead", " "]));
    assert!(e.contains("--instead is empty"), "{e}");
    let missing = w.abot_in(&wt, &["dead-end", "--tried", "a", "--failed", "b"]);
    assert!(!missing.status.success());

    let before = w.git(&w.log, &["rev-parse", "main:state/projects/demo.md"]);
    let out = ok(&w.abot_in(
        &wt,
        &["dead-end", "--tried", "polling the API", "--failed", "rate limits at 10 rps.\nmore detail", "--instead", "use the webhook"],
    ));
    assert!(out.contains("dead end demo/hunch"), "{out}");
    assert!(out.contains("pushed to") && out.contains("kept"), "{out}");

    // main: merge -s ours (files unchanged by the branch), then a Traps commit.
    let subjects = w.git(&w.log_origin, &["log", "--format=%s", "-3", "main"]);
    let s: Vec<&str> = subjects.lines().collect();
    assert!(s[0].starts_with("Trap from the demo/hunch dead end: use the webhook"), "{subjects}");
    assert_eq!(s[1], "Dead end: demo/hunch");
    let lesson = w.git(&w.log_origin, &["log", "-1", "--format=%B", "main^^2"]);
    assert!(lesson.starts_with("Dead end: demo/hunch. rate limits at 10 rps\n"), "{lesson}");
    assert!(lesson.contains("Tried: polling the API") && lesson.contains("Instead: use the webhook") && lesson.contains("Outcome: dead-end"));
    let merged = w.git(&w.log_origin, &["rev-parse", "main^:state/projects/demo.md"]);
    assert_eq!(merged, before, "the -s ours merge must not change main's files");
    let state = w.git(&w.log_origin, &["show", "main:state/projects/demo.md"]);
    let traps_at = state.find("## Traps").unwrap();
    let line = format!("- {} hunch: use the webhook", chrono_today());
    assert!(state[traps_at..].starts_with(&format!("## Traps\n{line}\n- 2026-01-01 old")), "{state}");

    // Cleaned up; code branch kept and on origin.
    assert!(!wt.exists());
    assert!(!w.origin_branches(&w.log_origin).contains("demo/hunch"));
    assert!(w.origin_branches(&w.code_origin).contains("demo/hunch"));
    assert!(!cwt.exists());
    // orient and tree see the dead end.
    let o = ok(&w.abot(&["orient", "demo"]));
    assert!(o.contains("Dead end: demo/hunch. rate limits"), "{o}");
    let t = ok(&w.abot(&["tree", "demo"]));
    assert!(t.contains("[dead-end] Dead end: demo/hunch. rate limits"), "{t}");
    assert!(t.contains("[in-flight] Start demo/hunch"), "{t}");
}

fn chrono_today() -> String {
    let o = Command::new("date").arg("+%Y-%m-%d").output().unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

#[test]
fn start_then_park() {
    let w = World::new();
    ok(&w.abot(&["start", "demo/later", "not yet"]));
    let wt = w.wt("later");
    w.commit(&wt, "notes.md", "half done\n", "Half of it");
    let out = ok(&w.abot_in(&wt, &["park", "waits on Felix's call about names"]));
    assert!(out.contains("parked demo/later"), "{out}");
    assert!(!wt.exists());
    // Branch kept, locally and on origin, ending in a parked commit.
    assert!(w.origin_branches(&w.log_origin).contains("demo/later"));
    let tip = w.git(&w.log_origin, &["log", "-1", "--format=%B", "demo/later"]);
    assert!(tip.starts_with("Park demo/later: waits on Felix's call") && tip.contains("Outcome: parked"), "{tip}");
    let state = w.git(&w.log_origin, &["show", "main:state/projects/demo.md"]);
    assert!(state.contains(&format!("## Parked\n- {} demo/later: waits on Felix's call about names", chrono_today())), "{state}");
    // map shows it in flight; tree shows parked.
    let m = ok(&w.abot(&["map", "--json"]));
    let v: serde_json::Value = serde_json::from_str(&m).unwrap();
    let demo = &v["projects"][0];
    assert_eq!(demo["project"], "demo");
    assert_eq!(demo["in_flight"][0], "demo/later");
    let t = ok(&w.abot(&["tree", "demo"]));
    assert!(t.contains("[parked] Park demo/later"), "{t}");
}

#[test]
fn map_reads_state_files() {
    let w = World::new();
    let m = ok(&w.abot(&["map", "--json"]));
    let v: serde_json::Value = serde_json::from_str(&m).unwrap();
    let demo = &v["projects"][0];
    assert_eq!(demo["live"], "dr1, v1");
    assert_eq!(demo["next"], "ship");
    assert_eq!(demo["waits_on_felix"][0], "pick a name");
    assert_eq!(demo["uses"], serde_json::json!(["kita", "pocket-id"]));
    assert_eq!(demo["serves"], serde_json::json!(["Sara"]));
    assert_eq!(demo["dead_ends"], 0);
    let text = ok(&w.abot(&["map"]));
    assert!(text.contains("PROJECT") && text.contains("demo") && text.contains("demo: pick a name"), "{text}");
}

#[test]
fn check_finds_problems() {
    let w = World::new();
    let out = ok(&w.abot(&["check"]));
    assert!(out.starts_with("ok: 1 state file"), "{out}");

    w.commit(&w.log, "state/projects/bad.md", "# bad\n## Next\n", "A malformed state file");
    w.commit(&w.log, "state/leak.md", "call 512-555-0134\n", "A phone number");
    w.commit(&w.log, "docs/transcripts/t.md", "call 512-555-0134\n", "A transcript");
    w.git(&w.log, &["push", "-q", "origin", "main"]);
    w.git(&w.log, &["branch", "ghost/x"]);
    w.git(&w.log, &["push", "-q", "origin", "ghost/x"]);
    w.git(&w.log, &["branch", "stray"]);
    w.git(&w.log, &["push", "-q", "origin", "stray"]);
    let o = w.abot(&["check"]);
    assert!(!o.status.success());
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("state/projects/bad.md: missing headings: What it is"), "{out}");
    assert!(out.contains("state/leak.md:1: looks like a phone number"), "{out}");
    assert!(!out.contains("transcripts"), "{out}");
    assert!(out.contains("origin/ghost/x: no state file for project `ghost`"), "{out}");
    assert!(out.contains("origin/stray: not a <project>/<task> branch"), "{out}");

    let j = w.abot(&["check", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&j.stdout).unwrap();
    assert_eq!(v["ok"], false);
}

#[test]
fn check_flags_stale_branches() {
    let w = World::new();
    w.git(&w.log, &["checkout", "-q", "-b", "demo/old"]);
    let mut c = Command::new("git");
    c.arg("-C").arg(&w.log).args(["commit", "-q", "--allow-empty", "-m", "old work"]);
    git_env(&mut c, &w.root);
    c.env("GIT_COMMITTER_DATE", "2026-01-01T12:00:00").env("GIT_AUTHOR_DATE", "2026-01-01T12:00:00");
    assert!(c.output().unwrap().status.success());
    w.git(&w.log, &["push", "-q", "origin", "demo/old"]);
    w.git(&w.log, &["checkout", "-q", "main"]);
    let out = String::from_utf8_lossy(&w.abot(&["check"]).stdout).into_owned();
    assert!(out.contains("origin/demo/old: in flight for") && out.contains("2026-01-01"), "{out}");
}

#[test]
fn orient_prints_sections_and_nothing() {
    let w = World::new();
    let o = ok(&w.abot(&["orient", "demo"]));
    for s in ["== ecosystem", "(no ecosystem/ yet)", "== demo: state/projects/demo.md", "== how it got here", "== dead ends", "(nothing)", "== in flight: demo/*", "== code repo:", "diwa skipped"] {
        assert!(o.contains(s), "missing {s} in\n{o}");
    }
    // org lives at state/org.md, the older place, and still orients.
    let o = ok(&w.abot(&["orient", "org"]));
    assert!(o.contains("== org: state/org.md"), "{o}");
    let o = ok(&w.abot(&["orient", "demo", "--cross", "org"]));
    assert!(o.contains("== map: these projects") && o.contains("demo: uses kita, pocket-id"), "{o}");
    assert!(fails(&w.abot(&["orient", "Bad"])).contains("not a project slug"));
}

#[test]
fn decide_appends_and_mirror_lands_once() {
    let w = World::new();
    let file = w.root.join("decisions.json");
    assert!(fails(&w.abot(&["decide", "x", "--why", "y", "--about", "org"])).contains("on this machine"));
    std::fs::write(&file, "{\n \"v\": 1,\n \"decisions\": [\n  {\n   \"id\": \"d-9\",\n   \"at\": \"2026-10-01\",\n   \"by\": \"Felix\",\n   \"text\": \"Old one.\",\n   \"why\": \"w\",\n   \"about\": [\n    \"org\"\n   ]\n  }\n ]\n}").unwrap();
    let out = ok(&w.abot(&["decide", "Felix: \"use abot\". Then more.", "--why", "it is lighter", "--about", "org, demo"]));
    assert!(out.contains("recorded d-10"), "{out}");
    let raw = std::fs::read_to_string(&file).unwrap();
    assert!(raw.contains("  {\n   \"id\": \"d-10\",\n   \"at\": \""), "{raw}");
    assert!(raw.contains("\"about\": [\n    \"org\",\n    \"demo\"\n   ]"), "{raw}");
    assert!(!raw.ends_with('\n'));

    let out = ok(&w.abot(&["mirror"]));
    assert!(out.contains("mirrored 2 decision(s) of 2") && out.contains("pushed"), "{out}");
    let s = w.git(&w.log_origin, &["log", "--format=%s", "-2", "main"]);
    assert_eq!(s, "d-10: Felix: \"use abot\"\nd-9: Old one");
    let first = w.git(&w.log_origin, &["log", "-1", "--format=%ad", "--date=short", "main^"]);
    assert_eq!(first, "2026-10-01", "first mirror is dated when it was decided");
    assert!(ok(&w.abot(&["mirror"])).contains("mirrored 0 decision(s) of 2"));
    // Revising the JSON lands a revised commit.
    std::fs::write(&file, raw.replace("Old one.", "Old one, revised.")).unwrap();
    assert!(ok(&w.abot(&["mirror"])).contains("mirrored 1 decision(s)"));
    assert!(w.git(&w.log, &["log", "-1", "--format=%s"]).starts_with("d-9 revised: Old one, revised"));
    // A string `supersedes` mangled one character at a time by the old
    // Python mirror is stale: one revised commit, then nothing.
    let d9 = w.log.join("decisions/d-9.md");
    let with_sup = std::fs::read_to_string(&file).unwrap().replace("\"why\": \"w\",", "\"why\": \"w\",\n   \"supersedes\": \"d-2\",");
    std::fs::write(&file, &with_sup).unwrap();
    assert!(ok(&w.abot(&["mirror"])).contains("mirrored 1 decision(s)"));
    let good = std::fs::read_to_string(&d9).unwrap();
    assert!(good.contains("supersedes: d-2\n"), "{good}");
    std::fs::write(&d9, good.replace("supersedes: d-2", "supersedes: d, -, 2")).unwrap();
    w.git(&w.log, &["commit", "-q", "-am", "mangle as the old mirror did"]);
    assert!(ok(&w.abot(&["mirror"])).contains("mirrored 1 decision(s)"));
    assert!(std::fs::read_to_string(&d9).unwrap().contains("supersedes: d-2\n"));
    assert!(ok(&w.abot(&["mirror"])).contains("mirrored 0 decision(s)"));
    // The orient of org sees decisions about it.
    let o = ok(&w.abot(&["orient", "org"]));
    assert!(o.contains("d-10: Felix"), "{o}");
}

#[test]
fn dig_shows_a_commit_and_its_refs() {
    let w = World::new();
    let code_sha = w.git(&w.code, &["rev-parse", "--short", "HEAD"]);
    let msg = format!("Point at the code\n\nWhy it matters.\n\nRefs: remotes/demo@{code_sha}");
    w.git(&w.log, &["commit", "-q", "--allow-empty", "-m", &msg]);
    let sha = w.git(&w.log, &["rev-parse", "--short", "HEAD"]);
    let o = ok(&w.abot(&["dig", &sha]));
    assert!(o.contains("Point at the code") && o.contains("Why it matters."), "{o}");
    assert!(o.contains(&format!("remotes/demo@{code_sha}: {code_sha}")) && o.contains("Seed the code"), "{o}");
    // A sha only the code repo has.
    let o = ok(&w.abot(&["dig", &code_sha]));
    assert!(o.contains("Seed the code") && o.contains("dev/demo"), "{o}");
    assert!(fails(&w.abot(&["dig", "abcdef1234"])).contains("no commit"));
}

/// A second clone moves origin on; the World's own clone stays behind.
fn move_origin(w: &World) -> String {
    let other = w.root.join("other");
    w.git(&w.root, &["clone", "-q", w.log_origin.to_str().unwrap(), other.to_str().unwrap()]);
    let state = std::fs::read_to_string(other.join("state/projects/demo.md")).unwrap();
    w.commit(&other, "state/projects/demo.md", &state.replace("- dr1, v1", "- dr1, v9 from origin"), "Origin moved on");
    w.git(&other, &["push", "-q", "origin", "main"]);
    w.git(&other, &["push", "-q", "origin", "main:refs/heads/demo/elsewhere"]);
    w.git(&other, &["rev-parse", "--short", "HEAD"])
}

#[test]
fn reads_come_from_origin_not_the_clone() {
    let w = World::new();
    let local_main = w.git(&w.log, &["rev-parse", "main"]);
    let sha = move_origin(&w);
    // The local checkout is stale, dirty and detached; none of it may show.
    std::fs::write(w.log.join("state/projects/demo.md"), "# dirty\n").unwrap();
    w.git(&w.log, &["checkout", "-q", "--detach"]);

    let o = ok(&w.abot(&["orient", "demo"]));
    assert!(o.contains("origin/main at") && o.contains("- dr1, v9 from origin"), "{o}");
    assert!(o.contains("Origin moved on") && o.contains("demo/elsewhere"), "{o}");
    assert!(!o.contains("# dirty") && !o.contains("unreachable"), "{o}");

    let m: serde_json::Value = serde_json::from_slice(&w.abot(&["map", "--json"]).stdout).unwrap();
    assert_eq!(m["read_from"], "origin/main");
    assert_eq!(m["projects"][0]["live"], "dr1, v9 from origin");
    assert_eq!(m["projects"][0]["in_flight"][0], "demo/elsewhere");

    assert!(ok(&w.abot(&["tree", "demo"])).contains("Origin moved on"));
    assert!(ok(&w.abot(&["dig", &sha])).contains("Origin moved on"));
    assert!(ok(&w.abot(&["check"])).starts_with("ok: 1 state file"));
    // Reading never moved the clone's main.
    assert_eq!(w.git(&w.log, &["rev-parse", "main"]), local_main);
}

#[test]
fn reads_fall_back_loudly_when_origin_is_unreachable() {
    let w = World::new();
    w.git(&w.log, &["remote", "set-url", "origin", w.root.join("gone.git").to_str().unwrap()]);
    let o = ok(&w.abot(&["orient", "demo"]));
    assert!(o.contains("origin unreachable, reading local main as of 20"), "{o}");
    assert!(o.contains("- dr1, v1"), "{o}");
    assert!(ok(&w.abot(&["map"])).contains("origin unreachable"));
    assert!(ok(&w.abot(&["tree", "demo"])).contains("origin unreachable"));
    let c = w.abot(&["check"]);
    assert!(c.status.success() && String::from_utf8_lossy(&c.stderr).contains("origin unreachable"));
    // A write that needs origin refuses instead.
    assert!(fails(&w.abot(&["start", "demo/x", "why"])).contains("origin unreachable"));
}

#[test]
fn start_branches_from_origin_main() {
    let w = World::new();
    let local_main = w.git(&w.log, &["rev-parse", "main"]);
    move_origin(&w);
    let origin_main = w.git(&w.log_origin, &["rev-parse", "main"]);
    ok(&w.abot(&["start", "demo/fresh", "from the copy of record"]));
    let wt = w.wt("fresh");
    assert_eq!(w.git(&wt, &["rev-parse", "HEAD^"]), origin_main);
    assert!(std::fs::read_to_string(wt.join("state/projects/demo.md")).unwrap().contains("v9 from origin"));
    assert_eq!(w.git(&w.log, &["rev-parse", "main"]), local_main, "start leaves the main checkout alone");
    // Landing from a stale clone still works: land pulls main first.
    let state = std::fs::read_to_string(wt.join("state/projects/demo.md")).unwrap();
    w.commit(&wt, "state/projects/demo.md", &state.replace("v9 from origin", "v10"), "v10 is live");
    ok(&w.abot_in(&wt, &["land"]));
    assert!(w.git(&w.log_origin, &["show", "main:state/projects/demo.md"]).contains("v10"));
}

#[test]
fn a_closed_pipe_ends_quietly() {
    let w = World::new();
    let mut c = abot_cmd();
    c.args(["orient", "demo"])
        .current_dir(&w.log)
        .env("ABOT_LOG", &w.log)
        .env("ABOT_PROJECTS", w.root.join("Projects"))
        .env("ABOT_DIWA", "")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    git_env(&mut c, &w.root);
    let mut child = c.spawn().unwrap();
    drop(child.stdout.take()); // the reader goes away before abot writes
    let out = child.wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("panicked") && !err.contains("Broken pipe"), "{err}");
    assert_ne!(out.status.code(), Some(101), "no panic exit");
}

/// The Code: trailer: land writes the exact range it fast-forwarded, a dead
/// end and a park the branch they kept; dig follows it and tree carries it.
#[test]
fn the_log_records_the_code_each_task_carried() {
    let w = World::new();
    let code_before = w.git(&w.code, &["rev-parse", "--short=7", "main"]);

    // land: the range on the code trunk, in the Land commit
    ok(&w.abot(&["start", "demo/feat", "try the feature"]));
    let wt = w.wt("feat");
    let cwt = w.code_wt("feat");
    let state = std::fs::read_to_string(wt.join("state/projects/demo.md")).unwrap();
    w.commit(&wt, "state/projects/demo.md", &state.replace("- dr1, v1", "- dr1, v2"), "v2 is live");
    w.commit(&cwt, "a.txt", "a\n", "Add part one");
    w.commit(&cwt, "b.txt", "b\n", "Add part two");
    ok(&w.abot_in(&wt, &["land"]));
    let code_after = w.git(&w.code_origin, &["rev-parse", "--short=7", "main"]);
    let land = w.git(&w.log_origin, &["log", "-1", "--format=%B", "main"]);
    assert!(land.contains(&format!("Code: remotes/demo {code_before}..{code_after}\nOutcome: landed")), "{land}");
    // git itself reads it as a trailer
    let t = w.git(&w.log_origin, &["log", "-1", "--format=%(trailers:key=Code,valueonly)", "main"]);
    assert_eq!(t.trim(), format!("remotes/demo {code_before}..{code_after}"));

    // dig on the Land commit lists the code it carried
    let land_sha = w.git(&w.log_origin, &["rev-parse", "--short", "main"]);
    let d = ok(&w.abot(&["dig", &land_sha]));
    assert!(d.contains(&format!("Code remotes/demo {code_before}..{code_after}")), "{d}");
    let one = d.find("Add part one").expect("part one listed");
    let two = d.find("Add part two").expect("part two listed");
    assert!(one < two, "oldest first: {d}");

    // tree --json carries it
    let tj: serde_json::Value = serde_json::from_str(&ok(&w.abot(&["tree", "demo", "--json"]))).unwrap();
    let node = tj["nodes"].as_array().unwrap().iter().find(|n| n["subject"] == "Land demo/feat").unwrap();
    assert_eq!(node["code"][0]["repo"], "remotes/demo");
    assert_eq!(node["code"][0]["to"], code_after.as_str());

    // a log-only task records no code
    ok(&w.abot(&["start", "demo/words", "only words"]));
    let wt2 = w.wt("words");
    let st2 = std::fs::read_to_string(wt2.join("state/projects/demo.md")).unwrap();
    w.commit(&wt2, "state/projects/demo.md", &format!("{st2}\nmore words\n"), "More words");
    ok(&w.abot_in(&wt2, &["land"]));
    let land2 = w.git(&w.log_origin, &["log", "-1", "--format=%B", "main"]);
    assert!(!land2.contains("Code:"), "{land2}");

    // dead end: the lesson names the branch's own commits
    ok(&w.abot(&["start", "demo/hunch", "maybe"]));
    let hwt = w.wt("hunch");
    let hcwt = w.code_wt("hunch");
    w.commit(&hcwt, "h.txt", "h\n", "Try the hunch");
    let tip = w.git(&hcwt, &["rev-parse", "--short=7", "HEAD"]);
    ok(&w.abot_in(&hwt, &["dead-end", "--tried", "a", "--failed", "b", "--instead", "c"]));
    let lesson = w.git(&w.log_origin, &["log", "-1", "--format=%B", "main^^2"]);
    assert!(lesson.contains(&format!("Code: remotes/demo {code_after}..{tip}\nOutcome: dead-end")), "{lesson}");

    // park: the same, on the parked tip
    ok(&w.abot(&["start", "demo/later", "not yet"]));
    let pwt = w.wt("later");
    let pcwt = w.code_wt("later");
    w.commit(&pcwt, "p.txt", "p\n", "Half of the code");
    let ptip = w.git(&pcwt, &["rev-parse", "--short=7", "HEAD"]);
    ok(&w.abot_in(&pwt, &["park", "waits"]));
    let park = w.git(&w.log_origin, &["log", "-1", "--format=%B", "demo/later"]);
    assert!(park.contains(&format!("Code: remotes/demo {code_after}..{ptip}\nOutcome: parked")), "{park}");
}

/// The Session: trailer: start records the session working the task (from
/// Remote Control's env, or --session), land, dead-end and park carry it onto
/// the commit that ends the task, and `abot sessions` reads where each stands.
#[test]
fn the_log_records_the_session_that_worked_each_task() {
    let w = World::new();
    let with_env = |cwd: &Path, args: &[&str], id: &str| {
        let mut c = abot_cmd();
        c.args(args)
            .current_dir(cwd)
            .env("ABOT_LOG", &w.log)
            .env("ABOT_PROJECTS", w.root.join("Projects"))
            .env("ABOT_DIWA", "")
            .env("CLAUDE_CODE_BRIDGE_SESSION_ID", id);
        git_env(&mut c, &w.root);
        c.output().unwrap()
    };

    // from the env, cse_ spelled as the sidebar's session_
    ok(&with_env(&w.log, &["start", "demo/feat", "try the feature"], "cse_AAA"));
    let wt = w.wt("feat");
    let first = w.git(&wt, &["log", "-1", "--format=%B"]);
    assert!(first.contains("Session: session_AAA\nOutcome: in-flight"), "{first}");
    let t = w.git(&wt, &["log", "-1", "--format=%(trailers:key=Session,valueonly)"]);
    assert_eq!(t.trim(), "session_AAA", "git reads it as a trailer");
    let state = std::fs::read_to_string(wt.join("state/projects/demo.md")).unwrap();
    w.commit(&wt, "state/projects/demo.md", &state.replace("- dr1, v1", "- dr1, v2"), "v2 is live");
    // landed by someone else's shell: the Land commit still names the worker
    ok(&with_env(&wt, &["land"], "session_OTHER"));
    let land = w.git(&w.log_origin, &["log", "-1", "--format=%B", "main"]);
    assert!(land.contains("Session: session_AAA\n") && !land.contains("OTHER"), "{land}");

    // --session wins over the env; dead-end carries it
    ok(&with_env(&w.log, &["start", "demo/hunch", "maybe", "--session", "session_BBB"], "cse_IGNORED"));
    ok(&w.abot_in(&w.wt("hunch"), &["dead-end", "--tried", "a", "--failed", "b", "--instead", "c"]));
    let lesson = w.git(&w.log_origin, &["log", "-1", "--format=%B", "main^^2"]);
    assert!(lesson.contains("Session: session_BBB\n") && lesson.contains("Outcome: dead-end"), "{lesson}");

    // park carries it; no session at all records none
    ok(&with_env(&w.log, &["start", "demo/later", "not yet"], "session_CCC"));
    ok(&w.abot_in(&w.wt("later"), &["park", "waits"]));
    ok(&w.abot(&["start", "demo/now", "in flight, no session"]));
    let none = w.git(&w.wt("now"), &["log", "-1", "--format=%B"]);
    assert!(!none.contains("Session:"), "{none}");
    ok(&with_env(&w.log, &["start", "demo/busy", "still going"], "session_DDD"));

    let v: serde_json::Value = serde_json::from_str(&ok(&w.abot(&["sessions", "--json"]))).unwrap();
    let rows = v.as_array().unwrap();
    let find = |s: &str| rows.iter().find(|r| r["session"] == s).unwrap_or_else(|| panic!("{s} in {v}"));
    assert_eq!(find("session_AAA")["outcome"], "landed");
    assert_eq!(find("session_AAA")["branch"], "demo/feat");
    assert_eq!(find("session_BBB")["outcome"], "dead-end");
    assert_eq!(find("session_CCC")["outcome"], "parked");
    assert_eq!(find("session_CCC")["branch"], "demo/later");
    assert_eq!(find("session_DDD")["outcome"], "in-flight");
    assert_eq!(rows.len(), 4, "{v}");
    let text = ok(&w.abot(&["sessions"]));
    assert!(text.contains("session_AAA  landed     demo/feat"), "{text}");
    assert!(find("session_AAA")["parent"].is_null(), "no spawner passed one");
}

/// The Parent: trailer: a worker started with its spawner's id in its settings
/// (CLAUDE_PARENT_SESSION_ID) or --parent records who started it on the Start commit,
/// never itself, and `abot sessions --json` returns it even after the task landed.
#[test]
fn start_records_who_started_the_session() {
    let w = World::new();
    let run = |cwd: &Path, args: &[&str], envs: &[(&str, &str)]| {
        let mut c = abot_cmd();
        c.args(args).current_dir(cwd).env("ABOT_LOG", &w.log).env("ABOT_PROJECTS", w.root.join("Projects")).env("ABOT_DIWA", "");
        for (k, v) in envs {
            c.env(k, v);
        }
        git_env(&mut c, &w.root);
        c.output().unwrap()
    };
    ok(&run(&w.log, &["start", "demo/kid", "a worker"], &[("CLAUDE_CODE_BRIDGE_SESSION_ID", "session_KID"), ("CLAUDE_PARENT_SESSION_ID", "cse_DAD")]));
    let first = w.git(&w.wt("kid"), &["log", "-1", "--format=%B"]);
    assert!(first.contains("Session: session_KID\nParent: session_DAD\n"), "{first}");
    let state = std::fs::read_to_string(w.wt("kid").join("state/projects/demo.md")).unwrap();
    w.commit(&w.wt("kid"), "state/projects/demo.md", &state.replace("- dr1, v1", "- dr1, v3"), "v3 is live");
    ok(&run(&w.wt("kid"), &["land"], &[]));
    // --parent wins over the env; a parent that is the session itself is not one
    ok(&run(&w.log, &["start", "demo/flag", "by hand", "--parent", "8c1f0e2a-1111-4222-8333-944455556666"], &[("CLAUDE_CODE_BRIDGE_SESSION_ID", "session_FLAG"), ("CLAUDE_PARENT_SESSION_ID", "session_IGNORED")]));
    ok(&run(&w.log, &["start", "demo/self", "loops"], &[("CLAUDE_CODE_BRIDGE_SESSION_ID", "session_SELF"), ("CLAUDE_PARENT_SESSION_ID", "session_SELF")]));
    let v: serde_json::Value = serde_json::from_str(&ok(&w.abot(&["sessions", "--json"]))).unwrap();
    let find = |s: &str| v.as_array().unwrap().iter().find(|r| r["session"] == s).cloned().unwrap_or_else(|| panic!("{s} in {v}"));
    assert_eq!(find("session_KID")["parent"], "session_DAD", "still known after it landed");
    assert_eq!(find("session_KID")["outcome"], "landed");
    assert_eq!(find("session_FLAG")["parent"], "8c1f0e2a-1111-4222-8333-944455556666");
    assert!(find("session_SELF")["parent"].is_null());
}

// ---------------------------------------------------------------- tickets

/// A stand-in for `claude -p`: it records its arguments, and answers with
/// `first.json` on a new session and `resume.json` on a resumed one, in the
/// shape `claude --output-format json` prints.
fn fake_writer(w: &World, first: serde_json::Value, resume: serde_json::Value) {
    let wrap = |so: serde_json::Value| serde_json::json!({ "type": "result", "is_error": false, "session_id": "sess-1234abcd", "total_cost_usd": 0.004, "structured_output": so }).to_string();
    std::fs::write(w.root.join("first.json"), wrap(first)).unwrap();
    std::fs::write(w.root.join("resume.json"), wrap(resume)).unwrap();
    let script = "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$FAKE_DIR/args.log\"\necho ---- >> \"$FAKE_DIR/args.log\"\nfor a in \"$@\"; do if [ \"$a\" = --resume ]; then cat \"$FAKE_DIR/resume.json\"; exit 0; fi; done\ncat \"$FAKE_DIR/first.json\"\n";
    let p = w.root.join("fake-claude");
    std::fs::write(&p, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn written(id: &str, steps: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "status": "file",
        "note": "made the done-when checkable",
        "ticket": {
            "id": id,
            "title": "The demo answers from its own model",
            "why": "The demo's desk answers questions without sending them to an outside service.",
            "done_when": "Asking the demo desk a question gets an answer within ten seconds, and the request log shows only our own machine.",
            "steps": steps.iter().map(|s| match s.strip_prefix("(done) ") { Some(t) => serde_json::json!({ "text": t, "done": true }), None => serde_json::json!({ "text": s, "done": false }) }).collect::<Vec<_>>(),
            "waits_on_who": "felix",
            "waits_on_what": "say which machine the model runs on",
            "detail": "state/projects/demo.md"
        }
    })
}

fn status(o: &Output) -> i32 {
    o.status.code().unwrap_or(-1)
}

#[test]
fn a_ticket_is_asked_about_then_filed_with_the_conversation_in_its_commit() {
    let w = World::new();
    let ask = serde_json::json!({ "status": "ask", "note": "two things only you know", "questions": ["What would someone see when it is done?", "Which machine?"] });
    fake_writer(&w, ask, written("own-model", &["(done) The model runs on our own machine and answers a test question", "The demo desk sends its questions to that model"]));

    let o = w.abot(&["ticket", "add", "demo", "own model", "--why", "privacy", "--step", "C: model"]);
    assert_eq!(status(&o), 2, "not filed yet is exit 2: {}", String::from_utf8_lossy(&o.stderr));
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("not filed yet") && out.contains("1. What would someone see") && out.contains("abot ticket answer sess-123"), "{out}");
    let before = w.git(&w.log_origin, &["rev-parse", "main"]);

    // the writer was run cheaply and on purpose: a named model, no tools
    let args = std::fs::read_to_string(w.root.join("args.log")).unwrap();
    for want in ["-p\n", "--model\nsonnet\n", "--effort\nlow\n", "--tools\n\n", "--json-schema\n"] {
        assert!(args.contains(want), "claude ran without {want:?}:\n{args}");
    }

    let out = ok(&w.abot(&["ticket", "answer", "sess-1234", "1. an answer in ten seconds 2. dr1"]));
    assert!(out.contains("filed demo/own-model on main"), "{out}");
    assert!(std::fs::read_to_string(w.root.join("args.log")).unwrap().contains("--resume\nsess-1234abcd\n"), "answer resumes the same session");
    assert_ne!(w.git(&w.log_origin, &["rev-parse", "main"]), before);

    // one commit on origin's main, the file written, the Q&A in the body
    let file = w.git(&w.log_origin, &["show", "main:state/tickets/demo.md"]);
    assert!(file.contains("### own-model: The demo answers from its own model"), "{file}");
    assert!(file.contains("Waits on: felix: say which machine the model runs on"), "{file}");
    assert!(file.contains("1. [done] The model runs on our own machine and answers a test question (") && file.contains("done before it was filed)"), "the writer can say a step is already done: {file}");
    let msg = w.git(&w.log_origin, &["log", "-1", "--format=%B", "main"]);
    assert!(msg.starts_with("Ticket demo/own-model: The demo's desk answers questions"), "{msg}");
    for want in ["The agent's draft:", "step 1: C: model", "Asked (round 1):", "What would someone see", "Answered: 1. an answer in ten seconds 2. dr1", "Ticket: demo/own-model"] {
        assert!(msg.contains(want), "missing {want:?} in:\n{msg}");
    }
    // the main checkout was brought along, and nothing in it was touched by hand
    assert_eq!(w.git(&w.log, &["rev-parse", "main"]), w.git(&w.log_origin, &["rev-parse", "main"]));
    assert!(w.git(&w.log, &["status", "--porcelain"]).is_empty());

    // the conversation is gone once filed
    assert!(fails(&w.abot(&["ticket", "answer", "sess-1234", "more"])).contains("no ticket waiting"));

    let v: serde_json::Value = serde_json::from_str(&ok(&w.abot(&["ticket", "list", "--json"]))).unwrap();
    let t = &v["tickets"][0];
    assert_eq!(t["ref"], "demo/own-model");
    assert_eq!(t["written_out"], true);
    assert_eq!(t["waits_on"]["who"], "felix");
    assert_eq!(t["steps"][1]["ready"], "demo/own-model.2");
    assert_eq!(t["steps"][0]["done"], true);
    assert!(ok(&w.abot(&["check"])).starts_with("ok:"), "a filed ticket passes check");
    assert!(ok(&w.abot(&["orient", "demo"])).contains("demo/own-model  The demo answers from its own model  [1/2 steps done] · waits on felix"));
}

#[test]
fn a_thin_ticket_never_lands_even_if_the_writer_files_it() {
    let w = World::new();
    let thin = serde_json::json!({ "status": "file", "note": "", "ticket": { "id": "views", "title": "C: views", "why": "views", "done_when": "it works", "steps": ["Docs pass 2"] } });
    fake_writer(&w, thin.clone(), thin);
    let before = w.git(&w.log_origin, &["rev-parse", "main"]);
    let o = w.abot(&["ticket", "add", "demo", "C: views"]);
    assert_eq!(status(&o), 2);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("abot's check: title") && out.contains("Done when"), "the refusal comes back as questions: {out}");
    assert_eq!(w.git(&w.log_origin, &["rev-parse", "main"]), before, "nothing was filed");
    let args = std::fs::read_to_string(w.root.join("args.log")).unwrap();
    assert!(args.contains("abot's check refused that ticket"), "the writer got one more turn with the refusal:\n{args}");

    // and a thin ticket written by hand is caught by check
    let thin_file = "# demo: tickets\n\n## Open\n\n### views: C: views\nWhy: views\nDone when: done\n1. [todo] Docs pass 2\n";
    w.commit(&w.log, "state/tickets/demo.md", thin_file, "hand-written");
    w.git(&w.log, &["push", "-q", "origin", "main"]);
    let o = w.abot(&["check"]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(!o.status.success() && out.contains("state/tickets/demo.md: views: title"), "{out}");
    let v: serde_json::Value = serde_json::from_str(&ok(&w.abot(&["ticket", "list", "--json"]))).unwrap();
    assert_eq!(v["tickets"][0]["written_out"], false, "overview can label it");
}

#[test]
fn land_marks_the_steps_a_task_finished() {
    let w = World::new();
    fake_writer(&w, written("own-model", &["The model runs on our own machine and answers a test question", "The demo desk sends its questions to that model"]), serde_json::json!({}));
    ok(&w.abot(&["ticket", "add", "demo", "own model"]));

    // a task names step 1 in a code commit
    ok(&w.abot(&["start", "demo/model-box", "run the model"]));
    let (wt, cwt) = (w.wt("model-box"), w.code_wt("model-box"));
    w.commit(&cwt, "model.txt", "x\n", "Run the model on our own machine\n\nReady: demo/own-model.1\n\nCo-Authored-By: someone");
    let out = ok(&w.abot_in(&wt, &["land"]));
    assert!(out.contains("ticket: demo/own-model.1"), "{out}");
    let file = w.git(&w.log_origin, &["show", "main:state/tickets/demo.md"]);
    let sha = w.git(&w.code_origin, &["log", "-1", "--format=%h", "main"]);
    assert!(file.contains(&format!("1. [done] The model runs on our own machine and answers a test question ({}, demo {sha})", chrono_today())) || file.contains(&format!("{sha})")), "{file}");
    assert!(file.contains("2. [todo]"), "{file}");
    let subjects = w.git(&w.log_origin, &["log", "--format=%s", "-3", "main"]);
    assert!(subjects.starts_with("Land demo/model-box\nTicket steps done by demo/model-box"), "{subjects}");

    // a branch named for the ticket, with one step left, finishes it
    ok(&w.abot(&["start", "demo/own-model", "wire the desk"]));
    w.commit(&w.code_wt("own-model"), "desk.txt", "x\n", "The desk asks our model");
    let out = ok(&w.abot_in(&w.wt("own-model"), &["land"]));
    assert!(out.contains("every step done, moved to Done"), "{out}");
    let file = w.git(&w.log_origin, &["show", "main:state/tickets/demo.md"]);
    assert!(file.contains("## Done\n- own-model: The demo answers from its own model"), "{file}");
    assert!(file.contains("(nothing open)"), "{file}");
}

#[test]
fn steps_are_added_and_marked_and_tickets_retired_through_abot() {
    let w = World::new();
    fake_writer(&w, written("own-model", &["The model runs on our own machine and answers a test question"]), serde_json::json!({}));
    ok(&w.abot(&["ticket", "add", "demo", "own model"]));

    assert!(fails(&w.abot(&["ticket", "step", "demo/own-model", "Docs pass 2"])).contains("plain sentence"));
    let out = ok(&w.abot(&["ticket", "step", "demo/own-model", "The desk shows which machine answered"]));
    assert!(out.contains("Ready: demo/own-model.2"), "{out}");
    ok(&w.abot(&["ticket", "step", "demo/own-model.1", "--done", "--by", "demo 1234567"]));
    let file = w.git(&w.log_origin, &["show", "main:state/tickets/demo.md"]);
    assert!(file.contains("answers a test question (") && file.contains("demo 1234567)") && file.contains("2. [todo] The desk shows"), "{file}");
    assert!(fails(&w.abot(&["ticket", "step", "demo/own-model.9", "--done"])).contains("has 2 step(s), not 9"));
    assert!(fails(&w.abot(&["ticket", "step", "demo/own-model.2", "Done: the desk shows it"])).contains("reads like a label"));
    ok(&w.abot(&["ticket", "step", "demo/own-model.2", "The desk shows which machine answered each question"]));
    let file = w.git(&w.log_origin, &["show", "main:state/tickets/demo.md"]);
    assert!(file.contains("2. [todo] The desk shows which machine answered each question"), "{file}");
    assert!(w.git(&w.log_origin, &["log", "-1", "--format=%B", "main"]).contains("Was: The desk shows which machine answered"));

    // retiring an open ticket, and recording an old one so its id stays taken
    ok(&w.abot(&["ticket", "retire", "demo/own-model", "the desk is going away, Felix 10-15"]));
    assert!(fails(&w.abot(&["ticket", "retire", "demo/old-epic", "gone"])).contains("--title"));
    ok(&w.abot(&["ticket", "retire", "demo/old-epic", "four of six done; the rest has no detail, Felix 10-15", "--title", "Machines we can explain"]));
    let file = w.git(&w.log_origin, &["show", "main:state/tickets/demo.md"]);
    assert!(file.contains("## Retired\n- own-model: The demo answers from its own model — the desk is going away, Felix 10-15 ("), "{file}");
    assert!(file.contains("- old-epic: Machines we can explain — four of six done; the rest has no detail, Felix 10-15 ("), "{file}");
    let v: serde_json::Value = serde_json::from_str(&ok(&w.abot(&["ticket", "list", "--json"]))).unwrap();
    assert_eq!(v["tickets"].as_array().unwrap().len(), 0);
    assert_eq!(v["retired"].as_array().unwrap().len(), 2);
    assert_eq!(v["retired"][1]["note"], "four of six done; the rest has no detail, Felix 10-15");
    assert!(fails(&w.abot(&["ticket", "add", "demo", "again", "--id", "old-epic"])).contains("taken"));
    // the agent's own --id wins over the one the writer picks
    ok(&w.abot(&["ticket", "add", "demo", "own model again", "--id", "mine"]));
    assert!(w.git(&w.log_origin, &["show", "main:state/tickets/demo.md"]).contains("### mine: The demo answers"));
    assert!(ok(&w.abot(&["check"])).starts_with("ok:"));
}

#[test]
fn ticket_commits_retry_on_top_of_someone_elses_push() {
    let w = World::new();
    fake_writer(&w, written("own-model", &["The model runs on our own machine and answers a test question"]), serde_json::json!({}));
    // someone else lands on origin from another clone; ours is now behind
    let other = w.root.join("other");
    git(&w.root, &w.root, &["clone", "-q", w.log_origin.to_str().unwrap(), other.to_str().unwrap()]);
    w.commit(&other, "state/org.md", "d-2. someone else's change\n", "someone else");
    w.git(&other, &["push", "-q", "origin", "main"]);
    ok(&w.abot(&["ticket", "add", "demo", "own model"]));
    let log = w.git(&w.log_origin, &["log", "--format=%s", "-2", "main"]);
    assert!(log.starts_with("Ticket demo/own-model") && log.contains("someone else"), "{log}");
}

/// A fake `claude` that only records its arguments, one per line.
fn fake_stopper(w: &World) {
    let p = w.root.join("fake-claude");
    std::fs::write(&p, "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$FAKE_DIR/args.log\"\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn stopped(w: &World) -> String {
    for _ in 0..50 {
        if let Ok(s) = std::fs::read_to_string(w.root.join("args.log")) {
            return s;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    String::new()
}

fn land_one(w: &World, task: &str, extra: &[(&str, &str)], flags: &[&str]) -> Output {
    ok(&w.abot(&["start", &format!("demo/{task}"), "try it"]));
    let wt = w.wt(task);
    let state = std::fs::read_to_string(wt.join("state/projects/demo.md")).unwrap();
    w.commit(&wt, "state/projects/demo.md", &format!("{state}\n- {task}\n"), "the truth changed");
    let mut args = vec!["land"];
    args.extend(flags);
    w.abot_env(&wt, &args, extra)
}

#[test]
fn land_closes_the_background_session_it_ran_in() {
    let w = World::new();
    fake_stopper(&w);
    let o = land_one(&w, "closes", &[("CLAUDE_JOB_DIR", "/tmp/jobs/abcd1234"), ("CLAUDECODE", "1"), ("ABOT_CLOSE_DELAY", "0")], &[]);
    let out = ok(&o);
    assert_eq!(out.lines().last().unwrap(), format!("cd {}", w.log.display()), "the last line stays runnable");
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("closing this session in 0s: claude stop abcd1234"), "{err}");
    assert_eq!(stopped(&w), "stop\nabcd1234\n");
}

#[test]
fn keep_open_and_terminals_close_nothing() {
    let w = World::new();
    fake_stopper(&w);
    let o = land_one(&w, "stays", &[("CLAUDE_JOB_DIR", "/tmp/jobs/abcd1234"), ("ABOT_CLOSE_DELAY", "0")], &["--keep-open"]);
    ok(&o);
    assert!(String::from_utf8_lossy(&o.stderr).contains("stays open (--keep-open)"));
    let o = land_one(&w, "terminal", &[("ABOT_CLOSE_DELAY", "0")], &[]);
    ok(&o);
    assert!(!String::from_utf8_lossy(&o.stderr).contains("clos"), "a person at a terminal closes nothing");
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(!w.root.join("args.log").exists(), "nothing was stopped");
}

#[test]
fn an_app_session_is_told_to_archive_itself() {
    let w = World::new();
    fake_stopper(&w);
    let o = land_one(&w, "app", &[("CLAUDECODE", "1"), ("CLAUDE_CODE_BRIDGE_SESSION_ID", "cse_01ABC"), ("ABOT_CLOSE_DELAY", "0")], &[]);
    ok(&o);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("ARCHIVE-SELF: session_01ABC"), "{err}");
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(!w.root.join("args.log").exists());
}

/// A session started from the desktop app or by overview has no
/// $CLAUDE_CODE_BRIDGE_SESSION_ID and no $CLAUDE_JOB_DIR, only
/// $CLAUDE_CODE_SESSION_ID: abot finds its job by that id, records its
/// Session: and closes it.
#[test]
fn a_job_without_bridge_env_is_found_by_its_session_id() {
    let w = World::new();
    fake_stopper(&w);
    let job = w.root.join(".claude/jobs/ab12cd34");
    std::fs::create_dir_all(&job).unwrap();
    std::fs::write(
        job.join("state.json"),
        r#"{"sessionId":"11111111-2222-4333-8444-555555555555","daemonShort":"ab12cd34","bridgeSessionId":"cse_01JOB"}"#,
    )
    .unwrap();
    let other = w.root.join(".claude/jobs/ffff0000");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("state.json"), r#"{"sessionId":"someone-else","daemonShort":"ffff0000","bridgeSessionId":"cse_01NOPE"}"#).unwrap();
    let env = [("CLAUDECODE", "1"), ("CLAUDE_CODE_SESSION_ID", "11111111-2222-4333-8444-555555555555"), ("ABOT_CLOSE_DELAY", "0")];
    ok(&w.abot_env(&w.log, &["start", "demo/nobridge", "try it"], &env));
    let wt = w.wt("nobridge");
    let start = w.git(&wt, &["log", "-1", "--format=%B"]);
    assert!(start.contains("Session: session_01JOB"), "{start}");
    let state = std::fs::read_to_string(wt.join("state/projects/demo.md")).unwrap();
    w.commit(&wt, "state/projects/demo.md", &format!("{state}\n- nobridge\n"), "the truth changed");
    let o = w.abot_env(&wt, &["land"], &env);
    ok(&o);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("claude stop ab12cd34"), "{err}");
    assert_eq!(stopped(&w), "stop\nab12cd34\n");
    let main = w.git(&w.log_origin, &["log", "-1", "--format=%B", "main"]);
    assert!(main.contains("Session: session_01JOB"), "{main}");
}

/// No job and no bridge id: still something the session can act on.
#[test]
fn an_app_session_with_no_id_is_told_how_to_find_it() {
    let w = World::new();
    fake_stopper(&w);
    let o = land_one(&w, "noid", &[("CLAUDECODE", "1"), ("CLAUDE_CODE_SESSION_ID", "99999999-0000-4000-8000-000000000000"), ("ABOT_CLOSE_DELAY", "0")], &[]);
    ok(&o);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("ARCHIVE-SELF:") && err.contains("get_session with no id"), "{err}");
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(!w.root.join("args.log").exists());
}
