if vim.g.loaded_tersh then
  return
end
vim.g.loaded_tersh = true

local command = vim.api.nvim_create_user_command

-- :[N]TershShow [file]  show a file (default: the file under the cursor) below the cursor line, N rows tall.
command("TershShow", function(opts)
  local path = opts.args ~= "" and opts.args or vim.fn.expand("<cfile>")
  if path == "" or vim.fn.filereadable(vim.fn.expand(path)) == 0 then
    vim.notify("tersh: no readable file " .. (path ~= "" and ("'" .. path .. "'") or "under the cursor"), vim.log.levels.ERROR)
    return
  end
  require("tersh").show_file(path, opts.count > 0 and opts.count or nil)
end, { nargs = "?", complete = "file", count = 0, desc = "Show a file as a tersh card below the cursor line" })

-- :TershPreview  toggle a live rendered preview of the current buffer in a split.
command("TershPreview", function()
  require("tersh").preview(0)
end, { desc = "Toggle a live tersh preview of the current buffer" })

-- :TershClose   close the cards on the cursor line;  :TershClose!  close every card.
command("TershClose", function(opts)
  local tersh = require("tersh")
  if opts.bang then
    tersh.close_all()
    return
  end
  local ids = tersh.cards_at(vim.api.nvim_get_current_buf(), vim.api.nvim_win_get_cursor(0)[1])
  for _, id in ipairs(ids) do
    tersh.close(id)
  end
end, { bang = true, desc = "Close tersh cards on this line (! for all)" })

-- Loading the module installs the autocommands that keep cards in place.
if (vim.env.TERSH_SOCKET or vim.env.TERM_PROGRAM == "tersh") and not vim.g.tersh_lazy then
  require("tersh")
end

-- Notebooks: run cells on a Jupyter kernel.
local function notebook_buffer()
  local buf = vim.api.nvim_get_current_buf()
  if not require("tersh.notebook").is_notebook(buf) then
    vim.notify("tersh: not a notebook buffer", vim.log.levels.WARN)
    return nil
  end
  return buf
end

-- :TershRun  run the cell under the cursor;  :TershRun!  run every cell.
command("TershRun", function(opts)
  local buf = notebook_buffer()
  if not buf then
    return
  end
  local notebook = require("tersh.notebook")
  if opts.bang then
    notebook.run_all(buf)
  else
    notebook.run_current(buf)
  end
end, { bang = true, desc = "Run the notebook cell (! for all cells)" })

command("TershInterrupt", function()
  local buf = notebook_buffer()
  if buf then
    require("tersh.kernel").interrupt(buf)
  end
end, { desc = "Interrupt the notebook's kernel" })

command("TershRestart", function()
  local buf = notebook_buffer()
  if buf then
    require("tersh.kernel").restart(buf)
  end
end, { desc = "Restart the notebook's kernel" })
