// SPDX-License-Identifier: GPL-3.0

use sysinfo::System;

pub struct ProcessMonitor {
    system: System,
}

impl ProcessMonitor {
    pub fn new() -> Self {
        Self {
            system: System::new_all(),
        }
    }

    /// Refresh process list and check if any process name contains one of the patterns.
    /// Returns a list of matched process names.
    pub fn check_patterns(&mut self, patterns: &[String]) -> Vec<String> {
        // Inside a Flatpak sandbox only host processes matter, and they are
        // invisible to sysinfo; list them via the sandbox escape instead.
        if crate::sandbox::is_flatpak() {
            if let Some(names) = crate::sandbox::host_process_names() {
                return Self::match_patterns(names.iter().map(String::as_str), patterns);
            }
        }

        self.system
            .refresh_processes(sysinfo::ProcessesToUpdate::All, true);

        Self::match_patterns(
            self.system
                .processes()
                .values()
                .map(|process| process.name().to_str().unwrap_or_default()),
            patterns,
        )
    }

    fn match_patterns<'a>(
        names: impl Iterator<Item = &'a str>,
        patterns: &[String],
    ) -> Vec<String> {
        let mut matched = Vec::new();

        for name in names {
            let name_lower = name.to_lowercase();
            for pattern in patterns {
                let pattern_lower = pattern.to_lowercase();
                if !pattern_lower.is_empty()
                    && name_lower.contains(&pattern_lower)
                    && !matched.iter().any(|m| m == name)
                {
                    matched.push(name.to_string());
                }
            }
        }

        matched
    }
}
