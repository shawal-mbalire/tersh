-- tersh.kernel: run notebook cells on a Jupyter kernel.
--
-- A small Python bridge (python/tersh_kernel.py) talks to the kernel with
-- jupyter_client; this module starts it and exchanges JSON lines with it.
--
-- Choosing Pythons, in order:
--   bridge: config.kernel.python (if it has jupyter_client), else
--           `uv run --script`, which installs the dependencies the script
--           declares (PEP 723), else the notebook's .venv, $VIRTUAL_ENV,
--           $CONDA_PREFIX or python3, whichever has jupyter_client.
--   kernel: the notebook's kernelspec if installed, else config.kernel.argv,
--           else the first of those Pythons with ipykernel, else
--           `uv run --with ipykernel` in the notebook's directory (which uses
--           the project's dependencies when it is a uv project).

local api = vim.api
local M = {}

local script = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":p:h:h:h") .. "/python/tersh_kernel.py"

---@type table<integer, table> buf -> kernel
local kernels = {}
local module_cache = {}

local function config()
  return require("tersh").config.kernel or {}
end

local function has_module(python, module)
  local key = python .. "\0" .. module
  if module_cache[key] == nil then
    local ok, result = pcall(function()
      return vim.system({ python, "-c", "import " .. module }, { text = true }):wait(15000)
    end)
    module_cache[key] = ok and result.code == 0
  end
  return module_cache[key]
end

local windows = vim.fn.has("win32") == 1

--- A virtual environment's Python.
local function env_python(root)
  return windows and (root .. "/Scripts/python.exe") or (root .. "/bin/python")
end

--- Pythons worth trying for a notebook in `dir`, best first.
local function pythons(dir)
  local list = {}
  local function add(path)
    if path and path ~= "" and vim.fn.executable(path) == 1 and not vim.tbl_contains(list, path) then
      table.insert(list, path)
    end
  end
  add(config().python and vim.fn.expand(config().python))
  local venv = vim.fs.find(".venv", { path = dir, upward = true, type = "directory" })[1]
  if venv then
    add(env_python(venv))
  end
  if vim.env.VIRTUAL_ENV then
    add(env_python(vim.env.VIRTUAL_ENV))
  end
  if vim.env.CONDA_PREFIX then
    add(windows and (vim.env.CONDA_PREFIX .. "/python.exe") or (vim.env.CONDA_PREFIX .. "/bin/python"))
  end
  add(vim.fn.exepath("python3"))
  add(vim.fn.exepath("python"))
  return list
end

local function has_uv()
  return vim.fn.executable("uv") == 1
end

--- The command that runs the bridge, or nil and why not.
function M.bridge_command(dir)
  if config().bridge then
    return config().bridge
  end
  -- A Python the user chose comes first.
  local chosen = config().python and vim.fn.expand(config().python)
  if chosen and vim.fn.executable(chosen) == 1 and has_module(chosen, "jupyter_client") then
    return { chosen, script }
  end
  -- The bridge declares its dependencies (PEP 723, at the top of the script):
  -- with uv they are installed exactly, whatever Pythons happen to be around.
  if has_uv() then
    return { "uv", "run", "--quiet", "--script", script }
  end
  for _, python in ipairs(pythons(dir)) do
    if has_module(python, "jupyter_client") then
      return { python, script }
    end
  end
  return nil, "no Python with jupyter_client found; install uv (recommended) or pip install jupyter_client ipykernel"
end

--- The kernel command line used when the notebook's kernelspec isn't installed.
function M.kernel_argv(dir)
  if config().argv then
    return config().argv
  end
  for _, python in ipairs(pythons(dir)) do
    if has_module(python, "ipykernel") then
      return { python, "-m", "ipykernel_launcher", "-f", "{connection_file}" }
    end
  end
  if has_uv() then
    return { "uv", "run", "--quiet", "--with", "ipykernel", "python", "-m", "ipykernel_launcher", "-f", "{connection_file}" }
  end
end

local function send(kernel, request)
  if kernel.job and kernel.job > 0 then
    vim.fn.chansend(kernel.job, vim.json.encode(request) .. "\n")
  end
