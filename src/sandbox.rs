// SPDX-License-Identifier: GPL-3.0

//! Flatpak sandbox support.
//!
//! When running inside a Flatpak sandbox the process cannot see host
//! processes (sandboxes get their own PID namespace), and each applet
//! instance runs in its own sandbox. Host process listing goes through
//! `flatpak-spawn --host` (requires `--talk-name=org.freedesktop.Flatpak`),
//! and cross-instance coordination uses the per-app runtime directory,
//! which Flatpak shares between all instances of the same application.

use std::path::{Path, PathBuf};

/// Whether this process is running inside a Flatpak sandbox.
pub fn is_flatpak() -> bool {
    Path::new("/.flatpak-info").exists()
}

/// A runtime directory writable by this process and shared with every other
/// instance of this application, both natively and inside Flatpak.
pub fn shared_runtime_dir() -> PathBuf {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);

    // Flatpak bind-mounts $XDG_RUNTIME_DIR/app/$FLATPAK_ID into every
    // sandbox instance of the same app; the rest of XDG_RUNTIME_DIR is
    // private to each instance.
    match std::env::var_os("FLATPAK_ID") {
        Some(id) if is_flatpak() => runtime_dir.join("app").join(id),
        _ => runtime_dir,
    }
}

/// Names of the processes running on the host, or `None` when they cannot
/// be listed. Names come from the kernel `comm` field, matching what the
/// `sysinfo` crate reports natively.
pub fn host_process_names() -> Option<Vec<String>> {
    let output = std::process::Command::new("flatpak-spawn")
        .args(["--host", "ps", "-eo", "comm="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect(),
    )
}
