-- :checkhealth tersh

local M = {}

function M.check()
  local health = vim.health
  local transport = require("tersh.transport")
  health.start("tersh.nvim")

  if vim.fn.has("nvim-0.10") == 1 then
    health.ok("Neovim " .. tostring(vim.version()))
  else
    health.error("Neovim 0.10 or newer is required")
  end

  local socket = transport.socket_path()
  if socket then
    health.ok("connected to tersh through $TERSH_SOCKET (" .. socket .. ")")
  elseif vim.env.TERM_PROGRAM == "tersh" then
    health.warn("$TERSH_SOCKET is not set: falling back to escape sequences", {
      "Inside tmux, add `set -ga update-environment \" TERSH_SOCKET\"` to tmux.conf and re-attach.",
    })
  else
    health.warn("not running inside tersh: cards are not shown", { "Open Neovim in a tersh window." })
  end

  if vim.env.TMUX then
    local out = vim.fn.system({ "tmux", "show", "-gv", "allow-passthrough" })
    if vim.trim(out) == "on" or vim.trim(out) == "all" then
      health.ok("tmux allow-passthrough is " .. vim.trim(out))
    else
      health.warn("tmux allow-passthrough is off", { "Add `set -g allow-passthrough on` to tmux.conf." })
    end
  end
end

return M
