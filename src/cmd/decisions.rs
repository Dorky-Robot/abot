//! decide and mirror: Felix's decisions live in felix-decisions.json, which
//! overview renders; the log keeps a mirror of them, one commit each, so
//! diwa can hang insights off them. Nobody writes decisions/ by hand.

use crate::ctx::Ctx;
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Map, Value};
use std::path::Path;

// ---------------------------------------------------------------- decide

pub fn decide(ctx: &Ctx, text: &str, why: &str, about: &[String], by: &str, json_out: bool) -> Result<()> {
    if text.trim().is_empty() || why.trim().is_empty() {
        bail!("a decision needs its text and --why");
    }
    let about: Vec<String> = about.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    if about.is_empty() {
        bail!("--about needs at least one project, e.g. --about org,overview");
    }
    let path = &ctx.decisions;
    if !path.exists() {
        bail!(
            "no {} on this machine: decisions are recorded where that file lives (dr2, the CEO's machine); run abot decide there",
            path.display()
        );
    }
    let raw = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut doc: Value = serde_json::from_str(&raw).with_context(|| format!("{} is not valid JSON", path.display()))?;
    let list = doc
        .get_mut("decisions")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("{} has no `decisions` array", path.display()))?;
    let next = list
        .iter()
        .filter_map(|d| d.get("id")?.as_str()?.strip_prefix("d-")?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        + 1;
    let mut entry = Map::new();
    entry.insert("id".into(), Value::from(format!("d-{next}")));
    entry.insert("at".into(), Value::from(super::today()));
    entry.insert("by".into(), Value::from(by));
    entry.insert("text".into(), Value::from(text.trim()));
    entry.insert("why".into(), Value::from(why.trim()));
    entry.insert("about".into(), Value::from(about));
    let entry = Value::Object(entry);
    list.push(entry.clone());

    let mut out = to_indent1(&doc)?;
    if raw.ends_with('\n') {
        out.push('\n');
    }
    write_atomic(path, &out)?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&entry)?);
    } else {
        println!("recorded d-{next} in {}", path.display());
        println!("the mirror lands it in the log as decisions/d-{next}.md (abot mirror runs it now)");
    }
    Ok(())
}

/// JSON the way Python's `json.dump(indent=1, ensure_ascii=False)` writes
/// it, so overview and every other reader see the same bytes they always did.
pub fn to_indent1(v: &Value) -> Result<String> {
    let mut buf = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
    serde::Serialize::serialize(v, &mut ser)?;
    Ok(String::from_utf8(buf)?)
}

fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(
        ".{}.abot-{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("decisions.json"),
        std::process::id()
    ));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

// ---------------------------------------------------------------- mirror

fn s<'a>(d: &'a Value, k: &str) -> &'a str {
    d.get(k).and_then(Value::as_str).unwrap_or("")
}

fn list(d: &Value, k: &str) -> Vec<String> {
    match d.get(k) {
        Some(Value::Array(a)) => a.iter().map(|x| x.as_str().map(String::from).unwrap_or_else(|| x.to_string())).collect(),
        Some(Value::String(x)) if !x.is_empty() => vec![x.clone()],
        _ => vec![],
    }
}

/// decisions/<id>.md, byte for byte what bin/mirror-decisions writes, so the
/// two can run against the same log and agree that nothing changed. One
/// difference: a `supersedes` that is a string ("d-21") is written as is,
/// where the Python joined its characters ("d, -, 2, 1").
pub fn render(d: &Value) -> String {
    render_with(d, false)
}

/// What bin/mirror-decisions wrote for the same decision, quirk included.
/// A file that already holds this is current: rewriting it would land a
/// "revised" commit for a decision nobody revised.
pub fn render_legacy(d: &Value) -> String {
    render_with(d, true)
}

fn render_with(d: &Value, legacy: bool) -> String {
    let about = list(d, "about").join(", ");
    let mut lines = vec![format!("# {} · {} · {}", s(d, "id"), s(d, "at"), s(d, "by")), String::new()];
    if !about.is_empty() {
        lines.push(format!("about: {about}"));
    }
    let sup = match d.get("supersedes") {
        Some(Value::String(x)) if legacy => x.chars().map(String::from).collect(),
        _ => list(d, "supersedes"),
    };
    if !sup.is_empty() {
        lines.push(format!("supersedes: {}", sup.join(", ")));
    }
    lines.extend([
        String::new(),
        s(d, "text").trim().to_string(),
        String::new(),
        format!("Why: {}", s(d, "why").trim()),
        String::new(),
    ]);
    lines.join("\n")
}

pub fn first_sentence(text: &str, limit: usize) -> String {
    let s: Vec<char> = text.split_whitespace().collect::<Vec<_>>().join(" ").chars().collect();
    let mut end = s.len();
    for (i, ch) in s.iter().enumerate() {
        if (*ch == '.' || *ch == ';') && (i + 1 == s.len() || s[i + 1] == ' ') {
            end = i;
            break;
        }
    }
    let s = &s[..end];
    if s.len() <= limit {
        s.iter().collect()
    } else {
        let cut: String = s[..limit - 1].iter().collect();
        format!("{}…", cut.trim_end())
    }
}

