//! Commit trailers (`Outcome:`, `Refs:`, `About:`), read the way git reads
//! them: the last paragraph of a message, when every line in it is a
//! `Key: value` line or an indented continuation of one.

pub fn parse(msg: &str) -> Vec<(String, String)> {
    let paras: Vec<&str> = msg
        .trim_end()
        .split("\n\n")
        .map(|p| p.trim_matches('\n'))
        .filter(|p| !p.trim().is_empty())
        .collect();
    // The subject alone is never a trailer block.
    if paras.len() < 2 {
        return vec![];
    }
    let mut out: Vec<(String, String)> = vec![];
    for line in paras[paras.len() - 1].lines() {
        if line.starts_with([' ', '\t']) {
            match out.last_mut() {
                Some((_, v)) => {
                    v.push(' ');
                    v.push_str(line.trim());
                    continue;
                }
                None => return vec![],
            }
        }
        match split_trailer(line) {
            Some((k, v)) => out.push((k.to_string(), v.trim().to_string())),
            None => return vec![],
        }
    }
    out
}

fn split_trailer(line: &str) -> Option<(&str, &str)> {
    let (k, v) = line.split_once(':')?;
    let ok = !k.is_empty()
        && k.chars().next()?.is_ascii_alphanumeric()
        && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    ok.then_some((k, v))
}

/// The last value of `key` (case-insensitive), as git's `trailers:key=` does.
pub fn get(msg: &str, key: &str) -> Option<String> {
    parse(msg)
        .into_iter()
        .rev()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
}

/// One `Code:` trailer: the code a task carried, as `<org/repo> <from>..<to>`
/// (what `abot land` fast-forwarded, or the branch a dead end or park kept).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRange {
    pub repo: String,
    pub from: String,
    pub to: String,
}

impl std::fmt::Display for CodeRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}..{}", self.repo, self.from, self.to)
    }
}

/// Every `Code:` range in a message; several trailers, or several ranges in
/// one split by commas. A value that isn't `<repo> <sha>..<sha>` is skipped.
pub fn code(msg: &str) -> Vec<CodeRange> {
    parse(msg)
        .into_iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("Code"))
        .flat_map(|(_, v)| v.split(',').map(str::trim).map(String::from).collect::<Vec<_>>())
        .filter_map(|v| {
            let (repo, range) = v.split_once(' ')?;
            let (from, to) = range.trim().split_once("..")?;
            let hex = |s: &str| s.len() >= 4 && s.chars().all(|c| c.is_ascii_hexdigit());
            (hex(from) && hex(to)).then(|| CodeRange { repo: repo.to_string(), from: from.to_string(), to: to.to_string() })
        })
        .collect()
}

/// Every `Refs:` target, e.g. `Dorky-Robot/overview@1a2b3c4`, split on
/// commas and spaces so one trailer can name several.
pub fn refs(msg: &str) -> Vec<String> {
    parse(msg)
        .into_iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("Refs"))
        .flat_map(|(_, v)| {
            v.split([',', ' '])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The outcomes abot writes, rendered as `[landed]` and friends.
pub fn outcome_tag(v: &str) -> Option<&'static str> {
    match v.trim().to_ascii_lowercase().as_str() {
        "landed" => Some("[landed]"),
        "dead-end" => Some("[dead-end]"),
        "parked" => Some("[parked]"),
        "in-flight" => Some("[in-flight]"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn code_ranges() {
        let m = "Land a/b\n\nCode: Dorky-Robot/overview 54b2923..1b8fbac, Org/two abcd..ef01\nOutcome: landed";
        let v = super::code(m);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].to_string(), "Dorky-Robot/overview 54b2923..1b8fbac");
        assert_eq!(v[1].repo, "Org/two");
        assert!(super::code("Land a/b\n\nCode: just words\nOutcome: landed").is_empty());
        assert!(super::code("Land a/b\n\nCode: Org/x zz..yy").is_empty(), "not hex");
    }

    use super::*;

    #[test]
    fn reads_last_paragraph() {
        let m = "Land org/x\n\nbody text: not a trailer\n\nOutcome: landed\nCo-Authored-By: A <a@b>\n";
        assert_eq!(get(m, "outcome").as_deref(), Some("landed"));
        assert_eq!(parse(m).len(), 2);
    }

    #[test]
    fn subject_only_has_no_trailers() {
        assert!(parse("Outcome: landed").is_empty());
        assert!(get("Fix: a thing", "Fix").is_none());
    }

    #[test]
    fn mixed_paragraph_is_not_trailers() {
        let m = "Subject\n\nOutcome: landed\nand then prose\n";
        assert!(parse(m).is_empty());
    }

    #[test]
    fn continuation_lines_fold() {
        let m = "S\n\nRefs: Dorky-Robot/overview@abc1234,\n  Dorky-Robot/diwa@def5678\nOutcome: dead-end";
        assert_eq!(
            refs(m),
            vec!["Dorky-Robot/overview@abc1234", "Dorky-Robot/diwa@def5678"]
        );
        assert_eq!(get(m, "Outcome").as_deref(), Some("dead-end"));
    }

    #[test]
    fn last_value_wins_and_tags_render() {
        let m = "S\n\nOutcome: in-flight\nOutcome: parked";
        assert_eq!(get(m, "Outcome").as_deref(), Some("parked"));
        assert_eq!(outcome_tag("Dead-End"), Some("[dead-end]"));
        assert_eq!(outcome_tag("maybe"), None);
    }

    #[test]
    fn url_lines_are_not_trailers() {
        // "https://x" splits as key "https", value "//x": a valid trailer in
        // form, which matches git's own reading; but a key with a dot is not.
        assert!(parse("S\n\nsee.this: x").is_empty());
    }
}
