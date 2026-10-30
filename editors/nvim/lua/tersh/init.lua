-- tersh.nvim: show rich cards (images, markdown, plots, notebooks, HTML) in
-- the tersh terminal, glued to buffer lines or windows.
--
-- A card attached to a line reserves blank virtual lines below it and the
-- card is drawn over them; it follows the line as you scroll, edit, resize or
-- switch windows. A card attached to a window covers that window.

local transport = require("tersh.transport")

local api = vim.api
local M = {}

M.config = {
  --- File extensions to preview automatically when opened.
  auto_preview = { "ipynb" },
  preview = {
    --- "vertical" or "horizontal" split, or "tab".
    split = "vertical",
    --- Width (vertical) or height (horizontal) of the preview window; < 1 is a fraction.
    size = 0.5,
  },
  --- Rows reserved for :TershShow cards when no count is given (text kinds size to their content).
  rows = 12,
  --- Milliseconds to wait after an edit before refreshing previews.
  debounce = 150,
  --- Send messages even if tersh isn't detected (e.g. over ssh with OSC passthrough).
  force = false,
}

local ns = api.nvim_create_namespace("tersh")
local group = api.nvim_create_augroup("tersh", { clear = true })

---@class TershCard
---@field id string
---@field kind string
---@field buf? integer  line cards: buffer holding the extmark
---@field mark? integer line cards: extmark id
---@field rows? integer line cards: reserved rows
---@field win? integer  window cards: covered window
---@field last? string  last placement sent, to skip redundant moves
local cards = {}
local counter = 0
local suspended = false

-- ---- terminal geometry -----------------------------------------------------

--- Offset of Neovim's screen inside the terminal: non-zero inside a tmux pane.
local offset = { row = 0, col = 0, visible = true }

local function refresh_tmux()
  if not vim.env.TMUX or not vim.env.TMUX_PANE then
    offset = { row = 0, col = 0, visible = true }
    return
  end
  local format = "#{pane_top} #{pane_left} #{status} #{status-position} #{window_active} #{window_zoomed_flag} #{pane_active}"
  local out = vim.fn.system({ "tmux", "display-message", "-p", "-t", vim.env.TMUX_PANE, format })
  local top, left, status, position, window_active, zoomed, pane_active =
    out:match("(%d+) (%d+) (%S+) (%S+) (%d) (%d) (%d)")
  if not top then
    return
  end
  local status_lines = status == "off" and 0 or (tonumber(status) or 1)
  offset = {
    row = tonumber(top) + (position == "top" and status_lines or 0),
    col = tonumber(left),
    -- Cards of a pane in a background tmux window (or hidden by a zoomed pane) must not show.
    visible = window_active == "1" and (zoomed == "0" or pane_active == "1"),
  }
end

-- ---- placement ---------------------------------------------------------------

local function is_float(win)
  return api.nvim_win_get_config(win).relative ~= ""
end

--- The window a line card should follow: the current one if it shows the buffer, else the first.
local function window_for(buf)
  local current = api.nvim_get_current_win()
  if api.nvim_win_get_buf(current) == buf and not is_float(current) then
    return current
  end
  for _, win in ipairs(api.nvim_tabpage_list_wins(0)) do
    if api.nvim_win_get_buf(win) == buf and not is_float(win) then
      return win
    end
  end
end

--- Screen box of a window's text area (0-based), or nil.
local function text_area(win)
  local info = vim.fn.getwininfo(win)[1]
  if not info then
    return nil
  end
  return {
    row = info.winrow - 1 + (info.winbar or 0),
    col = info.wincol - 1 + info.textoff,
    rows = info.height,
    cols = info.width - info.textoff,
    topline = info.topline,
    win = win,
  }
end

