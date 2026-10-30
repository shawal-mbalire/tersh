-- Lets plugin managers install tersh.nvim straight from the tersh repository,
-- e.g. with lazy.nvim: { "shawal-mbalire/tersh", main = "tersh", lazy = false, opts = {} }.
-- The plugin itself lives in editors/nvim; this adds it to 'runtimepath'.
local dir = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":p:h:h") .. "/editors/nvim"
if not vim.tbl_contains(vim.opt.runtimepath:get(), dir) then
  vim.opt.runtimepath:append(dir)
end
dofile(dir .. "/plugin/tersh.lua")
