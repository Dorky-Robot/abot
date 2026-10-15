//! A cheap secrets scan: the shapes of tokens, keys, passwords and phone
//! numbers. The log is shared, so it holds references, never secrets or
//! records. A hit is a refusal, not a judgement: move the thing out and
//! point at where it lives.

use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: String,
    pub line: usize,
    pub kind: &'static str,
    pub excerpt: String,
}

impl std::fmt::Display for Hit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: looks like {} ({})", self.path, self.line, self.kind, self.excerpt)
    }
}

/// Transcripts are quoted conversation; phone-number shapes there are not
/// ours to police, and `check` skips them altogether.
pub fn is_transcript(path: &str) -> bool {
    path.starts_with("docs/transcripts/")
}

fn patterns() -> &'static [(&'static str, Regex)] {
    static P: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    P.get_or_init(|| {
        [
            ("an API key (sk-)", r"\bsk-[A-Za-z0-9_-]{20,}"),
            ("a GitHub token", r"\b(gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{20,})"),
            ("an AWS access key", r"\bAKIA[0-9A-Z]{16}\b"),
            ("a Slack token", r"\bxox[bpas]-[A-Za-z0-9-]{10,}"),
            ("a password", r"(?i)\bpassword\s*=\s*\S"),
            ("a private key", r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY( BLOCK)?-----"),
        ]
        .into_iter()
        .map(|(k, r)| (k, Regex::new(r).expect("secret pattern")))
        .collect()
    })
}

fn phone() -> &'static Regex {
    // North American numbers: area codes start 2-9, which keeps 10-digit
    // unix timestamps (1xxxxxxxxx) out. The number must stand on its own: ten
    // digits inside a run of letters and digits (a sha256 in Cargo.lock, a
    // commit hash, an id) are not a phone number.
    static P: OnceLock<Regex> = OnceLock::new();
    P.get_or_init(|| {
        Regex::new(r"(?:^|[^0-9A-Za-z_])(\(?[2-9]\d{2}\)?[-. ]\d{3}[-. ]\d{4}|[2-9]\d{2}[2-9]\d{6})(?:$|[^0-9A-Za-z_])")
            .expect("phone pattern")
    })
}

fn mask(s: &str) -> String {
    let keep: String = s.chars().take(4).collect();
    format!("{keep}…")
}

pub fn scan_line(path: &str, n: usize, line: &str) -> Vec<Hit> {
    let mut hits = vec![];
    for (kind, re) in patterns() {
        if let Some(m) = re.find(line) {
            hits.push(Hit { path: path.into(), line: n, kind, excerpt: mask(m.as_str()) });
        }
    }
    if !is_transcript(path)
        && let Some(c) = phone().captures(line)
    {
        hits.push(Hit { path: path.into(), line: n, kind: "a phone number", excerpt: mask(&c[1]) });
    }
    hits
}

pub fn scan_text(path: &str, text: &str) -> Vec<Hit> {
    text.lines()
        .enumerate()
        .flat_map(|(i, l)| scan_line(path, i + 1, l))
        .collect()
}

/// Scan only the added lines of a unified diff (`git diff -U0`), with their
/// new-file line numbers.
pub fn scan_diff(diff: &str) -> Vec<Hit> {
    let mut hits = vec![];
    let mut path = String::new();
    let mut n = 0usize;
    for line in diff.lines() {
        if let Some(p) = line.strip_prefix("+++ ") {
            path = p.strip_prefix("b/").unwrap_or(p).to_string();
        } else if let Some(h) = line.strip_prefix("@@ ") {
            // @@ -a,b +c,d @@
            n = h
                .split_whitespace()
                .find_map(|t| t.strip_prefix('+'))
                .and_then(|t| t.split(',').next())
                .and_then(|t| t.parse().ok())
                .unwrap_or(0);
        } else if let Some(added) = line.strip_prefix('+') {
            hits.extend(scan_line(&path, n, added));
            n += 1;
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_token_shapes() {
        let cases = [
            "key sk-ant-api03-abcdefghijklmnopqrstuvwxyz",
            "ghp_0123456789abcdefghijklmnopqrstuvwxyzAB",
            "AKIAABCDEFGHIJKLMNOP",
            "xoxb-1234567890-abcdef",
            "PASSWORD = hunter2",
            "-----BEGIN OPENSSH PRIVATE KEY-----",
            "call (512) 555-0134",
            "call 512.555.0134",
            "call 5125550134 now",
        ];
        for c in cases {
            assert_eq!(scan_text("state/x.md", c).len(), 1, "{c}");
        }
    }

    #[test]
    fn leaves_ordinary_text() {
        let clean = [
            "d-127 at 2026-10-15, sha c411170abc",
            "unix time 1760000000",
            "the password is in 1Password, not here",
            "task-sk-x",
            "-----BEGIN CERTIFICATE-----",
            "port 8080, ip 100.64.1.2",
            "ticket 0123456789",
        ];
        for c in clean {
            assert!(scan_text("state/x.md", c).is_empty(), "{c}: {:?}", scan_text("state/x.md", c));
        }
    }

    #[test]
    fn digits_inside_a_hash_are_not_a_phone_number() {
        // archive/org-kita/Cargo.lock:119 tripped the old pattern.
        let lock = r#"checksum = "3b1c8a9e5125550134f0d2c6a7e8b9fa0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f""#;
        assert!(scan_text("archive/x/Cargo.lock", lock).is_empty());
        assert!(scan_text("state/x.md", "sha 9d2f6a5125550134bc").is_empty());
        // A number standing on its own is still caught.
        assert_eq!(scan_text("state/x.md", "call 5125550134 today").len(), 1);
        assert_eq!(scan_text("state/x.md", "tel:512-555-0134.").len(), 1);
    }

    #[test]
    fn transcripts_keep_phone_numbers_but_not_tokens() {
        assert!(scan_text("docs/transcripts/a.md", "512-555-0134").is_empty());
        assert_eq!(scan_text("docs/transcripts/a.md", "AKIAABCDEFGHIJKLMNOP").len(), 1);
    }

    #[test]
    fn diff_scan_reports_new_line_numbers() {
        let d = "diff --git a/state/x.md b/state/x.md\n--- a/state/x.md\n+++ b/state/x.md\n@@ -3,0 +4,2 @@\n+fine\n+token ghp_0123456789abcdefghijklmnopqrstuvwxyzAB\n";
        let h = scan_diff(d);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].path, "state/x.md");
        assert_eq!(h[0].line, 5);
        assert!(h[0].excerpt.starts_with("ghp_") && !h[0].excerpt.contains("0123"));
    }
}
