use std::path::Path;

/// Check if a process with the given PID is alive.
/// Returns true even when we lack permission to signal it (EPERM).
pub fn process_alive(pid: i32) -> bool {
    let ret = unsafe { libc::kill(pid, 0) };
    if ret == 0 {
        return true;
    }
    // EPERM means the process exists but we can't signal it
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Read a PID from a file, returning Some(pid) only if the file exists,
/// parses to a valid PID, and that process is still alive.
pub fn read_live_pid(path: &Path) -> Option<i32> {
    let contents = std::fs::read_to_string(path).ok()?;
    let pid = contents.trim().parse::<i32>().ok()?;
    if process_alive(pid) {
        Some(pid)
    } else {
        None
    }
}

/// Verify that a PID belongs to an abot process by checking its executable path.
/// Returns true if we can't determine the path (conservative: assume it's ours).
#[cfg(target_os = "macos")]
pub fn is_abot_process(pid: i32) -> bool {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let ret =
        unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr() as *mut libc::c_void, buf.len() as u32) };
    if ret <= 0 {
        // Can't determine — be conservative, assume it's ours
        return true;
    }
    let path = String::from_utf8_lossy(&buf[..ret as usize]);
    path.ends_with("/abot")
}

#[cfg(not(target_os = "macos"))]
pub fn is_abot_process(pid: i32) -> bool {
    // On Linux, check /proc/<pid>/exe
    if let Ok(exe) = std::fs::read_link(format!("/proc/{}/exe", pid)) {
        return exe.file_name().map(|n| n == "abot").unwrap_or(true);
    }
    true // Conservative fallback
}
