//! An entry: one thing Felix said (verbatim) or one thing seen on the boxes
//! (with its proof). Written once, never edited; `abot check` refuses a
//! landed entry that changed.
//!
//! graph/entries/<project>/<yyyy-mm>/<id>.md
//!
//!     said: 2026-11-04
//!     by: Felix
//!     about: session.waiting
//!     in-force: 2026-11-04
//!     rule: A session that waits on something is not archived.
//!     where: the session that heard it
//!
//!     we should not be archiving things that are waiting on something
//!
//! The header is `key: value` lines; the first says the kind and the date.
//! The body, after the first blank line, is his words (said) or what was
//! seen (seen), exactly as given. Optional keys: replaces, contradicts (an
//! entry id), corrects (a node id), settles (a clash id), changes (rarely,
//! sometimes, often), why, proof (seen), where.

use anyhow::{Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Said,
    Seen,
}

#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub id: String,
    pub project: String,
    pub path: String,
    pub kind: Kind,
    pub by: String,
    pub date: String,
    pub in_force: String,
    pub about: String,
    /// His words (said), or what was seen (seen), verbatim.
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaces: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contradicts: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corrects: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settles: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(rename = "where", skip_serializing_if = "Option::is_none")]
    pub where_: Option<String>,
}

pub const CHANGES: [&str; 3] = ["rarely", "sometimes", "often"];

pub fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// A node id: dotted lowercase words (`session.waiting`, `everyday-vet.ship`),
/// or `clash/<words>` for a clash.
pub fn valid_node(s: &str) -> bool {
    let s = s.strip_prefix("clash/").unwrap_or(s);
    !s.is_empty()
        && s.len() <= 80
        && s.split('.').all(|w| {
            !w.is_empty()
                && w.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
                && !w.starts_with('-')
        })
}

impl Entry {
    /// The words for a said entry, or what was seen, as a reader quotes it.
    pub fn quote(&self) -> String {
        match self.kind {
            Kind::Said => format!("{}, {}: \"{}\"", self.by, short_date(&self.date), one_para(&self.body)),
            Kind::Seen => {
                let p = self.proof.as_deref().map(|p| format!(" (proof: {p})")).unwrap_or_default();
                format!("Seen {}: {}{}", short_date(&self.date), one_para(&self.body), p)
            }
        }
    }

    /// The sentence this entry gives its subject: the rule for a said entry,
    /// what was seen for a seen one.
    pub fn sentence(&self) -> String {
        match self.kind {
            Kind::Said => self.rule.clone().unwrap_or_else(|| one_para(&self.body)),
            Kind::Seen => one_para(&self.body),
        }
    }
}

pub fn short_date(d: &str) -> String {
    d.get(5..).unwrap_or(d).to_string()
}

pub fn one_para(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The project an entry path belongs to: graph/entries/<project>/…
pub fn project_of_path(path: &str) -> Option<String> {
    let rest = path.strip_prefix("graph/entries/")?;
    Some(rest.split('/').next()?.to_string())
}

pub fn parse(path: &str, text: &str) -> Result<Entry> {
    let id = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".md").to_string();
    let project = project_of_path(path).unwrap_or_default();
    let (head, body) = match text.split_once("\n\n") {
        Some((h, b)) => (h, b),
        None => bail!("{path}: an entry needs a header, a blank line, then the words or what was seen"),
    };
    let mut e = Entry {
        id,
        project,
        path: path.to_string(),
        kind: Kind::Said,
        by: String::new(),
        date: String::new(),
        in_force: String::new(),
        about: String::new(),
        body: body.trim_end_matches('\n').to_string(),
        rule: None,
        proof: None,
        replaces: None,
        contradicts: None,
        corrects: None,
        settles: None,
        changes: None,
        why: None,
        where_: None,
    };
    for (i, line) in head.lines().enumerate() {
        let Some((k, v)) = line.split_once(':') else {
            bail!("{path}: header line {} is not `key: value`: {line}", i + 1);
        };
        let v = v.trim().to_string();
        let k = k.trim();
        if i == 0 {
            e.kind = match k {
                "said" => Kind::Said,
                "seen" => Kind::Seen,
                _ => bail!("{path}: the first line must be `said: <date>` or `seen: <date>`"),
            };
            e.date = v;
            continue;
        }
        let slot = match k {
            "by" => {
                e.by = v;
                continue;
            }
            "about" => {
                e.about = v;
                continue;
            }
            "in-force" => {
                e.in_force = v;
                continue;
            }
            "rule" => &mut e.rule,
            "proof" => &mut e.proof,
            "replaces" => &mut e.replaces,
            "contradicts" => &mut e.contradicts,
            "corrects" => &mut e.corrects,
            "settles" => &mut e.settles,
            "changes" => &mut e.changes,
            "why" => &mut e.why,
            "where" => &mut e.where_,
            _ => bail!("{path}: unknown key `{k}`"),
        };
        if !v.is_empty() {
            *slot = Some(v);
        }
    }
    if e.in_force.is_empty() {
        e.in_force = e.date.clone();
    }
    lint(&e).map_err(|w| anyhow::anyhow!("{path}: {w}"))?;
    Ok(e)
}

