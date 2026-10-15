//! Plain output with a line budget. Bold headers only on a terminal.

use regex::Regex;
use std::sync::OnceLock;

pub struct Out {
    lines: Vec<String>,
    tty: bool,
}

impl Out {
    pub fn new(tty: bool) -> Out {
        Out { lines: vec![], tty }
    }

    pub fn header(&mut self, s: &str) {
        if !self.lines.is_empty() {
            self.lines.push(String::new());
        }
        if self.tty {
            self.lines.push(format!("\x1b[1m== {s}\x1b[0m"));
        } else {
            self.lines.push(format!("== {s}"));
        }
    }

    pub fn line(&mut self, s: impl Into<String>) {
        self.lines.push(s.into());
    }

    /// Add up to `cap` lines; past that, one line saying how many were cut
    /// and where to see them. An empty input prints "(nothing)".
    pub fn block<I, S>(&mut self, lines: I, cap: usize, more: &str)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let all: Vec<String> = lines.into_iter().map(Into::into).collect();
        if all.is_empty() {
            self.lines.push("(nothing)".into());
            return;
        }
        let n = all.len();
        self.lines.extend(all.into_iter().take(cap));
        if n > cap {
            self.lines.push(format!("(… {} more; {more})", n - cap));
        }
    }

    /// Print, cutting at `cap` lines in total.
    pub fn print(self, cap: usize) {
        let n = self.lines.len();
        for l in self.lines.iter().take(cap) {
            println!("{l}");
        }
        if n > cap {
            println!("(… output capped at {cap} lines; {} more cut)", n - cap);
        }
    }
}

pub fn strip_ansi(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\x1b\[[0-9;?]*[A-Za-z]").unwrap())
        .replace_all(s, "")
        .into_owned()
}

/// Cut to `n` characters with an ellipsis.
pub fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_caps_and_says_nothing() {
        let mut o = Out::new(false);
        o.block(Vec::<String>::new(), 3, "");
        o.block(["a", "b", "c", "d"], 2, "see x");
        assert_eq!(o.lines, vec!["(nothing)", "a", "b", "(… 2 more; see x)"]);
    }

    #[test]
    fn strips_and_clips() {
        assert_eq!(strip_ansi("\x1b[1mhi\x1b[0m \x1b[90m[x]\x1b[0m"), "hi [x]");
        assert_eq!(clip("abcdef", 4), "abc…");
        assert_eq!(clip("abc", 4), "abc");
    }
}