end

local function deliver(buf, event)
  local kernel = kernels[buf]
  if not kernel then
    return
  end
  if event.ev == "ready" then
    kernel.ready = true
    kernel.name = event.kernel
    for _, request in ipairs(kernel.queue) do
      send(kernel, request)
    end
    kernel.queue = {}
  elseif event.ev == "status" then
    kernel.state = event.state
  elseif event.ev == "error" then
    vim.notify("tersh kernel: " .. tostring(event.message), vim.log.levels.ERROR)
  end
  require("tersh.notebook").on_kernel_event(buf, event)
end

--- The kernel for a notebook buffer, starting it if needed.
function M.get(buf, info)
  local kernel = kernels[buf]
  if kernel and kernel.job then
    return kernel
  end
  local dir = vim.fn.fnamemodify(api.nvim_buf_get_name(buf), ":p:h")
  local cmd, why = M.bridge_command(dir)
  if not cmd then
    vim.notify("tersh: " .. why, vim.log.levels.ERROR)
    return nil
  end
  kernel = { queue = {}, ready = false, state = "starting", stderr = {}, name = info.display_name or info.name or "kernel" }
  kernels[buf] = kernel
  local partial = ""
  kernel.job = vim.fn.jobstart(cmd, {
    cwd = dir,
    on_stdout = function(_, data)
      data[1] = partial .. data[1]
      partial = table.remove(data)
      for _, line in ipairs(data) do
        if line ~= "" then
          local ok, event = pcall(vim.json.decode, line)
          if ok and type(event) == "table" then
            vim.schedule(function()
              deliver(buf, event)
            end)
          end
        end
      end
    end,
    on_stderr = function(_, data)
      for _, line in ipairs(data) do
        if line ~= "" then
          table.insert(kernel.stderr, line)
          if #kernel.stderr > 20 then
            table.remove(kernel.stderr, 1)
          end
        end
      end
    end,
    on_exit = function(_, code)
      vim.schedule(function()
        if kernels[buf] ~= kernel then
          return
        end
        kernels[buf] = nil
        if code ~= 0 and not kernel.stopping then
          local detail = table.concat(kernel.stderr, "\n")
          vim.notify("tersh: the kernel bridge exited (" .. code .. ")\n" .. detail, vim.log.levels.ERROR)
        end
        require("tersh.notebook").on_kernel_event(buf, { ev = "status", state = "dead" })
      end)
    end,
  })
  if kernel.job <= 0 then
    kernels[buf] = nil
    vim.notify("tersh: could not start the kernel bridge: " .. table.concat(cmd, " "), vim.log.levels.ERROR)
    return nil
  end
  send(kernel, {
    op = "start",
    kernel = info.name,
    display_name = info.display_name,
    argv = M.kernel_argv(dir),
    cwd = dir,
  })
  return kernel
end

--- Run code; events for it come back with `id`.
function M.execute(buf, info, id, code)
  local kernel = M.get(buf, info)
  if not kernel then
    return false
  end
  local request = { op = "execute", id = id, code = code }
  if kernel.ready then
    send(kernel, request)
  else
    table.insert(kernel.queue, request)
  end
  return true
end

function M.interrupt(buf)
  local kernel = kernels[buf]
  if kernel then
    kernel.queue = {}
    send(kernel, { op = "interrupt" })
  end
end

function M.restart(buf)
  local kernel = kernels[buf]
  if kernel then
    kernel.queue = {}
    send(kernel, { op = "restart" })
  end
end

function M.stop(buf)
  local kernel = kernels[buf]
  if not kernel then
    return
  end
  kernel.stopping = true
  send(kernel, { op = "shutdown" })
  local job = kernel.job
  vim.defer_fn(function()
    pcall(vim.fn.jobstop, job)
  end, 3000)
  kernels[buf] = nil
end

function M.status(buf)
  local kernel = kernels[buf]
  return kernel and { name = kernel.name, state = kernel.state, ready = kernel.ready } or nil
end

function M.stop_all()
  for buf in pairs(kernels) do
    M.stop(buf)
  end
end

return M
