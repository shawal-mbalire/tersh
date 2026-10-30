-- Headless tests for tersh.nvim against a fake tersh socket.
-- Run: nvim --headless -u NONE -l editors/nvim/tests/run.lua

local uv = vim.uv or vim.loop
local root = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":p:h:h")
vim.opt.rtp:prepend(root)

-- ---- fake terminal ----------------------------------------------------------

local received = {}
local socket = vim.fn.tempname() .. ".sock"
local server = uv.new_pipe(false)
assert(server:bind(socket))
server:listen(16, function()
  local client = uv.new_pipe(false)
  server:accept(client)
  local partial = ""
  client:read_start(function(_, chunk)
    if not chunk then
      return
    end
    partial = partial .. chunk
    for line in partial:gmatch("([^\n]*)\n") do
      table.insert(received, vim.json.decode(line))
      client:write('{"ok":true}\n')
    end
    partial = partial:match("[^\n]*$")
  end)
end)
vim.env.TERSH_SOCKET = socket
vim.env.TMUX = nil

-- ---- helpers -------------------------------------------------------------------

local failures, passed = 0, 0

local function test(name, fn)
  received = {}
  local ok, err = pcall(fn)
  if ok then
    passed = passed + 1
    io.write("ok   " .. name .. "\n")
  else
    failures = failures + 1
    io.write("FAIL " .. name .. "\n     " .. tostring(err) .. "\n")
  end
  require("tersh").close_all()
  vim.cmd("silent! only! | enew! | silent! %bwipeout!")
  vim.wo.number = false
  vim.wait(50)
end

--- Wait for the next message matching `pred` and return it.
local function expect(pred, what)
  local found
  vim.wait(1000, function()
    for i, msg in ipairs(received) do
      if pred(msg) then
        found = msg
        table.remove(received, i)
        return true
      end
    end
    return false
  end, 5)
  assert(found, "no " .. what .. " message; got " .. vim.inspect(received))
  return found
end

local function op(name, id)
  return function(msg)
    return msg.op == name and (id == nil or msg.id == id)
  end
end

local function eq(actual, expected, label)
  assert(vim.deep_equal(actual, expected), string.format("%s: expected %s, got %s", label or "value", vim.inspect(expected), vim.inspect(actual)))
end

local function lines(n, width)
  local out = {}
  for i = 1, n do
    out[i] = width and string.rep("x", width) or ("line " .. i)
  end
  return out
end

local function scroll_to(top)
  -- Keep the cursor in view, or Neovim scrolls back to it.
  vim.fn.winrestview({ topline = top, lnum = top + 10 })
  vim.cmd("redraw")
end

local tersh = require("tersh")
tersh.setup({ auto_preview = {} })
local COLS = vim.o.columns

-- ---- tests ------------------------------------------------------------------------

test("line card is placed under its line and reserves rows", function()
  vim.api.nvim_buf_set_lines(0, 0, -1, false, lines(100))
  local id = tersh.attach({ kind = "markdown", data = "# hi", line = 5, rows = 4 })
  local msg = expect(op("show", id), "show")
  eq(msg.kind, "markdown", "kind")
  eq(msg.hidden, false, "hidden")
  eq(msg.anchor, { mode = "cell", row = 5, col = 0, rows = 4, cols = COLS }, "anchor")
  -- Virtual lines push line 6 down by the reserved rows.
  eq(vim.fn.screenpos(0, 6, 1).row, 6 + 4, "line 6 screen row")
end)

