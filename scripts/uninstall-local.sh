#!/usr/bin/env bash
# Removes every per-user trace of Torrent Throttle: running instances, the
# panel config entry, the dev desktop entry, the app's config/state
# directories, and — if installed — the flatpak app, its sandbox data, and
# permission overrides.
#
# This does NOT touch a system install ('just uninstall' handles files
# installed under /usr).
#
# Usage: ./scripts/uninstall-local.sh [--keep-config]
#   --keep-config   keep ~/.config settings (URL, patterns, mode)

set -euo pipefail

APPID="io.github.BlakeGardner.cosmic-ext-applet-torrent-throttle"
APPLET_ID="${APPID}.Applet"
BIN_NAME="cosmic-ext-applet-torrent-throttle"

KEEP_CONFIG=0
if [[ "${1:-}" == "--keep-config" ]]; then
    KEEP_CONFIG=1
fi

echo "==> Stopping running instances"
# Killing one instance broadcast-quits the rest via the shared quit signal,
# but sweep all PIDs to be thorough.
mapfile -t pids < <(pgrep -f "${BIN_NAME}" || true)
for pid in "${pids[@]:-}"; do
    [[ -n "$pid" && "$pid" != "$$" ]] && kill "$pid" 2>/dev/null || true
done
[[ ${#pids[@]} -gt 0 ]] && sleep 1

echo "==> Removing applet from panel config"
shopt -s nullglob
for f in ~/.config/cosmic/com.system76.CosmicPanel.*/v1/plugins_wings \
         ~/.config/cosmic/com.system76.CosmicPanel.*/v1/plugins_center; do
    if grep -q "$APPLET_ID" "$f"; then
        sed -i "/\"$APPLET_ID\",\?/d" "$f"
        echo "    removed from $f"
    fi
done

echo "==> Removing desktop entry"
rm -fv ~/.local/share/applications/"$APPLET_ID".desktop \
       ~/.local/share/applications/"$APPID".desktop

if command -v flatpak >/dev/null && flatpak info "$APPID" >/dev/null 2>&1; then
    echo "==> Uninstalling flatpak"
    flatpak uninstall -y "$APPID"
fi
if command -v flatpak >/dev/null; then
    # Drop any 'flatpak override' grants (harmless if none exist).
    flatpak override --user --reset "$APPID" 2>/dev/null || true
fi
rm -rf ~/.var/app/"$APPID"

echo "==> Removing state"
rm -rf ~/.local/state/cosmic/"$APPID"
runtime_dir="${XDG_RUNTIME_DIR:-/run/user/$UID}"
rm -f "$runtime_dir/${BIN_NAME}-applet.lock" \
      "$runtime_dir/app/$APPID/${BIN_NAME}-applet.lock"

if [[ $KEEP_CONFIG -eq 0 ]]; then
    echo "==> Removing config"
    rm -rf ~/.config/cosmic/"$APPID"
else
    echo "==> Keeping config (--keep-config)"
fi

echo "==> Done. Torrent Throttle has been removed for this user."
