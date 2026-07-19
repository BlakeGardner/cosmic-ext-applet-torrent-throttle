# cosmic-ext-applet-torrent-throttle

Torrent Throttle for the COSMIC™ desktop — a panel applet that monitors running processes and automatically pauses or throttles torrent downloads based on configurable process name patterns, so your torrents yield bandwidth to the programs you care about. Currently supports qBittorrent (via its Web API); support for other torrent clients is planned.

COSMIC™ is a trademark of System76. This is a third-party application and is not affiliated with or endorsed by System76.

## Features

- **Process Monitoring**: Scans running processes every 30 seconds for configurable name patterns
- **Auto-Pause**: Pauses all qBittorrent downloads when a matching process is detected
- **Auto-Resume**: Resumes downloads when no matching processes are running
- **Panel Applet**: A native COSMIC panel applet (like Wi-Fi/Bluetooth) whose popup has a real toggle switch for monitoring plus live status and throttle info. The settings window is a separate view launched from the popup
- **COSMIC Native**: Built with libcosmic for native integration with the COSMIC desktop
- **Configurable**: Set qBittorrent API connection details and process patterns through the GUI
- **i18n Ready**: Uses Fluent for internationalization
- **cosmic-config**: Persistent settings managed through COSMIC's configuration system

## Upcoming Features

- **Live Speed Display**: See current upload/download speed from your torrent client directly in the applet
- **Multi-Client Support**: Support for torrent clients beyond qBittorrent

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

This installs the binary plus two desktop entries: the settings application and
the panel applet (`io.github.BlakeGardner.cosmic-ext-applet-torrent-throttle.Applet`). Launching the
settings application adds the applet to your panel and starts it if it isn't
already running; you can also place it manually via **COSMIC Settings →
Desktop → Panel → Configure panel applets**.

### Uninstalling

`just uninstall` removes the system-installed files. To also wipe the per-user
footprint (running instances, panel entry, dev desktop entry, config and
state), run:

```bash
./scripts/uninstall-local.sh              # full cleanup
./scripts/uninstall-local.sh --keep-config  # keep your settings
```

## Running

The settings window:

```bash
cargo run --release
```

The panel applet (normally launched by the panel itself):

```bash
cargo run --release -- --applet
```

## Distributing via COSMIC Store

The packaging follows the pattern used by applets in the COSMIC Store:

- The AppStream metainfo declares `<provides><id>com.system76.CosmicApplet</id></provides>`,
  which is what places an app in the store's **Applets** section.
- Applets are distributed through the [COSMIC Flatpak repo](https://github.com/pop-os/cosmic-flatpak)
  (not Flathub) — submit a PR adding `app/io.github.BlakeGardner.cosmic-ext-applet-torrent-throttle/` with the
  manifest from `flatpak/` plus a generated `cargo-sources.json`
  ([flatpak-cargo-generator](https://github.com/flatpak/flatpak-builder-tools/tree/master/cargo)).
- Sandbox support is built in: when running inside Flatpak, process
  monitoring uses `flatpak-spawn --host ps` (enabled by
  `--talk-name=org.freedesktop.Flatpak`, the same pattern used by other
  applets in the COSMIC Flatpak repo), and cross-instance coordination
  (leader election, quit) uses the Flatpak per-app shared runtime
  directory and cosmic-config state instead of signals.

## Configuration

Settings are stored via `cosmic-config` under the app ID `io.github.BlakeGardner.cosmic-ext-applet-torrent-throttle`:

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
