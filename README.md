# cosmic-qbit-remote

A COSMIC desktop application that monitors running processes and automatically pauses/resumes qBittorrent downloads based on configurable process name patterns.

## Features

- **Process Monitoring**: Scans running processes every 30 seconds for configurable name patterns
- **Auto-Pause**: Pauses all qBittorrent downloads when a matching process is detected
- **Auto-Resume**: Resumes downloads when no matching processes are running
- **System Tray**: Minimizes to the system tray (StatusNotifierItem); the tray menu shows the current status and throttle limits, and lets you toggle monitoring, reopen the window, or quit
- **COSMIC Native**: Built with libcosmic for native integration with the COSMIC desktop
- **Configurable**: Set qBittorrent API connection details and process patterns through the GUI
- **i18n Ready**: Uses Fluent for internationalization
- **cosmic-config**: Persistent settings managed through COSMIC's configuration system

## Use Case

Automatically pause torrent downloads when bandwidth-hungry applications (games, video calls, etc.) are running, and resume when they close.

## Building

```bash
cargo build --release
```

Or using `just`:

```bash
just build-release
```

## Installation

```bash
just install
```

## Running

```bash
just run
```

Or directly:

```bash
cargo run --release
```

## Configuration

Settings are stored via `cosmic-config` under the app ID `com.github.cosmic-qbit-remote`:

- **qBittorrent URL**: The Web UI address (default: `http://localhost:8080`)
- **Username/Password**: qBittorrent Web UI credentials
- **Process Patterns**: List of substrings to match against running process names (case-insensitive)
- **Poll Interval**: How often to scan processes (default: 30 seconds)

## Requirements

- COSMIC desktop environment (or libcosmic dependencies)
- qBittorrent with Web UI enabled
- Rust toolchain

## License

GPL-3.0
