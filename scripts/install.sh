#!/usr/bin/env bash
# One-line install:  curl -fsSL https://raw.githubusercontent.com/Solomon-mithra/murmur/main/scripts/install.sh | bash
set -euo pipefail
repo="${MURMUR_REPO:-Solomon-mithra/murmur}"

[[ "$(uname -s)" == Darwin && "$(uname -m)" == arm64 ]] || { echo "Murmur needs an Apple Silicon Mac."; exit 1; }

url=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest" | grep -o '"browser_download_url": *"[^"]*\.dmg"' | head -1 | cut -d'"' -f4)
[[ -n "$url" ]] || { echo "No DMG found in the latest release of $repo."; exit 1; }

tmp=$(mktemp -d)
trap 'hdiutil detach -quiet "$tmp/mnt" 2>/dev/null || true; rm -rf "$tmp"' EXIT

echo "→ Downloading $(basename "$url") (~225 MB, models included)"
curl -fL# -o "$tmp/Murmur.dmg" "$url"
hdiutil attach -quiet -nobrowse -mountpoint "$tmp/mnt" "$tmp/Murmur.dmg"

echo "→ Installing to /Applications"
pkill -x murmur 2>/dev/null || true
rm -rf /Applications/Murmur.app
cp -R "$tmp/mnt/Murmur.app" /Applications/
# Not notarized yet: clear the download quarantine so macOS lets it open.
xattr -dr com.apple.quarantine /Applications/Murmur.app 2>/dev/null || true

open /Applications/Murmur.app
echo "✓ Murmur is running. Allow Accessibility + Microphone when macOS asks."
