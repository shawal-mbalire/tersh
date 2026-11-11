#!/usr/bin/env bash
# Install what building the tersh desktop app needs on Linux.
#
#   scripts/setup-linux.sh             install the WebKitGTK/GTK development packages on this machine (sudo)
#   scripts/setup-linux.sh --toolbox   set up a Fedora toolbox "tersh-dev" with them instead
#
# Rust (rustup) and pnpm are expected to be installed already; pnpm installs the pinned bun.
set -euo pipefail

DNF_PACKAGES=(webkit2gtk4.1-devel libsoup3-devel gtk3-devel librsvg2-devel openssl-devel gcc gcc-c++ make pkgconf-pkg-config)
APT_PACKAGES=(libwebkit2gtk-4.1-dev libsoup-3.0-dev libgtk-3-dev librsvg2-dev libssl-dev build-essential pkg-config)
PACMAN_PACKAGES=(webkit2gtk-4.1 libsoup3 gtk3 librsvg openssl base-devel pkgconf)
# For headless runs (Xvfb) and screenshots in the toolbox.
EXTRA_TOOLBOX=(xorg-x11-server-Xvfb xdotool ImageMagick xterm)

if [[ "${1:-}" == "--toolbox" ]]; then
  name=tersh-dev
  command -v toolbox >/dev/null || { echo "toolbox is not installed" >&2; exit 1; }
  toolbox list --containers | grep -qw "$name" || toolbox create -y "$name"
  # The Cisco openh264 repo is often unreachable and isn't needed.
  toolbox run -c "$name" sudo dnf install -y --disablerepo='*openh264*' --setopt=install_weak_deps=False \
    "${DNF_PACKAGES[@]}" "${EXTRA_TOOLBOX[@]}"
  cat <<EOF

Done. Build and run inside the toolbox (your home directory is shared):

  just setup && just frontend-dev &              # on the host or in the toolbox
  toolbox run -c $name bash -lc 'cd $(pwd)/backend && cargo build -p tersh && ./target/debug/tersh'

or enter it with: toolbox enter $name
EOF
  exit 0
fi

if command -v dnf >/dev/null; then
  sudo dnf install -y "${DNF_PACKAGES[@]}"
elif command -v apt-get >/dev/null; then
  sudo apt-get update && sudo apt-get install -y "${APT_PACKAGES[@]}"
elif command -v pacman >/dev/null; then
  sudo pacman -S --needed --noconfirm "${PACMAN_PACKAGES[@]}"
else
  echo "Unknown package manager; install the WebKitGTK 4.1 and GTK 3 development packages yourself." >&2
  exit 1
fi
pkg-config --modversion webkit2gtk-4.1 >/dev/null && echo "WebKitGTK $(pkg-config --modversion webkit2gtk-4.1) is ready: just setup && just dev"
