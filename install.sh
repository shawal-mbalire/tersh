#!/bin/sh
# tersh installer for macOS and Linux (Windows: install.ps1). Usually run
# through the OS-independent `install`:
#
#   curl -fsSL https://raw.githubusercontent.com/shawal-mbalire/tersh/main/install | sh
#
# Installs the latest release for the current user, with everything:
#
#   - the tersh app (Linux: AppImage + desktop entry; macOS: tersh.app)
#   - tersh-show, the card CLI
#   - the Neovim plugin (a lazy.nvim spec if you use lazy.nvim, else a package)
#   - tmux settings for cards (passthrough, focus events, $TERSH_SOCKET)
#   - shell completions (carapace)
#
# Options (after `sh -s --`), or the matching TERSH_* environment variable:
#
#   --version vX.Y.Z    TERSH_VERSION=vX.Y.Z   a specific release instead of the latest
#   --no-app            TERSH_NO_APP=1         skip the app (e.g. on a server you ssh into)
#   --no-nvim           TERSH_NO_NVIM=1        skip the Neovim plugin
#   --no-tmux           TERSH_NO_TMUX=1        leave tmux.conf alone
#   --no-completions    TERSH_NO_COMPLETIONS=1 skip shell completions
#
# --no-* choices are remembered for `tersh update`. TERSH_BIN_DIR sets where
# commands go (default ~/.local/bin).

set -eu

REPO="shawal-mbalire/tersh"
BIN_DIR="${TERSH_BIN_DIR:-$HOME/.local/bin}"
OPTIONS_FILE="${XDG_CONFIG_HOME:-$HOME/.config}/tersh/install-options"
VERSION="${TERSH_VERSION:-}"
NO_APP="${TERSH_NO_APP:-}"
NO_NVIM="${TERSH_NO_NVIM:-}"
NO_TMUX="${TERSH_NO_TMUX:-}"
NO_COMPLETIONS="${TERSH_NO_COMPLETIONS:-}"
CHOSEN=""

while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --no-app) NO_APP=1; CHOSEN=1; shift ;;
    --no-nvim) NO_NVIM=1; CHOSEN=1; shift ;;
    --no-tmux) NO_TMUX=1; CHOSEN=1; shift ;;
    --no-completions) NO_COMPLETIONS=1; CHOSEN=1; shift ;;
    --nvim) shift ;; # the plugin is installed by default now; kept for older `tersh update`s
    -h | --help) sed -n '2,25p' "$0" 2>/dev/null || true; exit 0 ;;
    *) echo "tersh install: unknown option $1" >&2; exit 1 ;;
  esac
done

# Remember --no-* choices, so `tersh update` (which passes none) keeps them.
if [ -n "$CHOSEN" ] || [ -n "$NO_APP$NO_NVIM$NO_TMUX$NO_COMPLETIONS" ]; then
  mkdir -p "$(dirname "$OPTIONS_FILE")"
  printf 'NO_APP=%s\nNO_NVIM=%s\nNO_TMUX=%s\nNO_COMPLETIONS=%s\n' "$NO_APP" "$NO_NVIM" "$NO_TMUX" "$NO_COMPLETIONS" >"$OPTIONS_FILE"
elif [ -f "$OPTIONS_FILE" ]; then
  # shellcheck disable=SC1090
  . "$OPTIONS_FILE"
fi
WITH_APP=$([ -n "$NO_APP" ] && echo 0 || echo 1)

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

try_fetch() { # try_fetch URL FILE
  curl -fsSL "$1" -o "$2" 2>/dev/null
}

# Replace FILE with SOURCE in one step (rename), so running copies of the old
# file keep working and running tersh windows see the new one appear.
replace() { # replace SOURCE FILE MODE
  staged="$2.new.$$"
  cp "$1" "$staged" && chmod "$3" "$staged" && mv -f "$staged" "$2" || die "could not write $2"
}

mkdir -p "$BIN_DIR"

