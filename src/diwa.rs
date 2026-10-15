//! diwa, called as a user would. Nothing about the log's conventions goes
//! into diwa; abot only reads what it prints.

use crate::ctx::Ctx;
use crate::out::strip_ansi;
use std::process::Command;

/// Run diwa and return its lines; failures come back as one line saying so,
/// because diwa is optional context, not a step that can fail a verb.
pub fn lines(ctx: &Ctx, args: &[&str]) -> Vec<String> {
    let Some(bin) = ctx.diwa.as_deref() else {
        return vec!["(diwa skipped: ABOT_DIWA is empty)".into()];
    };
    let o = match Command::new(bin).args(args).output() {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return vec!["(diwa is not installed here; brew install dorky-robot/tap/diwa)".into()];
        }
        Err(e) => return vec![format!("(diwa failed to start: {e})")],
    };
    if !o.status.success() {
        let err = strip_ansi(&String::from_utf8_lossy(&o.stderr));
        let first = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("no message");
        return vec![format!("(diwa {}: {first})", args.first().unwrap_or(&""))];
    }
    let text = String::from_utf8_lossy(&o.stdout);
    let text = if ctx.tty { text.into_owned() } else { strip_ansi(&text) };
    let mut v: Vec<String> = text.lines().map(|l| l.trim_end().to_string()).collect();
    while v.last().is_some_and(|l| l.is_empty()) {
        v.pop();
    }
    v
}