test("card follows scrolling and hides when its line leaves the window", function()
  vim.api.nvim_buf_set_lines(0, 0, -1, false, lines(100))
  local id = tersh.attach({ kind = "text", data = "x", line = 5, rows = 4 })
  expect(op("show", id), "show")
  scroll_to(3)
  tersh.sync()
  eq(expect(op("move", id), "move").anchor.row, 3, "row after scroll")
  scroll_to(6)
  tersh.sync()
  eq(expect(op("move", id), "move").hidden, true, "hidden")
  scroll_to(1)
  tersh.sync()
  local back = expect(op("move", id), "move")
  eq({ back.hidden, back.anchor.row }, { false, 5 }, "shown again")
  tersh.sync()
  vim.wait(30)
  eq(#vim.tbl_filter(op("move", id), received), 0, "no redundant moves")
end)

test("rows above with wrapping, numbers and other cards are accounted for", function()
  vim.wo.number = true
  local text = lines(50)
  text[2] = string.rep("w", 2 * COLS + 10) -- wraps onto 3 screen rows
  vim.api.nvim_buf_set_lines(0, 0, -1, false, text)
  local first = tersh.attach({ kind = "text", data = "a", line = 1, rows = 2 })
  local second = tersh.attach({ kind = "text", data = "b", line = 3, rows = 3 })
  local a = expect(op("show", first), "first show").anchor
  local b = expect(op("show", second), "second show").anchor
  local textoff = vim.fn.getwininfo(vim.api.nvim_get_current_win())[1].textoff
  eq({ a.row, a.col, a.cols }, { 1, textoff, COLS - textoff }, "first card")
  -- line 1, 2 reserved, line 2 wrapped over 3 rows, line 3 => row 7
  eq(b.row, 7, "second card row")
end)

test("cards in closed folds are hidden and edits move them", function()
  vim.api.nvim_buf_set_lines(0, 0, -1, false, lines(40))
  local id = tersh.attach({ kind = "text", data = "x", line = 10, rows = 2 })
  eq(expect(op("show", id), "show").anchor.row, 10, "initial row")
  vim.api.nvim_buf_set_lines(0, 0, 0, false, { "new 1", "new 2" })
  vim.cmd("redraw")
  tersh.sync()
  eq(expect(op("move", id), "move").anchor.row, 12, "row after inserting above")
  vim.cmd("8,14fold")
  tersh.sync()
  eq(expect(op("move", id), "move").hidden, true, "hidden in fold")
end)

test("deleting the line closes nothing but hides; wiping the buffer closes", function()
  vim.api.nvim_buf_set_lines(0, 0, -1, false, lines(20))
  local id = tersh.attach({ kind = "text", data = "x", line = 4, rows = 2 })
  expect(op("show", id), "show")
  vim.api.nvim_buf_set_lines(0, 3, 4, false, {})
  tersh.sync()
  eq(expect(op("move", id), "move").hidden, true, "hidden after delete")
  vim.cmd("enew! | silent! bwipeout! #")
  expect(op("close", id), "close")
end)

test("splits: card stays in its window's area", function()
  vim.api.nvim_buf_set_lines(0, 0, -1, false, lines(40))
  vim.cmd("vsplit")
  vim.cmd("wincmd l")
  local id = tersh.attach({ kind = "text", data = "x", line = 2, rows = 2 })
  local anchor = expect(op("show", id), "show").anchor
  local info = vim.fn.getwininfo(vim.api.nvim_get_current_win())[1]
  eq({ anchor.row, anchor.col, anchor.cols }, { 2, info.wincol - 1 + info.textoff, info.width - info.textoff }, "anchor in right window")
end)

test("show_file sends images as data URIs and guesses kinds", function()
  local png = vim.fn.tempname() .. ".png"
  local file = assert(io.open(png, "wb"))
  file:write("\137PNG\r\n")
  file:close()
  vim.api.nvim_buf_set_lines(0, 0, -1, false, lines(10))
  local id = tersh.show_file(png, 6)
  local msg = expect(op("show", id), "show")
  eq(msg.kind, "image", "kind")
  eq(msg.data, "data:image/png;base64," .. vim.base64.encode("\137PNG\r\n"), "data")
  eq(msg.anchor.rows, 6, "rows")
  eq(tersh.guess_kind("fig.json", '{"data":[],"layout":{}}'), "plotly", "plotly")
  eq(tersh.guess_kind("a.ipynb", ""), "notebook", "notebook")
  eq({ tersh.guess_kind("clip.WEBM", "") }, { "video", "video/webm" }, "video")
end)

test("preview covers a split and updates as the buffer changes", function()
  local md = vim.fn.tempname() .. ".md"
  vim.cmd("edit " .. md)
  vim.api.nvim_buf_set_lines(0, 0, -1, false, { "# Title" })
  local source = vim.api.nvim_get_current_buf()
  tersh.preview(0)
  local msg = expect(op("show"), "preview show")
  eq({ msg.kind, msg.data }, { "markdown", "# Title" }, "content")
  local wins = vim.api.nvim_tabpage_list_wins(0)
  eq(#wins, 2, "split opened")
  eq(vim.api.nvim_get_current_buf(), source, "focus stays on the source")
  local preview_win = wins[1] == vim.api.nvim_get_current_win() and wins[2] or wins[1]
  local info = vim.fn.getwininfo(preview_win)[1]
  eq({ msg.anchor.col, msg.anchor.cols, msg.anchor.rows }, { info.wincol - 1, info.width, info.height }, "covers window")

  vim.api.nvim_buf_set_lines(0, 0, -1, false, { "# Changed" })
  vim.api.nvim_exec_autocmds("TextChanged", { buffer = source })
  eq(expect(op("update", msg.id), "update").data, "# Changed", "updated data")

  tersh.preview(0)
  expect(op("close", msg.id), "close")
  eq(#vim.api.nvim_tabpage_list_wins(0), 1, "split closed")
end)

test("half-edited notebooks keep the last good render", function()
  local nb = vim.fn.tempname() .. ".ipynb"
  vim.cmd("edit " .. nb)
  vim.api.nvim_buf_set_lines(0, 0, -1, false, { '{"cells": [], "metadata": {}}' })
  tersh.preview(0)
  local msg = expect(op("show"), "show")
  eq(msg.kind, "notebook", "kind")
  vim.api.nvim_buf_set_lines(0, 0, -1, false, { '{"cells": [' })
  vim.api.nvim_exec_autocmds("TextChanged", { buffer = 0 })
  vim.wait(300)
  eq(#vim.tbl_filter(op("update", msg.id), received), 0, "no update for invalid JSON")
end)

test("suspending hides cards and resuming restores them", function()
  vim.api.nvim_buf_set_lines(0, 0, -1, false, lines(10))
  local id = tersh.attach({ kind = "text", data = "x", line = 1, rows = 2 })
  expect(op("show", id), "show")
  vim.api.nvim_exec_autocmds("VimSuspend", {})
  eq(expect(op("move", id), "move").hidden, true, "hidden on suspend")
  vim.api.nvim_exec_autocmds("VimResume", {})
  eq(expect(op("move", id), "move").hidden, false, "shown on resume")
end)

test("send passes raw messages through and reports availability", function()
  eq(tersh.available(), true, "available with a socket")
  local reply
  tersh.send({ op = "show", id = "raw", kind = "markdown", data = "# hi", anchor = { mode = "float", top = 1 } }, function(r)
    reply = r
  end)
  local msg = expect(op("show", "raw"), "raw show")
  eq(msg.anchor, { mode = "float", top = 1 }, "anchor untouched")
  vim.wait(500, function()
    return reply ~= nil
  end)
  eq(reply and reply.ok, true, "reply")
end)

test("the repository root works as a plugin directory", function()
  vim.g.loaded_tersh = nil
  local repo = vim.fn.fnamemodify(root, ":h:h")
  vim.cmd.source(repo .. "/plugin/tersh.lua")
  eq(vim.fn.exists(":TershPreview"), 2, "commands defined")
  eq(vim.tbl_contains(vim.opt.runtimepath:get(), root), true, "editors/nvim on runtimepath")
end)

io.write(string.format("\n%d passed, %d failed\n", passed, failures))
server:close()
os.remove(socket)
os.exit(failures == 0 and 0 or 1)