--- Where a line card goes: { row, col, rows, cols } in screen cells, or nil when out of view.
local function place_line(card)
  local mark = api.nvim_buf_get_extmark_by_id(card.buf, ns, card.mark, { details = true })
  if not mark[1] or (mark[3] and mark[3].invalid) then
    return nil
  end
  local line = mark[1] -- 0-based
  local win = window_for(card.buf)
  if not win then
    return nil
  end
  local area = text_area(win)
  local top = area.topline - 1
  if line < top then
    return nil
  end
  local folded = api.nvim_win_call(win, function()
    return vim.fn.foldclosed(line + 1) ~= -1
  end)
  if folded then
    return nil
  end
  -- Screen rows taken by everything above the anchor line, plus the line itself (wrapped).
  local above = 0
  if line > top then
    above = api.nvim_win_text_height(win, { start_row = top, end_row = line - 1 }).all
  end
  local own = api.nvim_win_text_height(win, { start_row = line, end_row = line })
  local topfill = api.nvim_win_call(win, function()
    return vim.fn.winsaveview().topfill or 0
  end)
  local row = above + topfill + (own.all - own.fill)
  local visible = math.min(card.rows, area.rows - row)
  if visible <= 0 then
    return nil
  end
  return { row = area.row + row, col = area.col, rows = visible, cols = area.cols }
end

local function place_window(card)
  local win = card.win
  if not api.nvim_win_is_valid(win) or api.nvim_win_get_tabpage(win) ~= api.nvim_get_current_tabpage() then
    return nil
  end
  local info = vim.fn.getwininfo(win)[1]
  return {
    row = info.winrow - 1 + (info.winbar or 0),
    col = info.wincol - 1,
    rows = info.height,
    cols = info.width,
  }
end

local function anchor_for(card)
  if suspended or not offset.visible then
    return nil
  end
  local box = card.win and place_window(card) or (card.mark and place_line(card))
  if not box then
    return nil
  end
  return { mode = "cell", row = box.row + offset.row, col = box.col + offset.col, rows = box.rows, cols = box.cols }
end

local function sync_card(card, force)
  local anchor = anchor_for(card)
  local key = anchor and string.format("%d,%d,%d,%d", anchor.row, anchor.col, anchor.rows, anchor.cols) or "hidden"
  if key == card.last and not force then
    return
  end
  local was_hidden = card.last == "hidden"
  card.last = key
  if anchor then
    transport.send({ op = "move", id = card.id, anchor = anchor, hidden = false })
  elseif not was_hidden then
    transport.send({ op = "move", id = card.id, hidden = true })
  end
end

--- Reposition every card. Cheap when nothing moved.
function M.sync(force)
  for _, card in pairs(cards) do
    sync_card(card, force)
  end
end

local sync_pending = false
local function schedule_sync()
  if sync_pending then
    return
  end
  sync_pending = true
  vim.schedule(function()
    sync_pending = false
    M.sync()
  end)
end

-- ---- cards ---------------------------------------------------------------------

local function enabled()
  if M.config.force or transport.available() then
    return true
  end
  vim.notify_once("tersh: not running inside tersh ($TERSH_SOCKET is not set)", vim.log.levels.WARN)
  return false
end

local function new_id(prefix)
  counter = counter + 1
  return string.format("nvim-%d-%s-%d", vim.fn.getpid(), prefix, counter)
end

local function show(card, message)
  cards[card.id] = card
  local anchor = anchor_for(card)
  card.last = anchor and string.format("%d,%d,%d,%d", anchor.row, anchor.col, anchor.rows, anchor.cols) or "hidden"
  message.op = "show"
  message.id = card.id
  message.anchor = anchor or { mode = "cell", row = 0, col = 0, rows = 1, cols = 1 }
  message.hidden = anchor == nil
  transport.send(message)
  return card.id
end

---@class TershAttachOpts
---@field kind string             card kind: markdown, image, svg, html, plotly, notebook, output, json, text
---@field data any                card data (for image: a data: URI or base64 string)
---@field title? string
---@field buf? integer            default: current buffer
---@field line? integer           1-based line the card goes below; default: cursor line
---@field rows? integer           rows to reserve; default config.rows
---@field id? string              reuse an id to replace an existing card
---@field chrome? boolean         show the card's title bar (default true)
---@field mime? string            image MIME type when data is bare base64