# Also update any other tersh/tersh-show copies earlier on PATH (e.g. ~/.cargo/bin).
# Skip the extracted app: when `tersh update` runs this script, the AppImage
# has put its own usr/bin on PATH, and its `tersh` is the real binary that
# the wrapper launches. Replacing it with the wrapper makes it exec itself.
overridden_bins() { # overridden_bins NAME
  app_dir="${XDG_DATA_HOME:-$HOME/.local/share}/tersh"
  IFS=:
  for dir in $PATH; do
    case "$dir" in
      "$app_dir"/* | "${APPDIR:-/nonexistent}"/*) continue ;;
    esac
    [ -n "$dir" ] && [ "$dir" != "$BIN_DIR" ] && [ -e "$dir/$1" ] && [ -w "$dir" ] && printf '%s\n' "$dir/$1"
  done
  unset IFS
}

install_bin() { # install_bin SOURCE NAME
  replace "$1" "$BIN_DIR/$2" 755
  chmod 755 "$BIN_DIR/$2"
  for target in $(overridden_bins "$2"); do
    replace "$1" "$target" 755
    chmod 755 "$target"
  done
}

# ---- tersh-show ---------------------------------------------------------------
say "Installing tersh-show $VERSION"
fetch "$base/tersh-show-$platform-$arch.tar.gz" "$tmp/tersh-show.tar.gz"
tar -xzf "$tmp/tersh-show.tar.gz" -C "$tmp"
install_bin "$tmp/tersh-show" "tersh-show"

# ---- app ----------------------------------------------------------------------
if [ "$WITH_APP" = 1 ]; then
  if [ "$platform" = linux ]; then
    appimage_arch=$([ "$arch" = x86_64 ] && echo amd64 || echo aarch64)
    say "Installing tersh $VERSION"
    share="${XDG_DATA_HOME:-$HOME/.local/share}"
    mkdir -p "$share/tersh" "$share/applications" "$share/icons/hicolor/128x128/apps"
    fetch "$base/tersh_${number}_${appimage_arch}.AppImage" "$tmp/tersh.AppImage"
    replace "$tmp/tersh.AppImage" "$share/tersh/tersh.AppImage" 755

    # Extract the AppImage once at install time so launching tersh is instant
    # and works on distributions without FUSE 2 (e.g. Fedora 40+, Ubuntu 24.04+).
    rm -rf "$share/tersh/app.new" "$share/tersh/app.old"
    (cd "$tmp" && "$share/tersh/tersh.AppImage" --appimage-extract >/dev/null 2>&1) || true
    if [ -x "$tmp/squashfs-root/AppRun" ]; then
      mkdir -p "$tmp/squashfs-root/share/themes/tersh/gtk-3.0"
      cat >"$tmp/squashfs-root/share/themes/tersh/gtk-3.0/gtk.css" <<'GTKEOF'
@import url("resource:///org/gtk/libgtk/theme/Adwaita/gtk-contained-dark.css");
window, window.background, window.csd, decoration {
  background-color: transparent;
  background-image: none;
  box-shadow: none;
}
GTKEOF
      if [ -f "$tmp/squashfs-root/apprun-hooks/linuxdeploy-plugin-gtk.sh" ]; then
        sed -i 's/dbus-send/true/g; s/gsettings/true/g' "$tmp/squashfs-root/apprun-hooks/linuxdeploy-plugin-gtk.sh" 2>/dev/null || true
      fi
      mv "$tmp/squashfs-root" "$share/tersh/app.new"
      [ -e "$share/tersh/app" ] && mv -f "$share/tersh/app" "$share/tersh/app.old"
      mv -f "$share/tersh/app.new" "$share/tersh/app"
      rm -rf "$share/tersh/app.old"
    fi

    cat >"$tmp/tersh-wrapper" <<EOF
#!/bin/sh
export TERSH_WRAPPER="$BIN_DIR/tersh"
export APPIMAGE_GTK_THEME="tersh"
if [ -x "$share/tersh/app/AppRun" ]; then
  exec "$share/tersh/app/AppRun" "\$@"
fi
if ! { PATH="\$PATH:/sbin:/usr/sbin" ldconfig -p 2>/dev/null | grep -q 'libfuse\.so\.2'; } && [ ! -e /usr/lib/libfuse.so.2 ] && [ ! -e /usr/lib64/libfuse.so.2 ] && [ ! -e /lib/x86_64-linux-gnu/libfuse.so.2 ] && [ ! -e /lib/aarch64-linux-gnu/libfuse.so.2 ]; then
  export APPIMAGE_EXTRACT_AND_RUN=1
fi
exec "$share/tersh/tersh.AppImage" "\$@"
EOF
    install_bin "$tmp/tersh-wrapper" "tersh"

    try_fetch "https://raw.githubusercontent.com/$REPO/$VERSION/src-tauri/icons/128x128.png" \
      "$share/icons/hicolor/128x128/apps/tersh.png" || true
    cat >"$share/applications/tersh.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=tersh
Comment=Lightweight terminal with rich media cards
Exec=$BIN_DIR/tersh
TryExec=$BIN_DIR/tersh
Icon=tersh
Terminal=false
StartupWMClass=tersh
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
    install_bin "$tmp/tersh-wrapper" "tersh"
  fi
fi

# ---- shell completions ---------------------------------------------------------------
if [ -z "$NO_COMPLETIONS" ]; then
  # carapace (bash, zsh, fish, nushell, ...) reads specs from its config directory.
  for specs in "${XDG_CONFIG_HOME:-$HOME/.config}/carapace/specs" "$HOME/Library/Application Support/carapace/specs"; do
    [ -d "$specs" ] || continue
    say "Installing completions for carapace"
    try_fetch "$base/tersh_completion_${number}.yaml" "$specs/tersh.yaml" \
      || try_fetch "https://raw.githubusercontent.com/$REPO/$VERSION/completions/tersh.yaml" "$specs/tersh.yaml" \
      || true
  done
fi

# ---- Neovim plugin ------------------------------------------------------------------
# lazy.nvim users get a plugin spec (lazy resets the package path, so a package
# would never load); everyone else gets a package Neovim loads on its own.
if [ -z "$NO_NVIM" ] && command -v nvim >/dev/null 2>&1; then
  nvim_config="${XDG_CONFIG_HOME:-$HOME/.config}/nvim"
  nvim_data="${XDG_DATA_HOME:-$HOME/.local/share}/nvim"
  pack="$nvim_data/site/pack/tersh/start/tersh.nvim"
  if grep -rqs "shawal-mbalire/tersh" "$nvim_config"; then
    say "Neovim plugin: already in your config (lazy.nvim keeps it updated)"
    rm -rf "$pack"
  elif [ -d "$nvim_data/lazy/lazy.nvim" ] && [ -d "$nvim_config/lua/plugins" ]; then
    say "Installing the Neovim plugin (lazy.nvim spec: $nvim_config/lua/plugins/tersh.lua)"
    cat >"$nvim_config/lua/plugins/tersh.lua" <<'LUAEOF'
-- tersh.nvim: images, video, notebooks, markdown and plots as cards in tersh.
-- Added by tersh's installer; edit freely (it isn't overwritten once present).
return {
  {
    "shawal-mbalire/tersh",
    main = "tersh",
    lazy = false, -- small, and needed at startup to preview notebooks as they open
    opts = {},
    keys = {
      { "<leader>Tp", "<cmd>TershPreview<cr>", desc = "tersh: toggle preview" },
      { "<leader>Ts", "<cmd>TershShow<cr>", desc = "tersh: show file under cursor" },
      { "<leader>Tx", "<cmd>TershClose!<cr>", desc = "tersh: close all cards" },
    },
  },
}
LUAEOF
    rm -rf "$pack"
  else
    say "Installing the Neovim plugin (package: $pack)"
    fetch "https://github.com/$REPO/archive/refs/tags/$VERSION.tar.gz" "$tmp/src.tar.gz"
    tar -xzf "$tmp/src.tar.gz" -C "$tmp"
    rm -rf "$pack"
    mkdir -p "$(dirname "$pack")"
    cp -R "$tmp/tersh-$number/editors/nvim" "$pack"
    [ -d "$nvim_data/lazy/lazy.nvim" ] \
      && echo "    lazy.nvim found without lua/plugins/: add { \"shawal-mbalire/tersh\", main = \"tersh\", lazy = false, opts = {} } to your specs"
  fi
fi

# ---- tmux -------------------------------------------------------------------------------
# Cards inside tmux need passthrough; focus events hide cards of background
# windows; TERSH_SOCKET lets new panes use the socket (no size limit).
if [ -z "$NO_TMUX" ] && command -v tmux >/dev/null 2>&1; then
  tmux_conf="${XDG_CONFIG_HOME:-$HOME/.config}/tmux/tmux.conf"
  [ -f "$tmux_conf" ] || tmux_conf="$HOME/.tmux.conf"
  if ! grep -qs ">>> tersh >>>" "$tmux_conf"; then
    say "Adding tersh's settings to $tmux_conf"
    cat >>"$tmux_conf" <<'TMUXEOF'

# >>> tersh >>> (added by tersh's installer; safe to remove)
set -g allow-passthrough on
set -g focus-events on
set -ga update-environment " TERSH_SOCKET"
# <<< tersh <<<
TMUXEOF
  fi
  # Apply to a running server too, without re-sourcing the whole config.
  if tmux info >/dev/null 2>&1; then
    tmux set -g allow-passthrough on 2>/dev/null || true
    tmux set -g focus-events on 2>/dev/null || true
    tmux show -g update-environment 2>/dev/null | grep -q TERSH_SOCKET || tmux set -ga update-environment " TERSH_SOCKET" 2>/dev/null || true
  fi
fi

say "Done."
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "    Add $BIN_DIR to your PATH, e.g.: export PATH=\"$BIN_DIR:\$PATH\"" ;;
esac
[ "$WITH_APP" = 1 ] && echo "    Start it with: tersh   (settings: ~/.config/tersh/config.toml; update later with: tersh update)"
exit 0
