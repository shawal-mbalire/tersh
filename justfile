# tersh task runner. `just` lists the recipes.
#
#   backend/   Rust workspace (domain, application, adapters, desktop, show)
#   frontend/  Angular page and @tersh/cards

set shell := ["bash", "-euo", "pipefail", "-c"]

root := justfile_directory()
backend := root / "backend"
frontend := root / "frontend"
# bun runs all frontend tooling; pnpm installs the pinned version (frontend/package.json devEngines).
bun := frontend / "node_modules/.bin/bun"
prettier := frontend / "node_modules/prettier/bin/prettier.cjs"
# The frontend's package manager; override with e.g. `PNPM="npx pnpm@12.18.0" just setup`.
pnpm := env("PNPM", "pnpm")

# Tauri's app and page live in sibling folders rather than nested.
export TAURI_APP_PATH := backend / "desktop"
export TAURI_FRONTEND_PATH := frontend

# List recipes.
default:
    @just --list --unsorted

# ---- setup -------------------------------------------------------------------

# Install frontend dependencies (pnpm, from the lockfile) and fetch Rust crates.
setup:
    cd {{frontend}} && {{pnpm}} install --frozen-lockfile
    cd {{backend}} && cargo fetch

# Install the WebKitGTK/GTK packages the desktop app needs on Linux (sudo, or `--toolbox`).
setup-linux *args:
    {{root}}/scripts/setup-linux.sh {{args}}

# ---- develop -----------------------------------------------------------------

# Run the desktop app with the page served live.
dev:
    cd {{frontend}} && {{pnpm}} run tauri dev

# Serve only the page (http://localhost:1420).
frontend-dev:
    cd {{frontend}} && {{pnpm}} run dev

# Build the desktop app for release (bundles in backend/target/release/bundle).
build:
    cd {{frontend}} && {{pnpm}} run tauri build

# Build only the page into frontend/dist.
frontend-build:
    cd {{frontend}} && {{pnpm}} run build

# Serve the @tersh/cards example page, rendering the demo notebook.
cards-example:
    cd {{frontend}} && {{bun}} --bun x vite packages/cards/example

# Build the Rust crates that need no WebKitGTK.
backend-build:
    cd {{backend}} && cargo build

# Run tersh-show with arguments, e.g. `just show --help`.
show *args:
    cd {{backend}} && cargo run -q -p tersh-show -- {{args}}

# ---- check -------------------------------------------------------------------

# Everything CI runs that works without WebKitGTK.
check: typecheck test lint

# All tests: page, Rust crates, Neovim plugin.
test: frontend-test backend-test nvim-test

# Frontend unit and architecture tests (bun test, happy-dom).
frontend-test:
    cd {{frontend}} && {{pnpm}} test

# Rust tests (all crates but the desktop one).
backend-test:
    cd {{backend}} && cargo test

# The desktop crate's tests (needs WebKitGTK).
desktop-test:
    cd {{backend}} && cargo test -p tersh

# The Neovim plugin's tests.
nvim-test:
    nvim --headless -u NONE -l {{root}}/nvim/tests/run.lua

# Type-check the page, Angular templates included.
typecheck:
    cd {{frontend}} && {{pnpm}} run typecheck

# Prettier, clippy and the third-party inventory.
lint: format-check clippy third-party-check

# Clippy on the crates that need no WebKitGTK.
clippy:
    cd {{backend}} && cargo clippy --all-targets -- -D warnings

# Clippy on the desktop crate (needs WebKitGTK and a built page).
clippy-desktop: frontend-build
    cd {{backend}} && cargo clippy -p tersh --all-targets -- -D warnings

# Format the page, docs and config with Prettier.
format:
    cd {{root}} && {{bun}} --bun {{prettier}} --write .

# Check formatting without changing files.
format-check:
    cd {{root}} && {{bun}} --bun {{prettier}} --check .

# ---- maintenance -------------------------------------------------------------

# Remove build output (Rust target, page dist, caches). Keeps node_modules.
clean:
    cd {{backend}} && cargo clean
    rm -rf {{frontend}}/dist {{frontend}}/packages/cards/dist {{frontend}}/.angular {{frontend}}/node_modules/.vite {{frontend}}/node_modules/.cache
    rm -rf {{backend}}/desktop/gen
    find {{root}} -name '*.tsbuildinfo' -not -path '*/node_modules/*' -delete

# `clean`, plus node_modules: back to a fresh checkout's state.
distclean: clean
    rm -rf {{frontend}}/node_modules

# Drop Rust build artifacts older than 14 days (needs cargo-sweep), else show target/'s size.
sweep:
    #!/usr/bin/env bash
    set -euo pipefail
    cd {{backend}}
    if command -v cargo-sweep >/dev/null; then cargo sweep --time 14; else echo "cargo-sweep not installed (cargo install cargo-sweep)"; fi
    du -sh target 2>/dev/null || true

# Show disk used by build output and dependencies.
disk:
    @du -sh {{backend}}/target {{frontend}}/node_modules {{frontend}}/dist 2>/dev/null || true

# Fetch, prune deleted remote branches and delete local branches merged into main.
prune-branches:
    git fetch --prune
    git branch --merged main | grep -vE '^\*|^\s*main$' | xargs -r git branch -d

# Show dependencies with newer versions.
outdated:
    -cd {{frontend}} && {{pnpm}} outdated
    -cd {{backend}} && if command -v cargo-outdated >/dev/null; then cargo outdated -R; else cargo update --dry-run --verbose 2>&1 | grep -E 'Updating|Unchanged' || true; fi

# Update dependencies within their semver ranges (lockfiles only).
update-deps:
    cd {{frontend}} && {{pnpm}} update
    cd {{backend}} && cargo update
    just third-party

# Look for known vulnerabilities (cargo-audit if installed).
audit:
    cd {{frontend}} && {{pnpm}} audit
    -cd {{backend}} && if command -v cargo-audit >/dev/null; then cargo audit; else echo "cargo-audit not installed (cargo install cargo-audit)"; fi

# Regenerate the dependency inventory in THIRD-PARTY.txt.
third-party:
    python3 {{root}}/scripts/third-party.py

# Fail if THIRD-PARTY.txt doesn't match the lockfiles.
third-party-check:
    python3 {{root}}/scripts/third-party.py --check

# Set the version everywhere it is recorded, e.g. `just version 0.7.0`.
version new:
    sed -i -E '0,/^version = ".*"/s//version = "{{new}}"/' {{backend}}/Cargo.toml
    sed -i -E 's/"version": "[^"]*"/"version": "{{new}}"/' {{backend}}/desktop/tauri.conf.json
    cd {{frontend}} && {{pnpm}} pkg set version={{new}}
    cd {{frontend}}/packages/cards && {{pnpm}} pkg set version={{new}}
    cd {{backend}} && cargo update --workspace -q
    just third-party
    @echo "version {{new}}: review with git diff, then commit 'chore: release {{new}}'"

# Periodic tidy-up: prune branches, sweep stale build output, list loose ends.
tidy: prune-branches sweep
    @echo "--- uncommitted"; git status --short
    @echo "--- stashes"; git stash list
