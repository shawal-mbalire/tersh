-- lazy.nvim reads this file from the plugin's root and merges it into
-- whatever spec installs tersh, so `{ "shawal-mbalire/tersh" }` is enough.
-- The plugin itself lives in nvim/ (plugin/tersh.lua adds it to
-- 'runtimepath'). It needs no other Neovim plugins; the notebook kernel
-- bridge declares its Python dependencies in nvim/python/tersh_kernel.py,
-- which uv installs on first use.
return {
  {
    "shawal-mbalire/tersh",
    main = "tersh",
    -- Small, and it has to be loaded at startup to render files as they open.
    lazy = false,
    opts = {},
  },
}
