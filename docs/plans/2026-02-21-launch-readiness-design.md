# v0.4.0 Launch Readiness Design

## Goal

Make Shabka discoverable, installable, and immediately valuable for new users. Currently the product is feature-complete but has no distribution pipeline, no presence on package registries, and the onboarding requires too many manual steps.

## Workstreams

### 1. CI/CD Pipeline (GitHub Actions)

**`ci.yml`** — every push/PR:
- `cargo clippy --workspace --no-default-features -- -D warnings`
- `cargo test` for all crates (core, cli, hooks, mcp, web)

**`release.yml`** — on `v*` tags:
- Cross-compile via `cross` or `cargo-zigbuild` for:
  - `x86_64-unknown-linux-gnu`
  - `x86_64-apple-darwin`
  - `aarch64-apple-darwin`
  - `x86_64-pc-windows-msvc`
- Produce tarballs: `shabka-{version}-{target}.tar.gz` containing `shabka` + `shabka-mcp` binaries
- Create GitHub Release with SHA256 checksums and release notes from CHANGELOG

### 2. Install Script

`install.sh` at repo root:
- Detects OS + arch
- Downloads correct tarball from latest GitHub Release
- Extracts to `~/.shabka/bin/` (or `~/.cargo/bin/` if it exists)
- Prints PATH hint if needed
- Usage: `curl -sSf https://raw.githubusercontent.com/mehdig-dev/shabka/main/install.sh | sh`

### 3. crates.io Publish

- Publish `shabka-core`, `shabka-cli`, `shabka-mcp`
- Core published first (dependency), then cli and mcp
- `cargo install shabka-cli shabka-mcp` becomes the Rust-user install path
- Add `publish.sh` helper for ordered publishing

### 4. `shabka demo` Command

New CLI subcommand that seeds 10-15 realistic sample memories:
- Covers all 9 memory kinds
- Includes 4-5 relations (fixes, caused_by, related, contradicts)
- Realistic content (auth patterns, database decisions, error fixes, coding lessons)
- Skips if memories already exist (idempotent)
- Lets new users immediately see value in TUI/search/web without real data

### 5. MCP Registry Submissions

Manual submissions to:
- mcp.so — primary MCP directory
- Smithery — secondary registry
- Prepare metadata: name, description, install command, tool list, screenshots

### 6. README Refresh

- Asciinema/GIF recording of TUI session (demo data → search → detail)
- "30-second demo" section: install → demo → tui
- Update Quick Start to show install script path alongside cargo
- Star History badge

## Success Criteria

- `cargo install shabka-cli shabka-mcp` works from crates.io
- `curl ... | sh` installs pre-built binaries on Linux/macOS
- `shabka demo && shabka tui` shows a compelling experience in <10 seconds
- CI passes on every PR, releases are automated on tag push
- Listed on at least one MCP registry

## Non-Goals

- Homebrew formula (future — requires sustained maintenance)
- Windows installer/MSI (tarball + PATH is sufficient for now)
- Documentation site (README + --help is sufficient for v0.4)
