# Demos

Two short demos of what tersh's WebKit view can draw over the terminal grid.
Run them from inside tersh.

## Video in a tmux pane

```sh
examples/demos/video-tmux.sh [clip.webm]
```

Opens a tmux session with `htop` (or `btop`/`top`) on the left and a looping
video covering the right pane, sent with `tersh-show --pane`. Without a clip it
renders an 8 second Mandelbrot zoom with ffmpeg (cached in
`~/.cache/tersh/`). Press Enter in the video pane to close the card.

Needs tmux with `set -g allow-passthrough on`, and ffmpeg for the default clip.
Use WebM: on Linux, MP4/H.264 also needs the GStreamer libav plugin.

## A notebook in Neovim

```sh
examples/demos/notebook-nvim.sh [notebook.ipynb]
```

Opens [`showcase.ipynb`](showcase.ipynb) in Neovim with this repo's tersh.nvim
plugin (and none of your config). The plugin renders the notebook in a split
next to its source: markdown with LaTeX, ANSI-coloured streams, an HTML table,
an interactive Plotly surface, PNG and SVG images, LaTeX and markdown outputs,
and a coloured traceback.
