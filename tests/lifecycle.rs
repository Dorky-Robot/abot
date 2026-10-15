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
        let mut c = Command::new(env!("CARGO_BIN_EXE_abot"));
        c.args(args)
            .current_dir(cwd)
            .env("ABOT_LOG", &self.log)
            .env("ABOT_PROJECTS", self.root.join("Projects"))
            .env("ABOT_DECISIONS", self.root.join("decisions.json"))
            .env("ABOT_DIWA", "")
            .env("NO_COLOR", "1");
        git_env(&mut c, &self.root);
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
    assert!(fails(&w.abot(&["start", "demo/x", " "])).contains("say why"));
    // Someone else has the branch on origin.
    w.git(&w.log, &["branch", "demo/taken"]);
    w.git(&w.log, &["push", "-q", "origin", "demo/taken"]);
    w.git(&w.log, &["branch", "-D", "demo/taken"]);
    assert!(fails(&w.abot(&["start", "demo/taken", "why"])).contains("already exists on origin"));
    assert!(!w.wt("taken").exists());
    // --new scaffolds a state file and refuses an existing one.
    assert!(fails(&w.abot(&["start", "demo/y", "why", "--new"])).contains("drop --new"));
    let out = ok(&w.abot(&["start", "fresh/first", "a new project", "--new"]));
    assert!(out.contains("none found"), "{out}");
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
