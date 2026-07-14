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
        self.system
            .refresh_processes(sysinfo::ProcessesToUpdate::All, true);

        let mut matched = Vec::new();

        for process in self.system.processes().values() {
            let name = process.name().to_string_lossy().to_lowercase();
            for pattern in patterns {
                let pattern_lower = pattern.to_lowercase();
                if !pattern_lower.is_empty() && name.contains(&pattern_lower) {
                    let display_name = process.name().to_string_lossy().to_string();
                    if !matched.contains(&display_name) {
                        matched.push(display_name);
                    }
                }
            }
        }

        matched
    }
}