pub fn message(d: &Value, revised: bool) -> String {
    let head = format!(
        "{}{}: {}",
        s(d, "id"),
        if revised { " revised" } else { "" },
        first_sentence(s(d, "text"), 150)
    );
    let mut trailers = vec![format!("Decision: {}", s(d, "id")), format!("Decided-by: {}", s(d, "by"))];
    let about = list(d, "about");
    if !about.is_empty() {
        trailers.push(format!("About: {}", about.join(", ")));
    }
    format!(
        "{head}\n\nWhy: {}\n\n{}\n\n{}",
        s(d, "why").trim(),
        s(d, "text").trim(),
        trailers.join("\n")
    )
}

pub fn mirror(ctx: &Ctx) -> Result<()> {
    let g = ctx.require_log()?;
    if g.current_branch().as_deref() != Some("main") {
        bail!("{} is not on main; not mirroring", ctx.log.display());
    }
    let common = g.common_dir().unwrap_or_else(|| ctx.log.join(".git"));
    if common.join("index.lock").exists() {
        println!("git is busy; next run");
        return Ok(());
    }
    let raw = std::fs::read_to_string(&ctx.decisions)
        .with_context(|| format!("reading {}", ctx.decisions.display()))?;
    let doc: Value = serde_json::from_str(&raw)?;
    let decisions = doc
        .get("decisions")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("{} has no `decisions` array", ctx.decisions.display()))?;
    let mut added = 0;
    for d in decisions {
        let id = s(d, "id");
        if id.is_empty() {
            continue;
        }
        let path = format!("decisions/{id}.md");
        let body = render(d);
        let have = g.try_raw(&["show", &format!("HEAD:{path}")]);
        if have.as_deref() == Some(body.as_str()) || have.as_deref() == Some(render_legacy(d).as_str()) {
            continue;
        }
        std::fs::create_dir_all(ctx.log.join("decisions"))?;
        std::fs::write(ctx.log.join(&path), &body)?;
        g.run(&["add", "--", &path])?;
        let msg = message(d, have.is_some());
        let date = format!("{}T12:00:00", s(d, "at"));
        let env: Vec<(&str, &str)> = if have.is_none() { vec![("GIT_AUTHOR_DATE", date.as_str())] } else { vec![] };
        g.run_env(&["commit", "-q", "-m", &msg, "--", &path], &env)?;
        added += 1;
    }
    println!("mirrored {added} decision(s) of {}", decisions.len());
    // main is shared, and indexed by diwa, only once it is on GitHub. A failed
    // push is retried by the next run, which pushes whenever main is ahead.
    let ahead = g.try_run(&["rev-list", "--count", "origin/main..main"]).unwrap_or_default();
    if !ahead.is_empty() && ahead != "0" {
        match g.run(&["push", "-q", "origin", "main"]) {
            Ok(_) => println!("pushed"),
            Err(e) => println!("push failed, next run retries: {e:#}"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn render_matches_the_python_mirror() {
        let d = json!({"id":"d-98","at":"2026-10-08","by":"Felix","text":" Do it. ","why":"Because.\n","about":["org","overview"],"supersedes":["d-68"]});
        assert_eq!(
            render(&d),
            "# d-98 · 2026-10-08 · Felix\n\nabout: org, overview\nsupersedes: d-68\n\nDo it.\n\nWhy: Because.\n"
        );
        let bare = json!({"id":"d-1","at":"2026-01-01","by":"Felix","text":"x","why":"y"});
        assert_eq!(render(&bare), "# d-1 · 2026-01-01 · Felix\n\n\nx\n\nWhy: y\n");
    }

    #[test]
    fn string_supersedes_renders_right_and_legacy_is_kept() {
        let d = json!({"id":"d-114","at":"2026-10-13","by":"Felix","text":"t","why":"w","about":["org"],"supersedes":"d-21"});
        assert!(render(&d).contains("supersedes: d-21\n"));
        assert!(render_legacy(&d).contains("supersedes: d, -, 2, 1\n"));
    }

    #[test]
    fn first_sentence_like_python() {
        assert_eq!(first_sentence("Felix, 10-14: \"go\". More.", 150), "Felix, 10-14: \"go\"");
        assert_eq!(first_sentence("v1.2 is out; then", 150), "v1.2 is out");
        assert_eq!(first_sentence("a   b\n c", 150), "a b c");
        let long = "x".repeat(200);
        let f = first_sentence(&long, 150);
        assert_eq!(f.chars().count(), 150);
        assert!(f.ends_with('…'));
    }

    #[test]
    fn message_has_trailers() {
        let d = json!({"id":"d-2","at":"2026-01-01","by":"Felix","text":"Ship it. Now.","why":"w","about":["org"]});
        let m = message(&d, true);
        assert!(m.starts_with("d-2 revised: Ship it\n\nWhy: w\n\nShip it. Now.\n\n"));
        assert!(m.ends_with("Decision: d-2\nDecided-by: Felix\nAbout: org"));
    }

    #[test]
    fn indent1_matches_python() {
        let v: Value = serde_json::from_str(r#"{"v":1,"d":[{"a":"é\n\"q\"","b":[]}],"e":{}}"#).unwrap();
        // python3 -c 'import json;print(json.dumps(v,indent=1,ensure_ascii=False))'
        assert_eq!(
            to_indent1(&v).unwrap(),
            "{\n \"v\": 1,\n \"d\": [\n  {\n   \"a\": \"é\\n\\\"q\\\"\",\n   \"b\": []\n  }\n ],\n \"e\": {}\n}"
        );
    }
}
