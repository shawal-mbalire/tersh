-- Sends rich-protocol messages to tersh: over $TERSH_SOCKET when it exists
-- (fast, no size worries), otherwise as OSC 7701 escape sequences written to
-- the terminal (wrapped for tmux passthrough when inside tmux).

local uv = vim.uv or vim.loop

local M = {}

local pipe = nil
local connected = false
local queue = {} -- lines waiting for the connection
local replies = {} -- callbacks (or false) for messages awaiting their reply line
local partial = ""

function M.socket_path()
  local path = vim.env.TERSH_SOCKET
  if path and path ~= "" and uv.fs_stat(path) then
    return path
  end
end

--- Whether we appear to be running inside tersh.
function M.available()
  return M.socket_path() ~= nil or vim.env.TERM_PROGRAM == "tersh"
end

local function reset()
  if pipe and not pipe:is_closing() then
    pipe:close()
  end
  pipe, connected, partial = nil, false, ""
  replies = {}
end

local function on_line(line)
  local callback = table.remove(replies, 1)
  local ok, reply = pcall(vim.json.decode, line)
  if not ok then
    return
  end
  vim.schedule(function()
    if reply.ok == false then
      vim.notify("tersh: " .. tostring(reply.error), vim.log.levels.WARN)
    end
    if callback then
      callback(reply)
    end
  end)
end

local function write_terminal(data)
  if vim.api.nvim_ui_send then
    vim.api.nvim_ui_send(data)
  else
    vim.fn.chansend(vim.v.stderr, data)
  end
end

local function send_osc(message)
  local payload = vim.base64.encode(vim.json.encode(message))
  local sequence = "\27]7701;" .. payload .. "\27\\"
  if vim.env.TMUX then
    -- Needs `set -g allow-passthrough on` in tmux.conf.
    sequence = "\27Ptmux;" .. sequence:gsub("\27", "\27\27") .. "\27\\"
  end
  write_terminal(sequence)
end

local function connect(path)
  pipe = uv.new_pipe(false)
  pipe:connect(path, function(err)
    if err then
      local pending = queue
      queue = {}
      reset()
      -- Fall back to escape sequences for what was queued.
      vim.schedule(function()
        for _, line in ipairs(pending) do
          send_osc(vim.json.decode(line))
        end
      end)
      return
    end
    connected = true
    pipe:read_start(function(read_err, chunk)
      if read_err or not chunk then
        reset()
        return
      end
      partial = partial .. chunk
      while true do
        local newline = partial:find("\n", 1, true)
        if not newline then
          break
        end
        on_line(partial:sub(1, newline - 1))
        partial = partial:sub(newline + 1)
      end
    end)
    for _, line in ipairs(queue) do
      pipe:write(line)
    end
    queue = {}
  end)
end

--- Send a message. `callback(reply)` runs with the socket's reply, if the socket is used.
function M.send(message, callback)
  local path = M.socket_path()
  if not path then
    send_osc(message)
    return
  end
  local line = vim.json.encode(message) .. "\n"
  table.insert(replies, callback or false)
  if connected then
    pipe:write(line)
  else
    table.insert(queue, line)
    if not pipe then
      connect(path)
    end
  end
end

function M.close()
  reset()
end

return M
