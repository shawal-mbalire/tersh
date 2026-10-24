#!/bin/sh
# tersh installer.
#
#   curl -fsSL https://raw.githubusercontent.com/shawal-mbalire/tersh/main/install.sh | sh
#
# Installs the latest release of the tersh app and the `tersh-show` CLI for the
# current user. Options (pass after `sh -s --`):
#
#   --version vX.Y.Z   install a specific release instead of the latest
#   --nvim             also install the Neovim plugin (as a package under ~/.local/share/nvim/site)
#   --no-app           only install tersh-show (e.g. on a remote machine you ssh into)
#
# Environment: TERSH_BIN_DIR (default ~/.local/bin).

set -eu

REPO="shawal-mbalire/tersh"
BIN_DIR="${TERSH_BIN_DIR:-$HOME/.local/bin}"
VERSION=""
WITH_NVIM=0
WITH_APP=1

while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --nvim) WITH_NVIM=1; shift ;;
    --no-app) WITH_APP=0; shift ;;
    -h | --help) sed -n '2,15p' "$0" 2>/dev/null || true; exit 0 ;;
    *) echo "tersh install: unknown option $1" >&2; exit 1 ;;
  esac
done

say() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "$1 is required"; }

need curl
need uname
need tar

os=$(uname -s)
arch=$(uname -m)
case "$arch" in
  x86_64 | amd64) arch=x86_64 ;;
  arm64 | aarch64) arch=aarch64 ;;
  *) die "unsupported architecture: $arch" ;;
esac
case "$os" in
  Linux) platform=linux ;;
  Darwin) platform=macos ;;
  *) die "unsupported OS: $os (on Windows, download the .msi from https://github.com/$REPO/releases)" ;;
esac

if [ -z "$VERSION" ]; then
  VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n1)
  [ -n "$VERSION" ] || die "could not find the latest release of $REPO"
fi
number=${VERSION#v}
base="https://github.com/$REPO/releases/download/$VERSION"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

fetch() { # fetch URL FILE
  curl -fL --progress-bar "$1" -o "$2" || die "download failed: $1"
}

# Replace FILE with SOURCE in one step (rename), so running copies of the old
# file keep working and running tersh windows see the new one appear.
replace() { # replace SOURCE FILE MODE
  staged="$2.new.$$"
  cp "$1" "$staged" && chmod "$3" "$staged" && mv -f "$staged" "$2" || die "could not write $2"
}

mkdir -p "$BIN_DIR"

# ---- tersh-show ---------------------------------------------------------------
say "Installing tersh-show $VERSION"
fetch "$base/tersh-show-$platform-$arch.tar.gz" "$tmp/tersh-show.tar.gz"
tar -xzf "$tmp/tersh-show.tar.gz" -C "$tmp"
replace "$tmp/tersh-show" "$BIN_DIR/tersh-show" 755

# ---- app ----------------------------------------------------------------------
if [ "$WITH_APP" = 1 ]; then
  if [ "$platform" = linux ]; then
    appimage_arch=$([ "$arch" = x86_64 ] && echo amd64 || echo aarch64)
    say "Installing tersh $VERSION (AppImage)"
    share="${XDG_DATA_HOME:-$HOME/.local/share}"
    mkdir -p "$share/tersh" "$share/applications" "$share/icons/hicolor/128x128/apps"
    fetch "$base/tersh_${number}_${appimage_arch}.AppImage" "$tmp/tersh.AppImage"
    replace "$tmp/tersh.AppImage" "$share/tersh/tersh.AppImage" 755

    # AppImages need FUSE 2; without it, run them by extracting instead.
    fuse_line=""
    if ! { ldconfig -p 2>/dev/null | grep -q 'libfuse\.so\.2'; } && [ ! -e /usr/lib/libfuse.so.2 ] && [ ! -e /usr/lib64/libfuse.so.2 ]; then
      fuse_line="export APPIMAGE_EXTRACT_AND_RUN=1"
    fi
    printf '#!/bin/sh\n%s\nexec "%s" "$@"\n' "$fuse_line" "$share/tersh/tersh.AppImage" >"$tmp/tersh-wrapper"
    replace "$tmp/tersh-wrapper" "$BIN_DIR/tersh" 755
    chmod 755 "$BIN_DIR/tersh"

    fetch "https://raw.githubusercontent.com/$REPO/$VERSION/src-tauri/icons/128x128.png" \
      "$share/icons/hicolor/128x128/apps/tersh.png" 2>/dev/null || true
    cat >"$share/applications/tersh.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=tersh
Comment=Lightweight terminal with rich media cards
Exec=$BIN_DIR/tersh
Icon=tersh
Terminal=false
Categories=System;TerminalEmulator;
Keywords=terminal;shell;console;
EOF
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$share/applications" >/dev/null 2>&1 || true
  else
    dmg_arch=$([ "$arch" = x86_64 ] && echo x64 || echo aarch64)
    say "Installing tersh $VERSION (macOS app)"
    fetch "$base/tersh_${number}_${dmg_arch}.dmg" "$tmp/tersh.dmg"
    mount="$tmp/mount"
    mkdir -p "$mount"
    hdiutil attach -nobrowse -quiet -mountpoint "$mount" "$tmp/tersh.dmg"
    apps="/Applications"
    [ -w "$apps" ] || apps="$HOME/Applications"
    mkdir -p "$apps"
    # Copy beside the old app, then swap, so running windows keep a complete app.
    rm -rf "$apps/.tersh.app.new"
    cp -R "$mount/tersh.app" "$apps/.tersh.app.new"
    hdiutil detach -quiet "$mount"
    rm -rf "$apps/tersh.app"
    mv "$apps/.tersh.app.new" "$apps/tersh.app"
    # The app isn't notarized; clear the quarantine flag so Gatekeeper lets it open.
    xattr -dr com.apple.quarantine "$apps/tersh.app" 2>/dev/null || true
    # With arguments (e.g. `tersh update`) run the binary directly so its output
    # shows; without, open the app normally.
    cat >"$tmp/tersh-wrapper" <<EOF
#!/bin/sh
if [ \$# -gt 0 ]; then exec "$apps/tersh.app/Contents/MacOS/tersh" "\$@"; fi
exec open -a "$apps/tersh.app"
EOF
    replace "$tmp/tersh-wrapper" "$BIN_DIR/tersh" 755
  fi
fi

# ---- Neovim plugin ----------------------------------------------------------------
if [ "$WITH_NVIM" = 1 ]; then
  say "Installing the Neovim plugin"
  fetch "https://github.com/$REPO/archive/refs/tags/$VERSION.tar.gz" "$tmp/src.tar.gz"
  tar -xzf "$tmp/src.tar.gz" -C "$tmp"
  pack="${XDG_DATA_HOME:-$HOME/.local/share}/nvim/site/pack/tersh/start/tersh.nvim"
  rm -rf "$pack"
  mkdir -p "$(dirname "$pack")"
  cp -R "$tmp/tersh-$number/editors/nvim" "$pack"
fi

say "Done."
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "    Add $BIN_DIR to your PATH, e.g.: export PATH=\"$BIN_DIR:\$PATH\"" ;;
esac
[ "$WITH_APP" = 1 ] && echo "    Start it with: tersh   (settings: ~/.config/tersh/config.toml; update later with: tersh update)"
echo "    For tmux: set -g allow-passthrough on; set -g focus-events on"