--- Show a card below a buffer line. Returns its id.
---@param opts TershAttachOpts
function M.attach(opts)
  if not enabled() then
    return nil
  end
  local buf = (opts.buf == nil or opts.buf == 0) and api.nvim_get_current_buf() or opts.buf
  local line = (opts.line or api.nvim_win_get_cursor(0)[1]) - 1
  local rows = math.max(1, opts.rows or M.config.rows)
  local id = opts.id or new_id("line")
  if cards[id] then
    M.close(id)
  end
  local virt_lines = {}
  for i = 1, rows do
    virt_lines[i] = { { "", "" } }
  end
  local mark = api.nvim_buf_set_extmark(buf, ns, line, 0, {
    virt_lines = virt_lines,
    invalidate = true,
    undo_restore = false,
  })
  -- The reserved lines only exist after a redraw; place the card once they do.
  local card = { id = id, kind = opts.kind, buf = buf, mark = mark, rows = rows }
  cards[id] = card
  vim.schedule(function()
    if cards[id] == card then
      show(card, { kind = opts.kind, data = opts.data, title = opts.title, chrome = opts.chrome, mime = opts.mime })
    end
  end)
  return id
end

--- Show a card covering a window (e.g. a preview split). Returns its id.
function M.attach_window(win, opts)
  if not enabled() then
    return nil
  end
  local id = opts.id or new_id("win")
  if cards[id] then
    M.close(id)
  end
  local card = { id = id, kind = opts.kind, win = win == 0 and api.nvim_get_current_win() or win }
  return show(card, { kind = opts.kind, data = opts.data, title = opts.title, chrome = opts.chrome, mime = opts.mime })
end

--- True when cards can be shown: running inside tersh (or `force` is set).
function M.available()
  return M.config.force or transport.available()
end

--- Send a raw protocol message (see docs/protocol.md), e.g. a float card:
---   tersh.send({ kind = "markdown", data = "# hi", anchor = { mode = "float", top = 2, left = 4, width = 40 } })
--- `callback(reply)` gets tersh's `{ ok, error? }` reply when the socket is used.
--- Cards sent this way are not tracked: they don't follow buffer lines.
function M.send(message, callback)
  transport.send(message, callback)
end

--- Replace a card's data (and optionally title).
function M.update(id, data, title)
  if cards[id] then
    transport.send({ op = "update", id = id, data = data, title = title })
  end
end

function M.close(id)
  local card = cards[id]
  if not card then
    return
  end
  cards[id] = nil
  if card.mark and api.nvim_buf_is_valid(card.buf) then
    pcall(api.nvim_buf_del_extmark, card.buf, ns, card.mark)
  end
  transport.send({ op = "close", id = id })
end

function M.close_all()
  for id in pairs(cards) do
    M.close(id)
  end
end

--- Ids of line cards anchored on a buffer line (1-based).
function M.cards_at(buf, line)
  local ids = {}
  for id, card in pairs(cards) do
    if card.buf == buf and card.mark then
      local pos = api.nvim_buf_get_extmark_by_id(buf, ns, card.mark, {})
      if pos[1] == line - 1 then
        table.insert(ids, id)
      end
    end
  end
  return ids
end

function M.list()
  return vim.tbl_keys(cards)
end

-- ---- files -----------------------------------------------------------------------

local image_mimes = {
  png = "image/png",
  jpg = "image/jpeg",
  jpeg = "image/jpeg",
  gif = "image/gif",
  webp = "image/webp",
  bmp = "image/bmp",
  avif = "image/avif",
}

local video_mimes = {
  webm = "video/webm",
  mp4 = "video/mp4",
  m4v = "video/mp4",
  ogv = "video/ogg",
  mov = "video/quicktime",
}

local function is_plotly(text)
  local ok, value = pcall(vim.json.decode, text)
  return ok and type(value) == "table" and type(value.data) == "table" and value.layout ~= nil
end

--- Card kind for a file name and its content.
function M.guess_kind(name, content)
  local ext = (name:match("%.([%w]+)$") or ""):lower()
  if image_mimes[ext] then
    return "image", image_mimes[ext]
  end
  if video_mimes[ext] then
    return "video", video_mimes[ext]
  end
  local by_ext = { svg = "svg", md = "markdown", markdown = "markdown", html = "html", htm = "html", ipynb = "notebook" }
  if by_ext[ext] then
    return by_ext[ext]
  end
  if ext == "json" then
    return is_plotly(content) and "plotly" or "json"
  end
  return "text"