/// What makes an entry well formed, on its own.
pub fn lint(e: &Entry) -> std::result::Result<(), String> {
    if !valid_date(&e.date) {
        return Err(format!("`{}` is not a date (YYYY-MM-DD)", e.date));
    }
    if !valid_date(&e.in_force) {
        return Err(format!("in-force `{}` is not a date", e.in_force));
    }
    if e.by.trim().is_empty() {
        return Err("`by:` is missing".into());
    }
    if !valid_node(&e.about) || e.about.starts_with("clash/") {
        return Err(format!("about `{}` is not a subject (dotted lowercase words)", e.about));
    }
    if e.body.trim().is_empty() {
        return Err("the words (or what was seen) are missing".into());
    }
    match e.kind {
        Kind::Said if e.rule.is_none() => return Err("a said entry needs `rule:`, the rule his words give".into()),
        Kind::Seen if e.proof.is_none() => return Err("a seen entry needs `proof:` (a command, a commit, an issue)".into()),
        _ => {}
    }
    if let Some(c) = &e.changes {
        if !CHANGES.contains(&c.as_str()) {
            return Err(format!("changes `{c}` is not one of rarely, sometimes, often"));
        }
    }
    if let Some(s) = &e.settles {
        if !s.starts_with("clash/") || !valid_node(s) {
            return Err(format!("settles `{s}` is not a clash id (clash/<words>)"));
        }
    }
    if let Some(c) = &e.corrects {
        if !valid_node(c) {
            return Err(format!("corrects `{c}` is not a node id"));
        }
    }
    Ok(())
}

pub fn render(e: &Entry) -> String {
    let kind = match e.kind {
        Kind::Said => "said",
        Kind::Seen => "seen",
    };
    let mut s = format!("{kind}: {}\nby: {}\nabout: {}\n", e.date, e.by, e.about);
    if e.in_force != e.date {
        s.push_str(&format!("in-force: {}\n", e.in_force));
    }
    for (k, v) in [
        ("rule", &e.rule),
        ("proof", &e.proof),
        ("replaces", &e.replaces),
        ("contradicts", &e.contradicts),
        ("corrects", &e.corrects),
        ("settles", &e.settles),
        ("changes", &e.changes),
        ("why", &e.why),
        ("where", &e.where_),
    ] {
        if let Some(v) = v {
            s.push_str(&format!("{k}: {}\n", one_para(v)));
        }
    }
    s.push('\n');
    s.push_str(e.body.trim_end_matches('\n'));
    s.push('\n');
    s
}

/// `<yyyymmdd>-<6 hex>`: the date it was said or seen, and the start of a
/// hash of what the entry holds, so two boxes never mint the same id for
/// different entries.
pub fn mint(date: &str, text: &str) -> String {
    let h = Sha256::digest(text.as_bytes());
    let hex: String = h.iter().take(3).map(|b| format!("{b:02x}")).collect();
    format!("{}-{hex}", date.replace('-', ""))
}

pub fn path_for(project: &str, date: &str, id: &str) -> String {
    format!("graph/entries/{project}/{}/{id}.md", date.get(..7).unwrap_or(date))
}

pub fn sha256_hex(s: &str) -> String {
    Sha256::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let t = "said: 2026-11-04\nby: Felix\nabout: session.waiting\nrule: A session that waits is not archived.\nreplaces: 20261028-aaaaaa\n\nwe should not be archiving things that are waiting on something\n";
        let e = parse("graph/entries/org/2026-11/20261104-abcdef.md", t).unwrap();
        assert_eq!(e.id, "20261104-abcdef");
        assert_eq!(e.project, "org");
        assert_eq!(e.in_force, "2026-11-04");
        assert_eq!(render(&e), t);
    }

    #[test]
    fn refuses_thin_entries() {
        assert!(parse("graph/entries/org/2026-11/x.md", "said: 2026-11-04\nby: Felix\nabout: a.b\n\nwords\n").is_err());
        assert!(parse("graph/entries/org/2026-11/x.md", "seen: 2026-11-04\nby: x\nabout: a.b\n\nwhat\n").is_err());
        assert!(parse("graph/entries/org/2026-11/x.md", "said: 11-04\nby: Felix\nabout: a.b\nrule: r\n\nw\n").is_err());
        assert!(parse("graph/entries/org/2026-11/x.md", "said: 2026-11-04\nby: Felix\nabout: A B\nrule: r\n\nw\n").is_err());
    }

    #[test]
    fn nodes() {
        assert!(valid_node("session.waiting"));
        assert!(valid_node("everyday-vet.ship"));
        assert!(valid_node("clash/waiting-vs-idle-stop"));
        assert!(!valid_node("Session.waiting"));
        assert!(!valid_node("a..b"));
    }
}
