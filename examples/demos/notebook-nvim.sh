#!/usr/bin/env sh
# Demo: a Jupyter notebook rendered next to its source in Neovim.
#
#   examples/demos/notebook-nvim.sh [NOTEBOOK]
#
# Opens the notebook (default: showcase.ipynb) with this repo's tersh.nvim
# plugin and none of your own config. The plugin previews `.ipynb` files in a
# split as soon as they open; `:TershPreview` toggles it.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
notebook=${1:-$here/showcase.ipynb}

exec nvim --clean \
  --cmd "set runtimepath^=$repo/editors/nvim" \
  -c "set termguicolors number" \
  -c "lua require('tersh').setup({ preview = { split = 'vertical', size = 0.6 } })" \
  -c "edit $notebook"