end

--- Card data for a file: images become data URIs, everything else is sent as text
--- (JSON kinds are parsed by the terminal, which keeps `{}` and `[]` distinct).
local function file_card(path)
  local file = io.open(path, "rb")
  if not file then
    return nil, "cannot read " .. path
  end
  local content = file:read("*a")
  file:close()
  local kind, mime = M.guess_kind(path, content)
  if kind == "image" or kind == "video" then
    return { kind = kind, data = "data:" .. mime .. ";base64," .. vim.base64.encode(content) }
  end
  return { kind = kind, data = content }
end

local function default_rows(kind, data)
  if (kind == "markdown" or kind == "text" or kind == "json") and type(data) == "string" then
    local _, lines = data:gsub("\n", "")
    return math.max(3, math.min(lines + 3, math.floor(vim.o.lines * 0.6)))
  end
  return M.config.rows
end

--- Show a file below the cursor line.
---@param path string
---@param rows? integer
function M.show_file(path, rows)
  path = vim.fn.fnamemodify(vim.fn.expand(path), ":p")
  local card, err = file_card(path)
  if not card then
    vim.notify("tersh: " .. err, vim.log.levels.ERROR)
    return
  end
  return M.attach({
    kind = card.kind,
    data = card.data,
    title = vim.fn.fnamemodify(path, ":t"),
    rows = rows or default_rows(card.kind, card.data),
  })
end

-- ---- live preview ------------------------------------------------------------------

local previews = {} -- source buf -> { card = id, win = preview window, timer }

local function buffer_card(buf)
  local name = api.nvim_buf_get_name(buf)
  local text = table.concat(api.nvim_buf_get_lines(buf, 0, -1, false), "\n")
  local kind = M.guess_kind(name, text)
  if kind == "text" then
    local by_ft = { markdown = "markdown", html = "html", svg = "svg", json = "json" }
    kind = by_ft[vim.bo[buf].filetype] or "text"
  end
  if kind == "image" then
    return nil -- binary buffers aren't previewed; use :TershShow on the file
  end
  if (kind == "notebook" or kind == "plotly" or kind == "json") and not pcall(vim.json.decode, text) then
    return { kind = kind, invalid = true }
  end
  return { kind = kind, data = text }
end

local function close_preview(buf)
  local preview = previews[buf]
  if not preview then
    return
  end
  previews[buf] = nil
  if preview.timer then
    preview.timer:stop()
    preview.timer:close()
  end
  M.close(preview.card)
  if api.nvim_win_is_valid(preview.win) then
    pcall(api.nvim_win_close, preview.win, true)
  end
end

local function open_split()
  local opts = M.config.preview
  local source = api.nvim_get_current_win()
  if opts.split == "tab" then
    vim.cmd("tab split")
  else
    local vertical = opts.split ~= "horizontal"
    local total = vertical and vim.o.columns or vim.o.lines
    local size = opts.size < 1 and math.floor(total * opts.size) or opts.size
    vim.cmd(string.format("botright %s%dnew", vertical and "vertical " or "", size))
  end
  local win = api.nvim_get_current_win()
  local scratch = api.nvim_get_current_buf()
  if opts.split == "tab" then
    scratch = api.nvim_create_buf(false, true)
    api.nvim_win_set_buf(win, scratch)
  end
  vim.bo[scratch].buftype = "nofile"
  vim.bo[scratch].bufhidden = "wipe"
  vim.bo[scratch].swapfile = false
  vim.bo[scratch].filetype = "tersh-preview"
  for option, value in pairs({ number = false, relativenumber = false, signcolumn = "no", foldcolumn = "0", list = false }) do
    vim.wo[win][option] = value
  end
  if opts.split ~= "tab" then
    api.nvim_set_current_win(source)
  end
  return win, scratch
end

