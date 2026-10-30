# tersh installer for Windows (macOS and Linux: install.sh). Usually run
# through the OS-independent `install`:
#
#   irm https://raw.githubusercontent.com/shawal-mbalire/tersh/main/install | iex
#
# Installs the latest release for the current user, with everything:
#
#   - the tersh app (per-user installer, Start menu entry)
#   - tersh-show, the card CLI
#   - the Neovim plugin (a lazy.nvim spec if you use lazy.nvim, else a package)
#   - shell completions (carapace)
#
# `tersh` and `tersh-show` are added to your user PATH. Options are environment
# variables, since `iex` takes no arguments:
#
#   $env:TERSH_VERSION = 'vX.Y.Z'   a specific release instead of the latest
#   $env:TERSH_NO_APP = 1           skip the app
#   $env:TERSH_NO_NVIM = 1          skip the Neovim plugin
#   $env:TERSH_NO_COMPLETIONS = 1   skip shell completions
#
# TERSH_NO_* choices are remembered for `tersh update`.
# No `exit` here: under `iex` it would close the user's PowerShell window.

function Install-Tersh {
  $ErrorActionPreference = 'Stop'
  $ProgressPreference = 'SilentlyContinue' # Invoke-WebRequest is very slow with a progress bar
  [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

  $repo = 'shawal-mbalire/tersh'
  $say = { param($text) Write-Host "==> $text" -ForegroundColor Blue }

  # ---- options ---------------------------------------------------------------------
  $optionsFile = Join-Path $env:APPDATA 'tersh\install-options'
  $names = 'NO_APP', 'NO_NVIM', 'NO_TMUX', 'NO_COMPLETIONS'
  $opts = @{}
  foreach ($n in $names) { $opts[$n] = [Environment]::GetEnvironmentVariable("TERSH_$n") }
  if (($names | ForEach-Object { $opts[$_] }) -join '') {
    New-Item -ItemType Directory -Force (Split-Path $optionsFile) | Out-Null
    ($names | ForEach-Object { "$_=$($opts[$_])" }) -join "`n" | Set-Content -Encoding ascii $optionsFile
  } elseif (Test-Path $optionsFile) {
    foreach ($line in Get-Content $optionsFile) {
      $key, $value = $line -split '=', 2
      if ($names -contains $key) { $opts[$key] = $value }
    }
  }

  $version = $env:TERSH_VERSION
  if (-not $version) {
    # github.com redirects /releases/latest to /releases/tag/vX.Y.Z. Unlike the
    # API, that isn't rate-limited for anonymous use (shared IPs, CI runners).
    $response = [Net.WebRequest]::Create("https://github.com/$repo/releases/latest").GetResponse()
    $version = ($response.ResponseUri.AbsoluteUri -split '/tag/')[1]
    $response.Close()
    if (-not $version) { throw "could not find the latest release of $repo" }
  }
  $number = $version.TrimStart('v')
  $base = "https://github.com/$repo/releases/download/$version"
  $tmp = Join-Path ([IO.Path]::GetTempPath()) ("tersh-install-" + [Guid]::NewGuid())
  New-Item -ItemType Directory $tmp | Out-Null
  $pathDirs = @()

  try {
    # ---- tersh-show ------------------------------------------------------------------
    & $say "Installing tersh-show $version"
    $bin = Join-Path $env:LOCALAPPDATA 'tersh\bin'
    New-Item -ItemType Directory -Force $bin | Out-Null
    $zip = Join-Path $tmp 'tersh-show.zip'
    Invoke-WebRequest "$base/tersh-show-windows-x86_64.zip" -OutFile $zip
    Expand-Archive -Force $zip $tmp
    Copy-Item -Force (Join-Path $tmp 'tersh-show.exe') $bin
    $pathDirs += $bin

    # ---- app -------------------------------------------------------------------------
    if (-not $opts.NO_APP) {
      & $say "Installing tersh $version"
      $setup = Join-Path $tmp 'tersh-setup.exe'
      Invoke-WebRequest "$base/tersh_${number}_x64-setup.exe" -OutFile $setup
      # Per-user NSIS installer; /S runs it silently.
      $process = Start-Process -FilePath $setup -ArgumentList '/S' -Wait -PassThru
      if ($process.ExitCode -ne 0) { throw "the app installer failed (exit code $($process.ExitCode))" }
      $appDir = $null
      foreach ($root in 'HKCU:', 'HKLM:') {
        $key = "$root\Software\Microsoft\Windows\CurrentVersion\Uninstall\tersh"
        if (Test-Path $key) { $appDir = (Get-ItemProperty $key).InstallLocation; if ($appDir) { break } }
      }
      if (-not $appDir) { $appDir = Join-Path $env:LOCALAPPDATA 'tersh' }
      $appDir = $appDir.Trim('"')
      if (Test-Path (Join-Path $appDir 'tersh.exe')) { $pathDirs += $appDir }
      else { Write-Warning "tersh.exe not found in $appDir; start tersh from the Start menu" }
    }

    # ---- PATH ------------------------------------------------------------------------
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $entries = @($userPath -split ';' | Where-Object { $_ })
    $added = $pathDirs | Where-Object { $entries -notcontains $_ }
    if ($added) {
      [Environment]::SetEnvironmentVariable('Path', (($entries + $added) -join ';'), 'User')
      $env:Path = (($env:Path -split ';') + $added) -join ';'
    }

    # ---- shell completions -----------------------------------------------------------
    $specs = Join-Path $env:APPDATA 'carapace\specs'
    if (-not $opts.NO_COMPLETIONS -and (Test-Path $specs)) {
      & $say 'Installing completions for carapace'
      try { Invoke-WebRequest "$base/tersh_completion_$number.yaml" -OutFile (Join-Path $specs 'tersh.yaml') }
      catch { Write-Warning "could not download completions: $_" }
    }

    # ---- Neovim plugin ---------------------------------------------------------------
    if (-not $opts.NO_NVIM -and (Get-Command nvim -ErrorAction SilentlyContinue)) {
      $nvimConfig = if ($env:XDG_CONFIG_HOME) { Join-Path $env:XDG_CONFIG_HOME 'nvim' } else { Join-Path $env:LOCALAPPDATA 'nvim' }
      $nvimData = if ($env:XDG_DATA_HOME) { Join-Path $env:XDG_DATA_HOME 'nvim-data' } else { Join-Path $env:LOCALAPPDATA 'nvim-data' }
      $pack = Join-Path $nvimData 'site\pack\tersh\start\tersh.nvim'
      $configured = (Test-Path $nvimConfig) -and (Get-ChildItem -Recurse -File $nvimConfig -ErrorAction SilentlyContinue |
          Select-String -SimpleMatch 'shawal-mbalire/tersh' -List -ErrorAction SilentlyContinue)
      if ($configured) {
        & $say 'Neovim plugin: already in your config (lazy.nvim keeps it updated)'
        Remove-Item -Recurse -Force $pack -ErrorAction SilentlyContinue
      } elseif ((Test-Path (Join-Path $nvimData 'lazy\lazy.nvim')) -and (Test-Path (Join-Path $nvimConfig 'lua\plugins'))) {
        $spec = Join-Path $nvimConfig 'lua\plugins\tersh.lua'
        & $say "Installing the Neovim plugin (lazy.nvim spec: $spec)"
        @'
-- tersh.nvim: images, video, notebooks, markdown and plots as cards in tersh.
-- Added by tersh's installer; edit freely (it isn't overwritten once present).
return {
  {
    "shawal-mbalire/tersh",
    main = "tersh",
    lazy = false, -- small, and needed at startup to preview notebooks as they open
    opts = {},
    keys = {
      { "<leader>Tp", "<cmd>TershPreview<cr>", desc = "tersh: toggle preview" },
      { "<leader>Ts", "<cmd>TershShow<cr>", desc = "tersh: show file under cursor" },
      { "<leader>Tx", "<cmd>TershClose!<cr>", desc = "tersh: close all cards" },
    },
  },
}
'@ | Set-Content -Encoding utf8 $spec
        Remove-Item -Recurse -Force $pack -ErrorAction SilentlyContinue
      } else {
        & $say "Installing the Neovim plugin (package: $pack)"
        $src = Join-Path $tmp 'src.zip'
        Invoke-WebRequest "https://github.com/$repo/archive/refs/tags/$version.zip" -OutFile $src
        Expand-Archive -Force $src $tmp
        Remove-Item -Recurse -Force $pack -ErrorAction SilentlyContinue
        New-Item -ItemType Directory -Force (Split-Path $pack) | Out-Null
        Copy-Item -Recurse (Join-Path $tmp "tersh-$number\editors\nvim") $pack
      }
    }

    & $say 'Done.'
    if (-not $opts.NO_APP) { Write-Host '    Start it from the Start menu or with: tersh   (update later with: tersh update)' }
    if ($added) { Write-Host '    Open a new terminal for the PATH change to apply.' }
  } finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
  }
}

Install-Tersh
