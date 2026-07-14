# cosmic-qbit-remote

A COSMIC desktop application that monitors running processes and automatically pauses/resumes qBittorrent downloads based on configurable process name patterns.

## Features

- **Process Monitoring**: Scans running processes every 30 seconds for configurable name patterns
- **Auto-Pause**: Pauses all qBittorrent downloads when a matching process is detected
- **Auto-Resume**: Resumes downloads when no matching processes are running
- **COSMIC Native**: Built with libcosmic for native integration with the COSMIC desktop
- **Configurable**: Set qBittorrent API connection details and process patterns through the GUI

## Use Case

Automatically pause torrent downloads when bandwidth-hungry applications (games, video calls, etc.) are running, and resume when they close.

## Configuration

Settings are stored in `~/.config/cosmic-qbit-remote/config.json`:

- **qBittorrent URL**: The Web UI address (default: `http://localhost:8080`)
- **Username/Password**: qBittorrent Web UI credentials
- **Process Patterns**: List of substrings to match against running process names (case-insensitive)

## Building

```bash
cargo build --release
```

## Requirements

- COSMIC desktop environment (or libcosmic dependencies)
- qBittorrent with Web UI enabled
- Rust toolchain

## Installation

```bash
cargo install --path .
cp resources/com.github.cosmic-qbit-remote.desktop ~/.local/share/applications/
```

## License

GPL-3.0
