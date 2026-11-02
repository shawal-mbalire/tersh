-- tersh.notebook: open .ipynb files as cells, the way notebook editors show them.
--
-- The buffer holds the notebook in "percent" form: each cell starts with a
-- `# %%` line (`# %% [markdown]`, `# %% [raw]`), and code cells keep their
-- source below it, so the code can be edited with the usual filetype, LSP and
-- highlighting. Markdown cells are drawn as rendered cards (Enter on one edits
-- its source), and each code cell's saved outputs are drawn below it.
-- Writing the buffer writes the notebook back as nbformat JSON.

local api = vim.api
local M = {}

local ns = api.nvim_create_namespace("tersh-notebook") -- one extmark per cell marker line
local ns_hl = api.nvim_create_namespace("tersh-notebook-hl") -- decorations, redrawn on change
local MARKER = "^# %%%%"
local EMPTY_DICT_MT = getmetatable(vim.empty_dict())

---@class TershNotebook
---@field nb table           the notebook without its cells
---@field by_mark table<integer, table> extmark id -> cell
---@field cards table<string, {line: integer, key: string}>
---@field next integer       next cell uid
---@field timer any

---@type table<integer, TershNotebook>
local states = {}

local function tersh()
  return require("tersh")
end

-- ---- JSON --------------------------------------------------------------------------

local function is_dict(value)
  return getmetatable(value) == EMPTY_DICT_MT or (next(value) ~= nil and not vim.islist(value))
end

--- JSON the way Jupyter writes it: sorted keys, one-space indent, raw UTF-8.
local function encode(value, depth)
  depth = depth or 0
  local kind = type(value)
  if value == nil or value == vim.NIL then
    return "null"
  elseif kind == "boolean" then
    return tostring(value)
  elseif kind == "number" then
    if value == math.floor(value) and math.abs(value) < 2 ^ 53 then
      return string.format("%d", value)
    end
    return vim.json.encode(value)
  elseif kind == "string" then
    return vim.json.encode(value)
  end
  local pad, inner = string.rep(" ", depth), string.rep(" ", depth + 1)
  local parts = {}
  if is_dict(value) then
    local keys = vim.tbl_keys(value)
    if #keys == 0 then
      return "{}"
    end
    table.sort(keys)
    for _, key in ipairs(keys) do
      table.insert(parts, inner .. vim.json.encode(key) .. ": " .. encode(value[key], depth + 1))
    end
    return "{\n" .. table.concat(parts, ",\n") .. "\n" .. pad .. "}"
  end
  if #value == 0 then
    return "[]"
  end
  for _, item in ipairs(value) do
    table.insert(parts, inner .. encode(item, depth + 1))
  end
  return "[\n" .. table.concat(parts, ",\n") .. "\n" .. pad .. "]"
end

M.encode = encode

local function join_source(source)
  if type(source) == "table" then
    return table.concat(source, "")
  end
  return type(source) == "string" and source or ""
end