--- Toggle a live preview of a buffer (markdown, HTML, SVG, notebooks, Plotly JSON).
function M.preview(buf)
  buf = (buf == nil or buf == 0) and api.nvim_get_current_buf() or buf
  if previews[buf] then
    close_preview(buf)
    return
  end
  if not enabled() then
    return
  end
  local content = buffer_card(buf)
  if not content then
    vim.notify("tersh: nothing to preview in this buffer", vim.log.levels.WARN)
    return
  end
  local win, scratch = open_split()
  local title = vim.fn.fnamemodify(api.nvim_buf_get_name(buf), ":t")
  local id = M.attach_window(win, {
    kind = content.kind,
    data = content.invalid and "" or content.data,
    title = title ~= "" and title or content.kind,
  })
  local preview = { card = id, win = win, kind = content.kind }
  previews[buf] = preview

  local function refresh()
    local next = buffer_card(buf)
    -- Keep the last good render while JSON is half-edited.
    if next and not next.invalid and previews[buf] == preview then
      M.update(id, next.data)
    end
  end
  preview.timer = (vim.uv or vim.loop).new_timer()
  api.nvim_create_autocmd({ "TextChanged", "TextChangedI", "BufWritePost" }, {
    group = group,
    buffer = buf,
    callback = function()
      if previews[buf] ~= preview then
        return true
      end
      preview.timer:stop()
      preview.timer:start(M.config.debounce, 0, vim.schedule_wrap(refresh))
    end,
  })
  api.nvim_create_autocmd({ "BufWipeout", "BufDelete" }, {
    group = group,
    buffer = buf,
    once = true,
    callback = function()
      close_preview(buf)
    end,
  })
  api.nvim_create_autocmd("BufWipeout", {
    group = group,
    buffer = scratch,
    once = true,
    callback = function()
      if previews[buf] == preview then
        vim.schedule(function()
          close_preview(buf)
        end)
      end
    end,
  })
end

-- ---- setup -------------------------------------------------------------------------

local function setup_autocmds()
  api.nvim_clear_autocmds({ group = group })
  api.nvim_create_autocmd(
    { "WinScrolled", "WinResized", "WinNew", "WinClosed", "BufWinEnter", "BufWinLeave", "TabEnter", "TextChanged", "TextChangedI", "CursorMoved" },
    { group = group, callback = schedule_sync }
  )
  api.nvim_create_autocmd({ "VimResized", "FocusGained", "FocusLost" }, {
    group = group,
    callback = function()
      refresh_tmux()
      M.sync(true)
    end,
  })
  api.nvim_create_autocmd({ "BufWipeout", "BufDelete" }, {
    group = group,
    callback = function(ev)
      for id, card in pairs(cards) do
        if card.buf == ev.buf then
          M.close(id)
        end
      end
    end,
  })
  api.nvim_create_autocmd("VimSuspend", {
    group = group,
    callback = function()
      suspended = true
      M.sync(true)
    end,
  })
  api.nvim_create_autocmd("VimResume", {
    group = group,
    callback = function()
      suspended = false
      M.sync(true)
    end,
  })
  api.nvim_create_autocmd("VimLeavePre", {
    group = group,
    callback = function()
      M.close_all()
    end,
  })
  if #M.config.auto_preview > 0 then
    local patterns = vim.tbl_map(function(ext)
      return "*." .. ext
    end, M.config.auto_preview)
    api.nvim_create_autocmd("BufWinEnter", {
      group = group,
      pattern = patterns,
      callback = function(ev)
        if not previews[ev.buf] and (M.config.force or transport.available()) then
          vim.schedule(function()
            M.preview(ev.buf)
          end)
        end
      end,
    })
  end
end

function M.setup(opts)
  opts = opts or {}
  M.config = vim.tbl_deep_extend("force", M.config, opts)
  -- Lists replace rather than merge, so `auto_preview = {}` turns it off.
  if opts.auto_preview then
    M.config.auto_preview = opts.auto_preview
  end
  refresh_tmux()
  setup_autocmds()
end

-- Work without an explicit setup() call.
refresh_tmux()
setup_autocmds()

return M
