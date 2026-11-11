-- Headless tests for tersh.nvim against a fake tersh socket.
-- Run: nvim --headless -u NONE -l nvim/tests/run.lua (or `just nvim-test`)

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

test("cards cut off by the window's edges are clipped, not shrunk", function()
  vim.api.nvim_buf_set_lines(0, 0, -1, false, lines(100))
  local id = tersh.attach({ kind = "text", data = "x", line = 5, rows = 4 })
  expect(op("show", id), "show")
  -- Line 5 just above the top; 2 of its 4 rows still show as filler.
  vim.fn.winrestview({ topline = 6, topfill = 2, lnum = 16 })
  vim.cmd("redraw")
  tersh.sync()
  local top = expect(op("move", id), "move").anchor
  eq({ top.row, top.rows, top.total, top.skip }, { 0, 2, 4, 2 }, "clipped at the top")
  -- Near the bottom of the window.
  local height = vim.api.nvim_win_get_height(0)
  vim.fn.winrestview({ topline = 1, lnum = 1 })
  vim.cmd("redraw")
  tersh.close(id)
  local low = tersh.attach({ kind = "text", data = "x", line = height - 2, rows = 6 })
  local bottom = expect(op("show", low), "show").anchor
  eq({ bottom.row, bottom.rows, bottom.total, bottom.skip }, { height - 2, 2, 6, 0 }, "clipped at the bottom")
end)

