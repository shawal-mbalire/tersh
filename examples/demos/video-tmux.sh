#!/usr/bin/env sh
# Demo: a video playing inside a tmux pane, next to an ordinary terminal app.
#
#   examples/demos/video-tmux.sh [CLIP]
#
# Without CLIP it renders a short Mandelbrot zoom with ffmpeg. Use WebM
# (VP8/VP9) clips: on Linux, MP4/H.264 needs the gstreamer libav plugin.
# Run it inside tersh; press Enter in the video pane to stop.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
session=tersh-video

if [ -z "${TERSH_SOCKET:-}" ]; then
  echo "Run this inside tersh (\$TERSH_SOCKET is not set)." >&2
  exit 1
fi

# The video card needs a tersh-show that knows `--pane` and video files.
if command -v cargo >/dev/null 2>&1; then
  cargo build --quiet --release --manifest-path "$repo/Cargo.toml" -p tersh-show
  show="$repo/target/release/tersh-show"
else
  show=$(command -v tersh-show)
fi

clip=${1:-}
if [ -z "$clip" ]; then
  clip="${XDG_CACHE_HOME:-$HOME/.cache}/tersh/demo-mandelbrot.webm"
  if [ ! -f "$clip" ]; then
    mkdir -p "$(dirname "$clip")"
    echo "Rendering a demo clip with ffmpeg..."
    ffmpeg -hide_banner -loglevel error -y \
      -f lavfi -i "mandelbrot=size=640x360:rate=30" -t 8 \
      -c:v libvpx-vp9 -b:v 600k -deadline realtime -cpu-used 8 -row-mt 1 "$clip"
  fi
fi
clip=$(cd "$(dirname "$clip")" && pwd)/$(basename "$clip")

# Pane command: wait until a client is attached (so the pane has its real size
# and tmux is on tersh's screen), cover the pane with the clip, and close the
# card when Enter is pressed.
player="until tmux list-clients -t $session 2>/dev/null | grep -q .; do sleep 0.1; done; sleep 0.3
'$show' --pane --id tmux-video --title '$(basename "$clip")' '$clip'
read _
'$show' --close tmux-video"

monitor=top
command -v btop >/dev/null 2>&1 && monitor=btop
command -v htop >/dev/null 2>&1 && monitor=htop

tmux kill-session -t "$session" 2>/dev/null || true
tmux new-session -d -s "$session" -e "TERSH_SOCKET=$TERSH_SOCKET" -x "$(tput cols)" -y "$(tput lines)" "$monitor"
tmux split-window -h -t "$session" -e "TERSH_SOCKET=$TERSH_SOCKET" "$player"

if [ -n "${TMUX:-}" ]; then
  tmux switch-client -t "$session"
else
  tmux attach -t "$session"
fi