--- nbformat stores source as a list of lines, each but the last ending in "\n".
local function split_source(text)
  if text == "" then
    return {}
  end
  local out = {}
  for line in (text .. "\n"):gmatch("(.-\n)") do
    table.insert(out, line)
  end
  out[#out] = out[#out]:sub(1, -2)
  if out[#out] == "" then
    out[#out] = nil
  end
  return out
end

local function shallow_copy(value)
  local copy = setmetatable({}, getmetatable(value))
  for k, v in pairs(value) do
    copy[k] = v
  end
  return copy
end

-- ---- cards' sizes --------------------------------------------------------------------

--- Text columns of the window showing `buf`.
local function text_cols(buf)
  for _, win in ipairs(api.nvim_tabpage_list_wins(0)) do
    if api.nvim_win_get_buf(win) == buf then
      local info = vim.fn.getwininfo(win)[1]
      return info.width - info.textoff
    end
  end
  return vim.o.columns
end

local function wrapped(line, per_row)
  return math.max(1, math.ceil(vim.fn.strdisplaywidth(line) / per_row))
end

--- Rows a rendered markdown text needs, estimated from its source. Cards scroll
--- when the estimate is short.
function M.markdown_rows(text, cols)
  -- Card text is proportional and a little taller than a terminal row.
  local per_row = math.max(20, math.floor(cols * 1.15))
  local rows, in_code, in_math = 1, false, false
  for _, line in ipairs(vim.split(text, "\n")) do
    if line:match("^%s*```") or line:match("^%s*~~~") then
      in_code = not in_code
      rows = rows + 0.8
    elseif in_code then
      rows = rows + 1.1
    elseif line:match("^%s*%$%$") then
      local closes = line:match("^%s*%$%$.*%$%$%s*$") and #line > 4
      if closes then
        rows = rows + 3
      else
        in_math = not in_math
        rows = rows + (in_math and 1 or 0.5)
      end
    elseif in_math then
      rows = rows + 1.4
    elseif line:match("^#%s") then
      rows = rows + 3
    elseif line:match("^##+%s") then
      rows = rows + 2.4
    elseif line:match("^%s*|") then
      rows = rows + (line:match("^%s*|[%s:|-]+|%s*$") and 0 or 1.6)
    elseif line:match("!%[.-%]%(.-%)") or line:match("<img") then
      rows = rows + 12
    elseif line:match("^%s*$") then
      rows = rows + 0.7
    else
      rows = rows + 1.2 * wrapped(line, per_row)
    end
  end
  return math.max(2, math.ceil(rows) + 1)
end

local function png_size(b64)
  local ok, head = pcall(vim.base64.decode, b64:gsub("%s", ""):sub(1, 44))
  if not ok or head:sub(2, 4) ~= "PNG" then
    return nil
  end
  local function u32(at)
    local a, b, c, d = head:byte(at, at + 3)
    return ((a * 256 + b) * 256 + c) * 256 + d
  end
  return u32(17), u32(21)
end

local CELL_PX = { w = 8.4, h = 18 }

local function image_rows(height, width, cols)
  local max_w = cols * CELL_PX.w - 24
  local h = width > max_w and height * max_w / width or height
  return math.ceil(h / CELL_PX.h) + 1
end

local function text_rows(text, cols)
  local rows = 0
  for _, line in ipairs(vim.split(text, "\n")) do
    rows = rows + wrapped(line, cols)
  end
  return rows
end

--- Rows the saved outputs of a code cell need.
function M.output_rows(outputs, cols)
  local rows = 0
  for _, out in ipairs(outputs) do
    local kind = out.output_type
    if kind == "stream" then
      rows = rows + text_rows(join_source(out.text):gsub("\n$", ""), cols)
    elseif kind == "error" then
      -- Each traceback entry may hold several lines (and ANSI colours).
      local text = table.concat(out.traceback or {}, "\n"):gsub("\27%[[%d;]*m", "")
      rows = rows + text_rows(text, cols) + 1
    else
      local data = out.data or {}
      if data["application/vnd.plotly.v1+json"] then
        rows = rows + 24
      elseif data["image/png"] or data["image/jpeg"] then
        local w, h = png_size(join_source(data["image/png"] or ""))
        rows = rows + (w and image_rows(h, w, cols) or 16)
      elseif data["image/svg+xml"] then
        local svg = join_source(data["image/svg+xml"])
        local h = tonumber(svg:match('height="([%d.]+)'))
        local w = tonumber(svg:match('width="([%d.]+)'))
        rows = rows + ((h and w) and image_rows(h, w, cols) or 16)
      elseif data["text/html"] then
        local html = join_source(data["text/html"])
        local _, trs = html:gsub("<tr", "")
        rows = rows + (trs > 0 and math.ceil(trs * 1.6) + 1 or text_rows(html:gsub("<[^>]+>", ""), cols))
      elseif data["text/markdown"] then
        rows = rows + M.markdown_rows(join_source(data["text/markdown"]), cols)
      elseif data["text/latex"] then
        rows = rows + 3
      elseif data["text/plain"] then
        rows = rows + text_rows(join_source(data["text/plain"]), cols)
      else
        rows = rows + 3
      end
    end
    rows = rows + 1
  end
  return math.min(math.max(rows + 1, 2), 60)
end

-- ---- reading ---------------------------------------------------------------------------

local function new_cell(state, kind, raw)
  state.next = state.next + 1
  return { uid = state.next, type = kind, raw = raw, source = raw and join_source(raw.source) or "" }
end

local function marker_kind(line)
  local tag = line:match("^# %%%%%s*%[(%w+)%]")
  if tag == "markdown" or tag == "md" then
    return "markdown"
  elseif tag == "raw" then
    return "raw"
  end
  return "code"
end

local function marker_line(kind)
  return kind == "code" and "# %%" or ("# %% [" .. kind .. "]")
end

local function language(nb)
  local meta = nb.metadata or {}
  local lang = (meta.kernelspec or {}).language or (meta.language_info or {}).name or "python"
  return lang:lower()
end

local function empty_notebook()
  return {
    cells = {},
    metadata = vim.empty_dict(),
    nbformat = 4,
    nbformat_minor = 5,
  }
end

local function set_lines_without_undo(buf, lines)
  local levels = vim.bo[buf].undolevels
  vim.bo[buf].undolevels = -1
  api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  vim.bo[buf].undolevels = levels
end

--- True when `buf` shows a notebook as cells.
function M.is_notebook(buf)
  return states[buf ~= 0 and buf or api.nvim_get_current_buf()] ~= nil
end

--- Read `path` into `buf` as cells. Falls back to the raw text if it isn't a notebook.
function M.read(buf, path)
  local text = ""
  local file = io.open(path, "rb")
  if file then
    text = file:read("*a")
    file:close()
  end
  local nb
  if text:match("^%s*$") then
    nb = empty_notebook()
  else
    local ok, value = pcall(vim.json.decode, text)
    if ok and type(value) == "table" and type(value.cells) == "table" then
      nb = value
    end
  end
  if not nb then
    set_lines_without_undo(buf, vim.split(text, "\n", { plain = true }))
    vim.bo[buf].filetype = "json"
    vim.bo[buf].modified = false
    vim.notify("tersh: " .. vim.fn.fnamemodify(path, ":t") .. " is not a valid notebook; showing it as JSON", vim.log.levels.WARN)
    return
  end

  local old = states[buf]
  if old then
    M.detach(buf)
  end
  local state = { nb = shallow_copy(nb), by_mark = {}, cards = {}, next = 0 }
  state.nb.cells = nil
  local cells, lines = {}, {}
  for _, raw in ipairs(nb.cells) do
    local kind = raw.cell_type == "markdown" and "markdown" or raw.cell_type == "raw" and "raw" or "code"
    local cell = new_cell(state, kind, raw)
    table.insert(cells, { cell = cell, row = #lines })
    table.insert(lines, marker_line(kind))
    if kind ~= "markdown" then
      vim.list_extend(lines, vim.split(cell.source, "\n", { plain = true }))
    end
  end
  if #lines == 0 then
    local cell = new_cell(state, "code")
    table.insert(cells, { cell = cell, row = 0 })
    lines = { "# %%", "" }
  end
  set_lines_without_undo(buf, lines)
  for _, entry in ipairs(cells) do
    local mark = api.nvim_buf_set_extmark(buf, ns, entry.row, 0, { invalidate = true })
    state.by_mark[mark] = entry.cell
  end
  states[buf] = state

  vim.bo[buf].buftype = "acwrite"
  vim.bo[buf].swapfile = false
  vim.bo[buf].modified = false
  vim.bo[buf].filetype = language(nb)
  M.attach(buf)
end

-- ---- the buffer as cells ------------------------------------------------------------------

--- Cells in buffer order: { cell?, row?, lines }. `cell` is nil for lines typed
--- outside any code cell (before the first marker or under a markdown cell).
local function scan(buf)
  local state = states[buf]
  local lines = api.nvim_buf_get_lines(buf, 0, -1, false)
  local at = {}
  for _, mark in ipairs(api.nvim_buf_get_extmarks(buf, ns, 0, -1, { details = true })) do
    local id, row, _, details = unpack(mark)
    local cell = state.by_mark[id]
    if cell and not details.invalid and not at[row] and (lines[row + 1] or ""):match(MARKER) then
      at[row] = cell
    elseif not details.invalid and not (lines[row + 1] or ""):match(MARKER) then
      -- The marker text was edited away: the cell is gone.
      api.nvim_buf_del_extmark(buf, ns, id)
      state.by_mark[id] = nil
    end
  end
  local cells, current = {}, nil
  for i, line in ipairs(lines) do
    local row = i - 1
    if line:match(MARKER) then
      local kind = marker_kind(line)
      local cell = at[row]
      if not cell or cell.type ~= kind then
        -- A marker typed by hand, or one whose type was changed.
        if cell then
          for id, c in pairs(state.by_mark) do
            if c == cell then
              api.nvim_buf_del_extmark(buf, ns, id)
              state.by_mark[id] = nil
            end
          end
        end
        cell = new_cell(state, kind)
        state.by_mark[api.nvim_buf_set_extmark(buf, ns, row, 0, { invalidate = true })] = cell
      end
      current = { cell = cell, row = row, lines = {} }
      table.insert(cells, current)
    else
      if not current or (current.cell and current.cell.type == "markdown") then
        current = { first = row, lines = {} }
        table.insert(cells, current)
      end
      table.insert(current.lines, line)
      current.last = row
    end
  end
  return cells
end

local function trimmed(lines)
  local last = #lines
  while last > 0 and lines[last]:match("^%s*$") do
    last = last - 1
  end
  return table.concat(lines, "\n", 1, last)
end

--- Multi-line strings in outputs as lists of lines, the way nbformat writes them.
local function normalize_output(out)
  local copy = shallow_copy(out)
  if type(copy.text) == "string" then
    copy.text = split_source(copy.text)
  end
  if type(copy.data) == "table" and is_dict(copy.data) then
    local data = setmetatable({}, getmetatable(copy.data))
    for mime, value in pairs(copy.data) do
      local textual = mime:match("^text/") or mime == "image/svg+xml"
      data[mime] = (textual and type(value) == "string") and split_source(value) or value
    end
    copy.data = data
  end
  return copy
end

--- The buffer as an nbformat notebook table.
function M.to_notebook(buf)
  local state = states[buf]
  local out = {}
  for _, entry in ipairs(scan(buf)) do
    local cell = entry.cell
    local source = (cell and cell.type == "markdown") and cell.source or trimmed(entry.lines)
    if cell or source ~= "" then
      local kind = cell and cell.type or "code"
      local raw = cell and cell.raw and shallow_copy(cell.raw) or { metadata = vim.empty_dict() }
      if not raw.metadata then
        raw.metadata = vim.empty_dict()
      end
      if not raw.id and (state.nb.nbformat or 4) >= 4 and (state.nb.nbformat_minor or 0) >= 5 then
        raw.id = string.format("%08x", math.random(0, 0x7fffffff))
      end
      raw.cell_type = kind
      raw.source = split_source(source)
      if kind == "code" then
        raw.outputs = vim.tbl_map(normalize_output, raw.outputs or {})
        if raw.execution_count == nil then
          raw.execution_count = vim.NIL
        end
      else
        raw.outputs, raw.execution_count = nil, nil
      end
      table.insert(out, raw)
    end
  end
  local nb = shallow_copy(state.nb)
  nb.cells = out
  return nb
end

--- The buffer as notebook JSON text.
function M.to_json(buf)
  return encode(M.to_notebook(buf)) .. "\n"
end

--- Write the notebook to `path` (default: the buffer's file).
function M.write(buf, path)
  path = path or api.nvim_buf_get_name(buf)
  if not states[buf] then
    vim.fn.writefile(api.nvim_buf_get_lines(buf, 0, -1, false), path)
    vim.bo[buf].modified = false
    return
  end
  -- Give lines typed outside a cell their own marker, so the view matches the file.
  local rows = {}
  for _, entry in ipairs(scan(buf)) do
    if not entry.cell and trimmed(entry.lines) ~= "" then
      table.insert(rows, 1, entry.first)
    end
  end
  for _, row in ipairs(rows) do
    api.nvim_buf_set_lines(buf, row, row, false, { "# %%" })
  end
  local file, err = io.open(path, "wb")
  if not file then
    vim.notify("tersh: cannot write " .. path .. ": " .. tostring(err), vim.log.levels.ERROR)
    return
  end
  file:write(M.to_json(buf))
  file:close()
  if path == api.nvim_buf_get_name(buf) then
    vim.bo[buf].modified = false
  end
  api.nvim_echo({ { string.format('"%s" written', vim.fn.fnamemodify(path, ":~:.")) } }, false, {})
  M.render(buf)
end

-- ---- drawing ---------------------------------------------------------------------------------

local function define_highlights()
  api.nvim_set_hl(0, "TershCodeCell", { default = true, link = "ColorColumn" })
  api.nvim_set_hl(0, "TershPromptIn", { default = true, link = "Function" })
  api.nvim_set_hl(0, "TershPromptOut", { default = true, link = "Constant" })
  -- Jupyter's colours for the selected cell: blue in command mode, green while editing.
  api.nvim_set_hl(0, "TershBarCommand", { default = true, fg = "#42a5f5" })
  api.nvim_set_hl(0, "TershBarEdit", { default = true, fg = "#66bb6a" })
end

local function card_id(buf, cell, what)
  return string.format("nvim-%d-nb%d-%s-%d", vim.fn.getpid(), buf, what, cell.uid)
end

--- Show a card, or move, resize and update the one already there. `version`
--- changes whenever its data does.
local function place(state, buf, id, version, opts)
  local known = state.cards[id]
  state.cards[id] = { version = version, line = opts.line, rows = opts.rows }
  if not known then
    opts.id, opts.buf, opts.chrome = id, buf, false
    tersh().attach(opts)
    return
  end
  if known.line ~= opts.line or known.rows ~= opts.rows then
    tersh().move_line(id, opts.line, opts.rows)
  end
  if known.version ~= version then
    tersh().update(id, opts.data)
  end
end

local function has_result(outputs)
  for _, out in ipairs(outputs) do
    if out.output_type == "execute_result" then
      return true
    end
  end
  return false
end

--- First and last buffer rows (0-based) of a cell, marker included.
local function span(entry)
  return entry.row or entry.first, entry.last or entry.row
end

--- Window-local options that make a window look like a notebook.
local function style_window(win)
  for option, value in pairs({
    statuscolumn = "%!v:lua.require'tersh.notebook'.statuscolumn()",
    number = true, -- the status column is only drawn with a number column
    relativenumber = false,
    signcolumn = "no",
    foldcolumn = "0",
    conceallevel = 2,
    concealcursor = "",
    wrap = true,
    winbar = "%!v:lua.require'tersh.notebook'.winbar()",
  }) do
    api.nvim_set_option_value(option, value, { win = win, scope = "local" })
  end
end

--- Redraw cell markers, code backgrounds, prompts and cards.
function M.render(buf)
  buf = (buf == nil or buf == 0) and api.nvim_get_current_buf() or buf
  local state = states[buf]
  if not state or not api.nvim_buf_is_valid(buf) then
    return
  end
  local cols = text_cols(buf)
  local seen = {}
  local layout = { cell_of = {}, prompt_at = {}, out_at = {}, entries = {} }
  api.nvim_buf_clear_namespace(buf, ns_hl, 0, -1)
  local lines = api.nvim_buf_get_lines(buf, 0, -1, false)
  for index, entry in ipairs(scan(buf)) do
    table.insert(layout.entries, entry)
    local cell = entry.cell
    local first, last = span(entry)
    for row = first, last do
      layout.cell_of[row] = index
    end
    if cell then
      -- The marker line is blank space between cells, shown only under the cursor.
      api.nvim_buf_set_extmark(buf, ns_hl, entry.row, 0, {
        end_col = #lines[entry.row + 1],
        conceal = "",
      })
    end
    local code = not cell or cell.type ~= "markdown"
    if code then
      local body = entry.row and entry.row + 1 or entry.first
      for row = body, last do
        api.nvim_buf_set_extmark(buf, ns_hl, row, 0, { line_hl_group = "TershCodeCell" })
      end
      if not cell or cell.type == "code" then
        local count = cell and cell.raw and cell.raw.execution_count
        local shown = (cell and cell.running) and "*" or (type(count) == "number" and count or " ")
        layout.prompt_at[math.min(body, last)] = string.format("In [%s]:", shown)
      end
    end
    if cell and cell.type == "markdown" then
      local id = card_id(buf, cell, "md")
      seen[id] = true
      local text = cell.source ~= "" and cell.source or "*Empty markdown cell. Press Enter to edit it.*"
      local rows = M.markdown_rows(text, cols)
      layout.card_rows = layout.card_rows or {}
      layout.card_rows[entry.row] = rows
      place(state, buf, id, text, { kind = "markdown", data = text, line = entry.row + 1, rows = rows })
    elseif cell and cell.type == "code" and not cell.hide_output and cell.raw and type(cell.raw.outputs) == "table" and #cell.raw.outputs > 0 then
      local id = card_id(buf, cell, "out")
      seen[id] = true
      local rows = M.output_rows(cell.raw.outputs, cols)
      local count = cell.raw.execution_count
      layout.out_at[last] = {
        rows = rows,
        label = (has_result(cell.raw.outputs) and type(count) == "number") and string.format("Out[%d]:", count) or "",
      }
      layout.card_rows = layout.card_rows or {}
      layout.card_rows[last] = rows
      place(state, buf, id, cell.version or 0, { kind = "output", data = cell.raw.outputs, line = last + 1, rows = rows })
    end
  end
  state.layout = layout
  for id in pairs(state.cards) do
    if not seen[id] then
      state.cards[id] = nil
      tersh().close(id)
    end
  end
  for _, win in ipairs(vim.fn.win_findbuf(buf)) do
    if vim.wo[win].statuscolumn == "" or not vim.wo[win].statuscolumn:find("tersh.notebook", 1, true) then
      style_window(win)
    end
  end
  M.update_current(buf)
  tersh().sync()
end

--- The gutter: In/Out prompts and the selected cell's bar.
function M.statuscolumn()
  local win = vim.g.statusline_winid
  local state = states[api.nvim_win_get_buf(win)]
  local layout = state and state.layout
  if not layout then
    return ""
  end
  local row, virt = vim.v.lnum - 1, vim.v.virtnum
  local cell, label, hl = layout.cell_of[row], "", "TershPromptIn"
  if virt < 0 then
    -- A card's rows are virtual lines below its line, reported with that
    -- line's number and virtnum -rows (top) .. -1.
    local out = layout.out_at[row]
    if out and virt == -out.rows then
      label, hl = out.label, "TershPromptOut"
    end
  elseif virt == 0 then
    label = layout.prompt_at[row] or ""
  end
  local bar = " "
  if cell and cell == state.current then
    bar = "%#" .. (state.command and "TershBarCommand" or "TershBarEdit") .. "#▎"
  end
  return bar .. "%#" .. hl .. "#" .. string.format("%8s ", label) .. "%*"
end

--- Track the cell under the cursor and redraw the gutter when it changes.
function M.update_current(buf)
  local state = states[buf]
  if not state or not state.layout then
    return
  end
  local win = api.nvim_get_current_win()
  if api.nvim_win_get_buf(win) ~= buf then
    return
  end
  local current = state.layout.cell_of[api.nvim_win_get_cursor(win)[1] - 1]
  if current ~= state.current or state.drawn_command ~= state.command then
    state.current, state.drawn_command = current, state.command
    pcall(api.nvim__redraw, { buf = buf, statuscolumn = true })
  end
end

-- ---- editing markdown cells ----------------------------------------------------------------

--- The markdown cell whose marker is on `row` (0-based), if any.
local function markdown_at(buf, row)
  for _, entry in ipairs(scan(buf)) do
    if entry.cell and entry.row == row and entry.cell.type == "markdown" then
      return entry.cell
    end
  end
end

--- Edit a markdown cell's source in a split below. The rendered cell follows
--- as you type; `q` or any way of closing the split finishes.
function M.edit_markdown(buf, cell)
  local lines = vim.split(cell.source, "\n", { plain = true })
  local height = math.min(math.max(#lines + 1, 6), math.floor(vim.o.lines * 0.4))
  vim.cmd(string.format("belowright %dnew", height))
  local edit = api.nvim_get_current_buf()
  local win = api.nvim_get_current_win()
  api.nvim_buf_set_lines(edit, 0, -1, false, lines)
  vim.bo[edit].buftype = "nofile"
  vim.bo[edit].bufhidden = "wipe"
  vim.bo[edit].swapfile = false
  vim.bo[edit].filetype = "markdown"
  vim.bo[edit].modified = false
  api.nvim_buf_set_name(edit, string.format("%s [markdown cell %d]", api.nvim_buf_get_name(buf), cell.uid))
  local function save()
    if not api.nvim_buf_is_valid(edit) or not api.nvim_buf_is_valid(buf) then
      return
    end
    local text = trimmed(api.nvim_buf_get_lines(edit, 0, -1, false))
    if text ~= cell.source then
      cell.source = text
      vim.bo[buf].modified = true
      M.render(buf)
    end
  end
  local timer = (vim.uv or vim.loop).new_timer()
  api.nvim_create_autocmd({ "TextChanged", "TextChangedI" }, {
    buffer = edit,
    callback = function()
      timer:stop()
      timer:start(tersh().config.debounce, 0, vim.schedule_wrap(save))
    end,
  })
  api.nvim_create_autocmd("BufWipeout", {
    buffer = edit,
    once = true,
    callback = function()
      timer:stop()
      timer:close()
    end,
  })
  api.nvim_create_autocmd("WinClosed", {
    pattern = tostring(win),
    once = true,
    callback = save,
  })
  vim.keymap.set("n", "q", "<cmd>close<cr>", { buffer = edit, desc = "Save the markdown cell and close" })
end

-- ---- lifecycle ------------------------------------------------------------------------------

local group = api.nvim_create_augroup("tersh-notebook", { clear = true })

local function schedule_render(buf, delay)
  local state = states[buf]
  if not state then
    return
  end
  state.timer = state.timer or (vim.uv or vim.loop).new_timer()
  state.timer:stop()
  state.timer:start(delay or tersh().config.debounce, 0, vim.schedule_wrap(function()
    M.render(buf)
  end))
end

-- ---- cells as units: Jupyter's command mode ----------------------------------------------

local clipboard -- { kind, source, raw } of the last cut or copied cell

local function entries(buf)
  local state = states[buf]
  if not state.layout then
    M.render(buf)
  end
  return state.layout.entries
end

local function current_index(buf)
  M.render(buf)
  local state = states[buf]
  return state.layout.cell_of[api.nvim_win_get_cursor(0)[1] - 1] or 1
end

--- The row the cursor goes to when a cell is selected.
local function home_row(entry)
  local first, last = span(entry)
  if entry.cell and entry.cell.type == "markdown" then
    return first
  end
  return math.min(entry.row and entry.row + 1 or first, last)
end

--- Scroll just enough to show a whole cell with its output, as Jupyter does;
--- a cell taller than the window goes to the top.
function M.reveal(buf, entry)
  local win = api.nvim_get_current_win()
  local first, last = span(entry)
  local height = api.nvim_win_get_height(win)
  local card = (states[buf].layout.card_rows or {})[last] or 0
  local view = vim.fn.winsaveview()
  local top = view.topline - 1
  -- Screen rows of buffer rows a..b, leaving out the filler above a.
  local function rows(a, b)
    local span_height = api.nvim_win_text_height(win, { start_row = a, end_row = b }).all
    return span_height - api.nvim_win_text_height(win, { start_row = a, end_row = a }).fill
  end
  if first < top then
    vim.fn.winrestview({ topline = first + 1, topfill = 0 })
    return
  end
  if (view.topfill or 0) + rows(top, last) + card <= height then
    return
  end
  if rows(first, last) + card >= height then
    vim.fn.winrestview({ topline = first + 1, topfill = 0 })
    return
  end
  local new_top = top
  while new_top < first and rows(new_top, last) + card > height do
    new_top = new_top + 1
  end
  vim.fn.winrestview({ topline = new_top + 1, topfill = 0 })
end

local function select_cell(buf, index)
  local list = entries(buf)
  index = math.max(1, math.min(index, #list))
  local entry = list[index]
  if not entry then
    return
  end
  api.nvim_win_set_cursor(0, { home_row(entry) + 1, 0 })
  M.reveal(buf, entry)
  M.update_current(buf)
end

--- Insert a cell at `row` (0-based) and return its index after rendering.
local function insert_cell(buf, row, kind, source, raw)
  local state = states[buf]
  local lines = { marker_line(kind) }
  if kind ~= "markdown" then
    vim.list_extend(lines, source ~= "" and vim.split(source, "\n", { plain = true }) or { "" })
  end
  api.nvim_buf_set_lines(buf, row, row, false, lines)
  local cell = new_cell(state, kind, raw and shallow_copy(raw) or nil)
  cell.source = source
  if cell.raw then
    cell.raw.id = nil -- a pasted cell is a new cell
  end
  state.by_mark[api.nvim_buf_set_extmark(buf, ns, row, 0, { invalidate = true })] = cell
  M.render(buf)
  return states[buf].layout.cell_of[row]
end

local function cell_source(entry)
  if entry.cell and entry.cell.type == "markdown" then
    return entry.cell.source
  end
  return trimmed(entry.lines)
end

local function copy_cell(entry)
  clipboard = { kind = entry.cell and entry.cell.type or "code", source = cell_source(entry), raw = entry.cell and entry.cell.raw }
end

local function delete_cell(buf, entry)
  local first, last = span(entry)
  api.nvim_buf_set_lines(buf, first, last + 1, false, {})
  if api.nvim_buf_line_count(buf) == 1 and api.nvim_buf_get_lines(buf, 0, 1, false)[1] == "" then
    insert_cell(buf, 0, "code", "")
    api.nvim_buf_set_lines(buf, -2, -1, false, {})
  end
  M.render(buf)
end

local function set_marker(buf, entry, kind)
  local old = api.nvim_buf_get_lines(buf, entry.row, entry.row + 1, false)[1]
  api.nvim_buf_set_text(buf, entry.row, 0, entry.row, #old, { marker_line(kind) })
end

--- Change a cell's type, keeping its source.
local function convert(buf, entry, kind)
  if not entry.cell then
    -- Lines outside any cell: give them a marker first.
    insert_cell(buf, entry.first, "code", "")
    api.nvim_buf_set_lines(buf, entry.first + 1, entry.first + 2, false, {})
    M.render(buf)
    entry = entries(buf)[states[buf].layout.cell_of[entry.first]]
  end
  local cell = entry.cell
  if cell.type == kind then
    return
  end
  if kind == "markdown" then
    cell.source = trimmed(entry.lines)
    if entry.last then
      api.nvim_buf_set_lines(buf, entry.row + 1, entry.last + 1, false, {})
    end
  elseif cell.type == "markdown" then
    api.nvim_buf_set_lines(buf, entry.row + 1, entry.row + 1, false, vim.split(cell.source, "\n", { plain = true }))
  end
  cell.type = kind
  set_marker(buf, entry, kind)
  M.render(buf)
end

-- ---- running cells ---------------------------------------------------------------------------

local function kernel()
  return require("tersh.kernel")
end

local function kernel_info(state)
  local meta = state.nb.metadata or {}
  local spec = meta.kernelspec or {}
  return { name = spec.name, display_name = spec.display_name, language = spec.language or (meta.language_info or {}).name }
end

local function cell_by_uid(buf, uid)
  for _, entry in ipairs(scan(buf)) do
    if entry.cell and entry.cell.uid == uid then
      return entry.cell, entry
    end
  end
end

--- The kernel's name and state, shown in the notebook window's winbar.
function M.winbar()
  local buf = api.nvim_win_get_buf(vim.g.statusline_winid)
  local state = states[buf]
  if not state then
    return ""
  end
  local status = kernel().status(buf)
  local name = (status and status.name) or kernel_info(state).display_name or "Python"
  local label, hl
  if not status then
    label, hl = "○ not started", "Comment"
  elseif status.state == "busy" then
    label, hl = "● busy", "DiagnosticWarn"
  elseif status.state == "idle" then
    label, hl = "○ idle", "DiagnosticOk"
  else
    label, hl = "◌ " .. tostring(status.state), "DiagnosticInfo"
  end
  return "%=%#Comment#" .. name .. "  %#" .. hl .. "#" .. label .. " %*"
end

local function redraw_winbar(buf)
  for _, win in ipairs(vim.fn.win_findbuf(buf)) do
    pcall(api.nvim__redraw, { win = win, winbar = true })
  end
end

--- Send a code cell to the kernel. Lines outside any cell get a marker first.
local function resolve(buf)
  return (buf == nil or buf == 0) and api.nvim_get_current_buf() or buf
end

function M.run_entry(buf, entry)
  buf = resolve(buf)
  local state = states[buf]
  if entry.cell and entry.cell.type ~= "code" then
    return -- markdown is already rendered; raw cells don't run
  end
  if not entry.cell then
    api.nvim_buf_set_lines(buf, entry.first, entry.first, false, { "# %%" })
    M.render(buf)
    for _, e in ipairs(scan(buf)) do
      if e.row == entry.first then
        entry = e
      end
    end
  end
  local cell = entry.cell
  local code = trimmed(entry.lines)
  cell.raw = cell.raw or { cell_type = "code", metadata = vim.empty_dict(), outputs = {}, execution_count = vim.NIL }
  cell.running = true
  cell.clear_next = true -- outputs clear when the first new one arrives, or when it finishes
  if not kernel().execute(buf, kernel_info(state), tostring(cell.uid), code) then
    cell.running = nil
  end
  M.render(buf)
  redraw_winbar(buf)
end

function M.run_all(buf, from, to)
  buf = resolve(buf)
  local list = scan(buf)
  for i = from or 1, math.min(to or #list, #list) do
    local entry = list[i]
    if entry.cell and entry.cell.type == "code" then
      M.run_entry(buf, entry)
    end
  end
end

local function take_outputs(cell)
  if cell.clear_next then
    cell.raw.outputs = {}
    cell.clear_next = false
  end
  if cell.clear_wait then
    cell.raw.outputs = {}
    cell.clear_wait = false
  end
end

--- Events from the kernel bridge (see python/tersh_kernel.py).
function M.on_kernel_event(buf, event)
  local state = states[buf]
  if not state then
    return
  end
  if event.ev == "status" or event.ev == "ready" then
    if event.ev == "ready" then
      state.nb.metadata = state.nb.metadata or vim.empty_dict()
      if event.language_info and next(event.language_info) then
        state.nb.metadata.language_info = event.language_info
      end
    end
    if event.state == "dead" then
      for _, entry in ipairs(scan(buf)) do
        if entry.cell then
          entry.cell.running = nil
        end
      end
      M.render(buf)
    end
    redraw_winbar(buf)
    return
  end
  local cell = event.id and cell_by_uid(buf, tonumber(event.id))
  if not cell then
    return
  end
  if event.ev == "output" then
    take_outputs(cell)
    local outputs = cell.raw.outputs
    local last = outputs[#outputs]
    local out = event.output
    if out.output_type == "stream" and last and last.output_type == "stream" and last.name == out.name then
      -- Consecutive stream chunks join, as Jupyter stores them.
      last.text = join_source(last.text) .. join_source(out.text)
    else
      table.insert(outputs, out)
      if event.display_id and event.display_id ~= vim.NIL then
        state.displays = state.displays or {}
        state.displays[event.display_id] = { cell = cell, output = out }
      end
    end
  elseif event.ev == "update" then
    local shown = (state.displays or {})[event.display_id]
    if shown then
      shown.output.data, shown.output.metadata = event.output.data, event.output.metadata
      cell = shown.cell
    end
  elseif event.ev == "clear" then
    if event.wait then
      cell.clear_wait = true
    else
      cell.raw.outputs = {}
    end
  elseif event.ev == "done" then
    if cell.clear_next then
      cell.raw.outputs = {}
      cell.clear_next = false
    end
    cell.running = nil
    if type(event.count) == "number" then
      cell.raw.execution_count = event.count
    end
  end
  cell.version = (cell.version or 0) + 1
  vim.bo[buf].modified = true
  schedule_render(buf, 30)
end

local HELP = {
  "Notebook command mode (Esc in Normal mode)",
  "",
  "  j / k       select the next / previous cell",
  "  Enter, i    edit the cell (markdown opens its source)",
  "  a / b       insert a code cell above / below",
  "  dd          delete the cell        z   undo",
  "  x / c       cut / copy the cell",
  "  v / V       paste below / above",
  "  m / y / r   make it markdown / code / raw",
  "  M           merge with the cell below",
  "  o           hide or show the outputs",
  "  s           save the notebook",
  "  Shift-Enter run, select below   Ctrl-Enter run",
  "  Alt-Enter   run, insert below",
  "  ii / 00     interrupt / restart the kernel",
  "  h           this help",
}

local function show_help(buf)
  local width = 0
  for _, line in ipairs(HELP) do
    width = math.max(width, vim.fn.strdisplaywidth(line))
  end
  local help = api.nvim_create_buf(false, true)
  api.nvim_buf_set_lines(help, 0, -1, false, HELP)
  vim.bo[help].bufhidden = "wipe"
  local win = api.nvim_open_win(help, true, {
    relative = "editor",
    row = math.floor((vim.o.lines - #HELP) / 2) - 1,
    col = math.floor((vim.o.columns - width) / 2) - 2,
    width = width + 2,
    height = #HELP,
    style = "minimal",
    border = "rounded",
  })
  for _, key in ipairs({ "q", "<Esc>", "h", "<CR>" }) do
    vim.keymap.set("n", key, function()
      api.nvim_win_close(win, true)
      -- Back to the notebook in command mode, where help was opened.
      if api.nvim_get_current_buf() == buf then
        M.enter_command(buf)
      end
    end, { buffer = help, nowait = true })
  end
end

--- Jupyter's three run keys. `after`: "next" selects the cell below (adding
--- one at the end), "insert" adds a cell below, nil stays.
local function run_current(buf, after)
  buf = resolve(buf)
  local index = current_index(buf)
  local entry = entries(buf)[index]
  M.run_entry(buf, entry)
  local list = entries(buf)
  if after == "next" then
    if index >= #list then
      local _, last = span(list[#list])
      insert_cell(buf, last + 1, "code", "")
    end
    select_cell(buf, index + 1)
  elseif after == "insert" then
    local _, last = span(list[index])
    select_cell(buf, insert_cell(buf, last + 1, "code", ""))
  end
end

M.run_current = run_current

local command_keys -- defined below

--- Leave command mode (back to ordinary editing).
function M.leave_command(buf)
  local state = states[buf]
  if not state or not state.command then
    return
  end
  state.command = false
  for _, map in ipairs(command_keys) do
    pcall(vim.keymap.del, "n", map[1], { buffer = buf })
  end
  M.update_current(buf)
  api.nvim_echo({ { "" } }, false, {})
end

--- Enter command mode: single keys act on whole cells, as in Jupyter.
function M.enter_command(buf)
  local state = states[buf]
  if not state then
    return
  end
  state.command = true
  for _, map in ipairs(command_keys) do
    vim.keymap.set("n", map[1], function()
      -- Each cell command is its own undo step.
      vim.bo[buf].undolevels = vim.bo[buf].undolevels
      map[2](buf)
    end, { buffer = buf, nowait = #map[1] > 1 or not ({ d = true, i = true, ["0"] = true })[map[1]], desc = "tersh notebook: " .. map[3] })
  end
  select_cell(buf, current_index(buf))
  M.update_current(buf)
  api.nvim_echo({ { "-- NOTEBOOK --", "ModeMsg" }, { "  h for keys, Enter to edit", "Comment" } }, false, {})
end

local function edit(buf)
  local entry = entries(buf)[current_index(buf)]
  M.leave_command(buf)
  if entry.cell and entry.cell.type == "markdown" then
    M.edit_markdown(buf, entry.cell)
  else
    api.nvim_win_set_cursor(0, { home_row(entry) + 1, 0 })
  end
end

command_keys = {
  { "j", function(buf) select_cell(buf, current_index(buf) + 1) end, "next cell" },
  { "<Down>", function(buf) select_cell(buf, current_index(buf) + 1) end, "next cell" },
  { "k", function(buf) select_cell(buf, current_index(buf) - 1) end, "previous cell" },
  { "<Up>", function(buf) select_cell(buf, current_index(buf) - 1) end, "previous cell" },
  { "<CR>", edit, "edit the cell" },
  { "i", edit, "edit the cell" },
  { "a", function(buf)
    local first = span(entries(buf)[current_index(buf)])
    select_cell(buf, insert_cell(buf, first, "code", ""))
  end, "insert a cell above" },
  { "b", function(buf)
    local _, last = span(entries(buf)[current_index(buf)])
    select_cell(buf, insert_cell(buf, last + 1, "code", ""))
  end, "insert a cell below" },
  { "dd", function(buf)
    local index = current_index(buf)
    delete_cell(buf, entries(buf)[index])
    select_cell(buf, index)
  end, "delete the cell" },
  { "z", function(buf)
    vim.cmd("silent! undo")
    M.render(buf)
    select_cell(buf, current_index(buf))
  end, "undo" },
  { "x", function(buf)
    local index = current_index(buf)
    copy_cell(entries(buf)[index])
    delete_cell(buf, entries(buf)[index])
    select_cell(buf, index)
  end, "cut the cell" },
  { "c", function(buf)
    copy_cell(entries(buf)[current_index(buf)])
  end, "copy the cell" },
  { "v", function(buf)
    if clipboard then
      local _, last = span(entries(buf)[current_index(buf)])
      select_cell(buf, insert_cell(buf, last + 1, clipboard.kind, clipboard.source, clipboard.raw))
    end
  end, "paste below" },
  { "V", function(buf)
    if clipboard then
      local first = span(entries(buf)[current_index(buf)])
      select_cell(buf, insert_cell(buf, first, clipboard.kind, clipboard.source, clipboard.raw))
    end
  end, "paste above" },
  { "m", function(buf) convert(buf, entries(buf)[current_index(buf)], "markdown") end, "make markdown" },
  { "y", function(buf) convert(buf, entries(buf)[current_index(buf)], "code") end, "make code" },
  { "r", function(buf) convert(buf, entries(buf)[current_index(buf)], "raw") end, "make raw" },
  { "M", function(buf)
    local index = current_index(buf)
    local list = entries(buf)
    local here, below = list[index], list[index + 1]
    if not below then
      return
    end
    local kind = here.cell and here.cell.type or "code"
    local merged = cell_source(here) .. "\n" .. cell_source(below)
    local first = span(here)
    local _, last = span(below)
    api.nvim_buf_set_lines(buf, first, last + 1, false, {})
    select_cell(buf, insert_cell(buf, first, kind, merged, here.cell and here.cell.raw))
  end, "merge with the cell below" },
  { "o", function(buf)
    local entry = entries(buf)[current_index(buf)]
    if entry.cell then
      entry.cell.hide_output = not entry.cell.hide_output
      M.render(buf)
    end
  end, "hide or show outputs" },
  { "s", function()
    vim.cmd("write")
  end, "save" },
  { "h", show_help, "help" },
  { "<S-CR>", function(buf) run_current(buf, "next") end, "run and select below" },
  { "<C-CR>", function(buf) run_current(buf) end, "run" },
  { "<M-CR>", function(buf) run_current(buf, "insert") end, "run and insert below" },
  { "ii", function(buf) kernel().interrupt(buf) end, "interrupt the kernel" },
  { "00", function(buf)
    kernel().restart(buf)
    for _, entry in ipairs(scan(buf)) do
      if entry.cell then
        entry.cell.running = nil
      end
    end
    M.render(buf)
  end, "restart the kernel" },
  { "<Esc>", function() end, "stay in command mode" },
}

-- ---- lifecycle (keymaps) ------------------------------------------------------------------

--- Keymaps and autocommands for a notebook buffer.
function M.attach(buf)
  define_highlights()
  vim.keymap.set("n", "<Esc>", function()
    vim.cmd("nohlsearch")
    M.enter_command(buf)
  end, { buffer = buf, desc = "tersh notebook: command mode" })
  -- Jupyter's run keys also work while editing; they finish in command mode.
  for key, after in pairs({ ["<S-CR>"] = "next", ["<C-CR>"] = false, ["<M-CR>"] = "insert" }) do
    vim.keymap.set({ "n", "i" }, key, function()
      vim.cmd("stopinsert")
      vim.schedule(function()
        M.enter_command(buf)
        M.run_current(buf, after or nil)
      end)
    end, { buffer = buf, desc = "tersh notebook: run the cell" })
  end
  vim.keymap.set("n", "<CR>", function()
    local cell = markdown_at(buf, api.nvim_win_get_cursor(0)[1] - 1)
    if cell then
      M.edit_markdown(buf, cell)
    else
      api.nvim_feedkeys(api.nvim_replace_termcodes("<CR>", true, false, true), "n", false)
    end
  end, { buffer = buf, desc = "tersh: edit the markdown cell" })
  api.nvim_clear_autocmds({ group = group, buffer = buf })
  api.nvim_create_autocmd({ "TextChanged", "TextChangedI" }, {
    group = group,
    buffer = buf,
    callback = function()
      schedule_render(buf)
    end,
  })
  api.nvim_create_autocmd({ "CursorMoved", "CursorMovedI" }, {
    group = group,
    buffer = buf,
    callback = function()
      M.update_current(buf)
    end,
  })
  api.nvim_create_autocmd({ "InsertEnter", "BufLeave" }, {
    group = group,
    buffer = buf,
    callback = function()
      M.leave_command(buf)
    end,
  })
  api.nvim_create_autocmd("BufWinEnter", {
    group = group,
    buffer = buf,
    callback = function()
      style_window(api.nvim_get_current_win())
      schedule_render(buf)
    end,
  })
  api.nvim_create_autocmd({ "BufWipeout", "BufDelete" }, {
    group = group,
    buffer = buf,
    once = true,
    callback = function()
      M.detach(buf)
    end,
  })
  vim.schedule(function()
    M.render(buf)
  end)
end

function M.detach(buf)
  local state = states[buf]
  if not state then
    return
  end
  states[buf] = nil
  require("tersh.kernel").stop(buf)
  if state.timer then
    state.timer:stop()
    state.timer:close()
  end
  for id in pairs(state.cards) do
    tersh().close(id)
  end
  if api.nvim_buf_is_valid(buf) then
    api.nvim_buf_clear_namespace(buf, ns, 0, -1)
    api.nvim_buf_clear_namespace(buf, ns_hl, 0, -1)
  end
end

--- Re-render every notebook (e.g. after a window resize changes card sizes).
function M.render_all()
  for buf in pairs(states) do
    M.render(buf)
  end
end

return M
