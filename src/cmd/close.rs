//! A task that has ended closes the session it ran in (Felix, 10-28: "we
//! should cleanup if we determine or the session itself determines its done
//! and have done /abot-land"). Never on a timer: land, dead-end and park are
//! the only triggers, and only once they have succeeded.
//!
//! A `claude --bg` job knows its short id (`crate::session`), so abot
//! stops it itself: a detached `claude stop <short>` a little later, so the
//! session's last message still gets out. Stopping a live Remote Control
//! session archives its sidebar entry, and `claude attach <short>` brings it
//! back. A session started in the desktop app or on claude.ai has no job
//! id and no CLI that archives it, so abot says so and names its id.
//!
//! Everything here goes to stderr: land's last stdout line stays runnable.

use std::process::{Command, Stdio};

pub fn after_end(keep_open: bool) {
    let in_claude = std::env::var_os("CLAUDECODE").is_some();
    let in_job = std::env::var("CLAUDE_JOB_DIR").is_ok_and(|s| !s.trim().is_empty());
    if !in_claude && !in_job {
        return; // a person at a terminal: nothing to close
    }
    if keep_open {
        eprintln!("this session stays open (--keep-open)");
        return;
    }
    let me = crate::session::me();
    let Some(short) = me.job else {
        match me.session {
            Some(sid) => eprintln!(
                "ARCHIVE-SELF: {sid}: this session is not a claude --bg job, so abot can't close it. \
                 If you have an archive_session tool, call it on that id as your very last action; \
                 otherwise end your last message with one line asking Felix to archive this session."
            ),
            None => eprintln!(
                "ARCHIVE-SELF: (this session's id is unknown to abot): it is not a claude --bg job, so abot can't close it. \
                 If you have get_session and archive_session tools, call get_session with no id to learn yours, \
                 then archive_session on it as your very last action; \
                 otherwise end your last message with one line asking Felix to archive this session."
            ),
        }
        return;
    };
    let delay: u64 = std::env::var("ABOT_CLOSE_DELAY").ok().and_then(|d| d.parse().ok()).unwrap_or(30);
    let claude = std::env::var("ABOT_CLAUDE").ok().filter(|m| !m.trim().is_empty()).unwrap_or_else(|| "claude".into());
    // Its own process group and HUP ignored, so it outlives the shell (and
    // the session) it was started from.
    let mut c = Command::new("/bin/sh");
    c.args(["-c", "trap '' HUP; sleep \"$1\"; exec \"$2\" stop \"$3\"", "abot-close", &delay.to_string(), &claude, &short])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    match c.spawn() {
        Ok(_) => eprintln!(
            "closing this session in {delay}s: claude stop {short} (claude attach {short} brings it back; \
             say your last words now, and next time pass --keep-open to stay)"
        ),
        Err(e) => eprintln!("could not close this session ({e}); run: claude stop {short}"),
    }
}