test("cards hide under floating windows and come back when they close", function()
  vim.api.nvim_buf_set_lines(0, 0, -1, false, lines(40))
  local id = tersh.attach({ kind = "text", data = "x", line = 2, rows = 5 })
  expect(op("show", id), "show")
  local float = vim.api.nvim_open_win(vim.api.nvim_create_buf(false, true), false, {
    relative = "editor", row = 4, col = 10, width = 20, height = 3, border = "rounded",
  })
  tersh.sync()
  eq(expect(op("move", id), "move").hidden, true, "hidden under the float")
  vim.api.nvim_win_close(float, true)
  tersh.sync()
  eq(expect(op("move", id), "move").hidden, false, "back after it closes")
  -- A float elsewhere doesn't hide it.
  float = vim.api.nvim_open_win(vim.api.nvim_create_buf(false, true), false, {
    relative = "editor", row = 20, col = 10, width = 20, height = 3,
  })
  tersh.sync()
  vim.wait(50)
  eq(#vim.tbl_filter(op("move", id), received), 0, "unaffected by a distant float")
  vim.api.nvim_win_close(float, true)
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

test("half-edited JSON keeps the last good render", function()
  local fig = vim.fn.tempname() .. ".json"
  vim.cmd("edit " .. fig)
  vim.api.nvim_buf_set_lines(0, 0, -1, false, { '{"data": [], "layout": {}}' })
  tersh.preview(0)
  local msg = expect(op("show"), "show")
  eq(msg.kind, "plotly", "kind")
  vim.api.nvim_buf_set_lines(0, 0, -1, false, { '{"data": [' })
  vim.api.nvim_exec_autocmds("TextChanged", { buffer = 0 })
  vim.wait(300)
  eq(#vim.tbl_filter(op("update", msg.id), received), 0, "no update for invalid JSON")
end)

test("in-place renders hide while their window shows another buffer", function()
  local svg = vim.fn.tempname() .. ".svg"
  vim.cmd("edit " .. svg)
  vim.api.nvim_buf_set_lines(0, 0, -1, false, { "<svg xmlns='http://www.w3.org/2000/svg'/>" })
  local source = vim.api.nvim_get_current_buf()
  local win = vim.api.nvim_get_current_win()
  tersh.preview(0)
  local msg = expect(op("show"), "show")
  eq(#vim.api.nvim_tabpage_list_wins(0), 1, "no split")
  local info = vim.fn.getwininfo(win)[1]
  eq({ msg.anchor.row, msg.anchor.col, msg.anchor.rows, msg.anchor.cols }, { info.winrow - 1, info.wincol - 1, info.height, info.width }, "covers the window")

  vim.cmd("enew")
  tersh.sync()
  eq(expect(op("move", msg.id), "move").hidden, true, "hidden on another buffer")
  vim.cmd("buffer " .. source)
  tersh.sync()
  eq(expect(op("move", msg.id), "move").hidden, false, "shown again")

  tersh.preview(0)
  expect(op("close", msg.id), "close")
  eq(vim.api.nvim_win_is_valid(win), true, "window kept")
end)

test("images render from disk over their window", function()
  local png = vim.fn.tempname() .. ".png"
  local file = assert(io.open(png, "wb"))
  file:write("\137PNG\r\n")
  file:close()
  vim.cmd("edit " .. png)
  tersh.preview(0)
  local msg = expect(op("show"), "show")
  eq({ msg.kind, msg.data }, { "image", "data:image/png;base64," .. vim.base64.encode("\137PNG\r\n") }, "image card")
  eq(#vim.api.nvim_tabpage_list_wins(0), 1, "no split")
end)

test("auto preview: markdown side by side, svg in place, split follows the file", function()
  tersh.setup({ auto_preview = { "md", "svg" } })
  local md = vim.fn.tempname() .. ".md"
  vim.fn.writefile({ "# Title" }, md)
  vim.cmd("edit " .. md)
  eq(expect(op("show"), "markdown show").kind, "markdown", "markdown kind")
  eq(#vim.api.nvim_tabpage_list_wins(0), 2, "markdown split")

  local svg = vim.fn.tempname() .. ".svg"
  vim.fn.writefile({ "<svg xmlns='http://www.w3.org/2000/svg'/>" }, svg)
  vim.cmd("edit " .. svg)
  eq(expect(op("show"), "svg show").kind, "svg", "svg kind")
  vim.wait(200, function()
    return #vim.api.nvim_tabpage_list_wins(0) == 1
  end)
  eq(#vim.api.nvim_tabpage_list_wins(0), 1, "markdown split closed, svg in place")
  tersh.setup({ auto_preview = {} })
end)

-- ---- notebooks ---------------------------------------------------------------------

local NOTEBOOK = {
  cells = {
    { cell_type = "markdown", id = "m1", metadata = vim.empty_dict(), source = { "# Title\n", "Some *text*" } },
    {
      cell_type = "code",
      id = "c1",
      execution_count = 3,
      metadata = { tags = {} },
      source = { "x = 1\n", "print(x)" },
      outputs = { { output_type = "stream", name = "stdout", text = { "1\n" } } },
    },
  },
  metadata = { kernelspec = { language = "python", name = "python3", display_name = "Python 3" } },
  nbformat = 4,
  nbformat_minor = 5,
}

local function open_notebook()
  local path = vim.fn.tempname() .. ".ipynb"
  vim.fn.writefile({ vim.json.encode(NOTEBOOK) }, path)
  vim.cmd("edit " .. path)
  return path
end

local function shows()
  local md, out
  vim.wait(1000, function()
    for _, msg in ipairs(received) do
      if msg.op == "show" and msg.kind == "markdown" then
        md = msg
      elseif msg.op == "show" and msg.kind == "output" then
        out = msg
      end
    end
    return md ~= nil and out ~= nil
  end, 5)
  return md, out
end

test("notebooks open as cells with rendered markdown and outputs", function()
  open_notebook()
  eq(vim.api.nvim_buf_get_lines(0, 0, -1, false), { "# %% [markdown]", "# %%", "x = 1", "print(x)" }, "lines")
  eq(vim.bo.filetype, "python", "filetype")
  eq(vim.bo.modified, false, "unmodified")
  local md, out = shows()
  assert(md and out, "markdown and output cards; got " .. vim.inspect(received))
  -- Row 0 is the kernel winbar, row 1 the markdown marker.
  eq({ md.data, md.chrome, md.anchor.row }, { "# Title\nSome *text*", false, 2 }, "markdown card under its marker")
  eq(out.data[1].text, { "1\n" }, "outputs")
  -- Output card sits under the last code line: rows 0 (md marker), md card, row 1+md rows (code marker), 2 code lines.
  eq(out.anchor.row, md.anchor.row + md.anchor.rows + 3, "output card row")
end)

test("typing in a code cell moves its outputs; writing round-trips the notebook", function()
  local path = open_notebook()
  local _, out = shows()
  vim.api.nvim_buf_set_lines(0, 3, 3, false, { "y = 2" })
  vim.api.nvim_exec_autocmds("TextChanged", { buffer = 0 })
  local moved = expect(function(msg)
    return msg.op == "move" and msg.id == out.id and not msg.hidden and msg.anchor.row == out.anchor.row + 1
  end, "output move")
  assert(moved)
  vim.cmd("write")
  eq(vim.bo.modified, false, "saved")
  local saved = vim.json.decode(table.concat(vim.fn.readfile(path), "\n"))
  eq(saved.cells[2].source, { "x = 1\n", "y = 2\n", "print(x)" }, "code source")
  eq(saved.cells[2].outputs[1].text, { "1\n" }, "outputs kept")
  eq(saved.cells[2].execution_count, 3, "count kept")
  eq(saved.cells[1].source, { "# Title\n", "Some *text*" }, "markdown kept")
  eq(saved.metadata.kernelspec.name, "python3", "metadata kept")
  local text = table.concat(vim.fn.readfile(path), "\n")
  assert(text:find('"metadata": {}', 1, true), "empty metadata stays an object")
  assert(text:find('\n "cells": [', 1, true), "Jupyter-style indent")
end)

test("new cells: typed markers and lines under markdown", function()
  local path = open_notebook()
  vim.api.nvim_buf_set_lines(0, -1, -1, false, { "# %% [markdown]", "# %%", "z = 3" })
  vim.api.nvim_buf_set_lines(0, 1, 1, false, { "import os" }) -- under the markdown cell
  vim.cmd("write")
  local saved = vim.json.decode(table.concat(vim.fn.readfile(path), "\n"))
  local kinds = vim.tbl_map(function(c)
    return c.cell_type
  end, saved.cells)
  eq(kinds, { "markdown", "code", "code", "markdown", "code" }, "cell types")
  eq(saved.cells[2].source, { "import os" }, "implicit cell")
  eq(saved.cells[5].source, { "z = 3" }, "typed cell")
  eq(vim.api.nvim_buf_get_lines(0, 1, 3, false), { "# %%", "import os" }, "marker inserted for the implicit cell")
end)

test("Enter on a markdown cell edits its source; closing re-renders it", function()
  open_notebook()
  local md = shows()
  local nb = vim.api.nvim_get_current_buf()
  vim.api.nvim_win_set_cursor(0, { 1, 0 })
  vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes("<CR>", true, false, true), "x", false)
  local edit = vim.api.nvim_get_current_buf()
  assert(edit ~= nb, "editor opened")
  eq(vim.api.nvim_buf_get_lines(edit, 0, -1, false), { "# Title", "Some *text*" }, "source")
  eq(vim.bo[edit].filetype, "markdown", "filetype")
  vim.api.nvim_buf_set_lines(edit, 0, -1, false, { "# New title" })
  vim.cmd("close")
  expect(function(msg)
    return (msg.op == "show" or msg.op == "update") and msg.id == md.id and msg.data == "# New title"
  end, "re-render with the new markdown")
  eq(vim.bo[nb].modified, true, "notebook modified")
end)

local function gutter(lnum, virt)
  local win = vim.api.nvim_get_current_win()
  local text = vim.api.nvim_eval_statusline(vim.wo[win].statuscolumn, { winid = win, use_statuscol_lnum = lnum }).str
  return vim.trim(text), text
end

local function keys(seq)
  vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes(seq, true, false, true), "mx", false)
end

local function buffer_lines()
  return vim.api.nvim_buf_get_lines(0, 0, -1, false)
end

test("notebooks look like Jupyter: prompts, hidden markers, selected-cell bar", function()
  open_notebook()
  shows()
  vim.api.nvim_win_set_cursor(0, { 3, 0 })
  require("tersh.notebook").render(0)
  eq(vim.wo.conceallevel, 2, "markers concealed")
  eq(gutter(3), "▎ In [3]:", "prompt and bar on the selected code cell")
  eq(gutter(4), "▎", "bar on the rest of the cell")
  eq(gutter(1), "", "markdown cell not selected")
end)

test("command mode: Jupyter keys act on cells", function()
  local path = open_notebook()
  shows()
  vim.api.nvim_win_set_cursor(0, { 3, 0 })
  keys("<Esc>")
  assert(require("tersh.notebook").is_notebook(0), "notebook")
  keys("b")
  eq(buffer_lines(), { "# %% [markdown]", "# %%", "x = 1", "print(x)", "# %%", "" }, "b inserts a code cell below")
  eq(vim.api.nvim_win_get_cursor(0)[1], 6, "new cell selected")
  keys("k")
  eq(vim.api.nvim_win_get_cursor(0)[1], 3, "k selects the cell above")
  keys("c")
  keys("j")
  keys("v")
  eq(#buffer_lines(), 9, "v pastes the copied cell below")
  eq(buffer_lines()[8], "x = 1", "pasted source")
  keys("dd")
  eq(#buffer_lines(), 6, "dd deletes it")
  keys("z")
  eq(#buffer_lines(), 9, "z undoes")
  keys("m")
  eq(buffer_lines()[7], "# %% [markdown]", "m makes it markdown")
  keys("gg")
  keys("j")
  keys("a")
  eq(buffer_lines()[2], "# %%", "a inserts above")
  keys("<CR>")
  eq(vim.api.nvim_get_current_line(), "", "Enter edits the cell")
  vim.cmd("write")
  local saved = vim.json.decode(table.concat(vim.fn.readfile(path), "\n"))
  local kinds = vim.tbl_map(function(c)
    return c.cell_type
  end, saved.cells)
  eq(kinds, { "markdown", "code", "code", "code", "markdown" }, "saved cell types")
  eq(saved.cells[5].source, { "x = 1\n", "print(x)" }, "converted markdown keeps the source")
  eq(saved.cells[3].outputs[1].text, { "1\n" }, "outputs stay with their cell")
end)

test("running cells: outputs, counts and prompts come from the kernel", function()
  tersh.setup({ auto_preview = {}, kernel = { bridge = { "python3", root .. "/tests/fake_kernel.py" } } })
  local path = open_notebook()
  shows()
  local notebook = require("tersh.notebook")
  vim.api.nvim_win_set_cursor(0, { 3, 0 })
  notebook.run_current(0)
  local buf = vim.api.nvim_get_current_buf()
  local done = vim.wait(5000, function()
    local status = require("tersh.kernel").status(buf)
    return status and status.state == "idle" and not vim.tbl_isempty(vim.tbl_filter(function(msg)
      return msg.op == "update" and type(msg.data) == "table" and msg.data[1] and msg.data[1].text == "ran x = 1\nprint(x)\n"
    end, received))
  end, 20)
  assert(done, "output update from the kernel; got " .. vim.inspect(received))
  vim.wait(100)
  eq(gutter(3):match("In %[(%d+)%]"), "1", "execution count in the prompt")
  vim.cmd("write")
  local saved = vim.json.decode(table.concat(vim.fn.readfile(path), "\n"))
  local cell = saved.cells[2]
  eq(cell.execution_count, 1, "saved count")
  eq(#cell.outputs, 2, "stream chunks joined + result")
  eq(cell.outputs[1].text, { "ran x = 1\n", "print(x)\n" }, "stream text")
  eq(cell.outputs[2].data["text/plain"], { "1" }, "result")
  require("tersh.kernel").stop(buf)
  tersh.setup({ auto_preview = {}, kernel = { bridge = false } })
end)

test("preview of a notebook buffer renders its JSON", function()
  open_notebook()
  shows()
  received = {}
  tersh.preview(0)
  local msg = expect(op("show"), "preview")
  eq(msg.kind, "notebook", "kind")
  eq(#vim.json.decode(msg.data).cells, 2, "cells")
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
  local repo = vim.fn.fnamemodify(root, ":h")
  vim.cmd.source(repo .. "/plugin/tersh.lua")
  eq(vim.fn.exists(":TershPreview"), 2, "commands defined")
  eq(vim.tbl_contains(vim.opt.runtimepath:get(), root), true, "nvim/ on runtimepath")
end)

io.write(string.format("\n%d passed, %d failed\n", passed, failures))
server:close()
os.remove(socket)
os.exit(failures == 0 and 0 or 1)
