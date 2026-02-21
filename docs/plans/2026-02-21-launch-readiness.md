# v0.4.0 Launch Readiness Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make Shabka discoverable and installable with a compelling first-run experience — crates.io, pre-built binaries, install script, demo data, MCP registry listings.

**Architecture:** Six independent workstreams: (1) release CI/CD pipeline, (2) install script, (3) crates.io readiness, (4) `shabka demo` command, (5) MCP registry metadata, (6) README refresh. Tasks 1-4 are code; 5-6 are content.

**Tech Stack:** GitHub Actions, `cross` (cross-compilation), shell scripting, Rust (demo command)

---

### Task 1: Add release workflow for cross-compiled binaries

**Files:**
- Create: `.github/workflows/release.yml`

**Step 1: Write the release workflow**

```yaml
name: Release

on:
  push:
    tags: ["v*"]

permissions:
  contents: write

env:
  CARGO_TERM_COLOR: always

jobs:
  build:
    name: Build ${{ matrix.target }}
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        include:
          - target: x86_64-unknown-linux-gnu
            os: ubuntu-latest
          - target: x86_64-apple-darwin
            os: macos-latest
          - target: aarch64-apple-darwin
            os: macos-latest
          - target: x86_64-pc-windows-msvc
            os: windows-latest

    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - uses: Swatinem/rust-cache@v2
        with:
          key: ${{ matrix.target }}

      - name: Build shabka-cli
        run: cargo build --release --no-default-features -p shabka-cli --target ${{ matrix.target }}

      - name: Build shabka-mcp
        run: cargo build --release --no-default-features -p shabka-mcp --target ${{ matrix.target }}

      - name: Package (Unix)
        if: runner.os != 'Windows'
        run: |
          mkdir -p dist
          cp target/${{ matrix.target }}/release/shabka dist/
          cp target/${{ matrix.target }}/release/shabka-mcp dist/
          cd dist && tar czf ../shabka-${{ github.ref_name }}-${{ matrix.target }}.tar.gz *

      - name: Package (Windows)
        if: runner.os == 'Windows'
        run: |
          mkdir dist
          cp target/${{ matrix.target }}/release/shabka.exe dist/
          cp target/${{ matrix.target }}/release/shabka-mcp.exe dist/
          cd dist && 7z a ../shabka-${{ github.ref_name }}-${{ matrix.target }}.zip *

      - uses: actions/upload-artifact@v4
        with:
          name: shabka-${{ matrix.target }}
          path: shabka-${{ github.ref_name }}-*

  release:
    name: Create Release
    needs: build
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: actions/download-artifact@v4
        with:
          path: artifacts
          merge-multiple: true

      - name: Generate checksums
        run: |
          cd artifacts
          sha256sum shabka-* > SHA256SUMS.txt

      - name: Create GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          generate_release_notes: true
          files: |
            artifacts/shabka-*
            artifacts/SHA256SUMS.txt
```

**Step 2: Verify workflow syntax**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))" 2>&1 || echo "Install pyyaml or just visually verify"`

**Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: add release workflow for cross-compiled binaries"
```

---

### Task 2: Update CI workflow to also test CLI and MCP crates

**Files:**
- Modify: `.github/workflows/ci.yml`

**Step 1: Add CLI and MCP test steps**

Add after the existing `shabka-hooks` test step:

```yaml
      - name: Unit tests (shabka-cli)
        run: cargo test -p shabka-cli --no-default-features

      - name: Unit tests (shabka-mcp)
        run: cargo test -p shabka-mcp --no-default-features
```

**Step 2: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add shabka-cli and shabka-mcp to test matrix"
```

---

### Task 3: Create install script

**Files:**
- Create: `install.sh`

**Step 1: Write the install script**

```bash
#!/bin/sh
set -eu

REPO="mehdig-dev/shabka"
INSTALL_DIR="${SHABKA_INSTALL_DIR:-$HOME/.shabka/bin}"

main() {
    need_cmd curl
    need_cmd tar

    local _arch
    _arch="$(uname -m)"
    local _os
    _os="$(uname -s)"

    local _target
    case "$_os" in
        Linux)
            case "$_arch" in
                x86_64) _target="x86_64-unknown-linux-gnu" ;;
                *) err "Unsupported architecture: $_arch" ;;
            esac
            ;;
        Darwin)
            case "$_arch" in
                x86_64) _target="x86_64-apple-darwin" ;;
                arm64)  _target="aarch64-apple-darwin" ;;
                *) err "Unsupported architecture: $_arch" ;;
            esac
            ;;
        *) err "Unsupported OS: $_os" ;;
    esac

    local _version
    _version="$(curl -sSf "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)"
    if [ -z "$_version" ]; then
        err "Failed to determine latest version"
    fi

    local _url="https://github.com/$REPO/releases/download/$_version/shabka-$_version-$_target.tar.gz"

    printf "Installing shabka %s for %s...\n" "$_version" "$_target"

    mkdir -p "$INSTALL_DIR"

    curl -sSfL "$_url" | tar xz -C "$INSTALL_DIR"
    chmod +x "$INSTALL_DIR/shabka" "$INSTALL_DIR/shabka-mcp"

    printf "\n✓ Installed shabka and shabka-mcp to %s\n" "$INSTALL_DIR"

    # Check if in PATH
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            printf "\n  Add to your PATH:\n"
            printf "    export PATH=\"%s:\$PATH\"\n\n" "$INSTALL_DIR"
            ;;
    esac
}

need_cmd() {
    if ! command -v "$1" > /dev/null 2>&1; then
        err "Required command not found: $1"
    fi
}

err() {
    printf "error: %s\n" "$1" >&2
    exit 1
}

main "$@"
```

**Step 2: Make executable and test locally**

Run: `chmod +x install.sh && shellcheck install.sh 2>/dev/null || echo "shellcheck not installed, skip"`

**Step 3: Commit**

```bash
git add install.sh
git commit -m "feat: add install script for pre-built binaries"
```

---

### Task 4: Prepare crates.io publishing

**Files:**
- Modify: `Cargo.toml` (workspace)
- Modify: `crates/shabka-core/Cargo.toml`
- Modify: `crates/shabka-cli/Cargo.toml`
- Modify: `crates/shabka-mcp/Cargo.toml`
- Create: `scripts/publish.sh`

**Step 1: Verify each crate has required crates.io metadata**

Each crate needs: `name`, `version`, `description`, `license`, `repository`. Check that `shabka-cli` and `shabka-mcp` have `description` set (they already do per earlier reads). Ensure `readme` is set in workspace metadata.

**Step 2: Add `readme` to workspace package if missing**

In workspace `Cargo.toml`, verify `readme = "README.md"` exists (it does).

**Step 3: Write the publish script**

```bash
#!/bin/sh
set -eu

echo "Publishing shabka-core..."
cargo publish -p shabka-core --no-default-features

echo "Waiting for crates.io to index shabka-core..."
sleep 30

echo "Publishing shabka-cli..."
cargo publish -p shabka-cli --no-default-features

echo "Publishing shabka-mcp..."
cargo publish -p shabka-mcp --no-default-features

echo "✓ All crates published."
```

**Step 4: Dry-run publish to check for issues**

Run: `cargo publish -p shabka-core --no-default-features --dry-run 2>&1 | tail -5`

**Step 5: Commit**

```bash
chmod +x scripts/publish.sh
git add scripts/publish.sh
git commit -m "feat: add publish script for crates.io"
```

---

### Task 5: Implement `shabka demo` command

**Files:**
- Modify: `crates/shabka-cli/src/main.rs` (add Demo variant + cmd_demo function)

**Step 1: Add the Demo variant to the Cli enum**

In the `Cli` enum, after the `Tui` variant:

```rust
    /// Populate sample memories for demonstration
    Demo {
        /// Remove demo memories instead of creating them
        #[arg(long)]
        clean: bool,
    },
```

**Step 2: Add the match arm in `run()`**

```rust
        Cli::Demo { clean } => {
            let storage = make_storage(config)?;
            let embedder = EmbeddingService::from_config(&config.embedding)
                .context("failed to create embedding service")?;
            let history = HistoryLogger::new(config.history.enabled);
            cmd_demo(&storage, &embedder, user_id, &history, clean).await
        }
```

**Step 3: Write the `cmd_demo` function**

This creates 12 sample memories across all 9 kinds, with 5 relations between them. Each memory title starts with `[demo]` so `--clean` can find and remove them.

Content covers realistic software engineering scenarios: auth decisions, error patterns, database lessons, coding preferences.

Key implementation details:
- Check if demo memories already exist (search for `[demo]` prefix in titles via timeline) — skip if found, print "Demo data already exists"
- Create memories with `Memory::new()`, embed each, save with embedding
- Create 5 relations: fixes, caused_by, related, supersedes, contradicts
- `--clean` flag: load timeline, find `[demo]`-prefixed titles, delete them
- Print summary: "Created 12 demo memories and 5 relations. Try: shabka tui"

**Step 4: Write a test for demo idempotence**

In `tui/app.rs` tests (or inline in main.rs if simpler): this is a CLI command so primarily tested manually. The core functions it calls (save_memory, embed, add_relation) are already tested in shabka-core.

**Step 5: Run clippy + tests**

Run: `cargo clippy --workspace --no-default-features -- -D warnings && cargo test -p shabka-core --no-default-features && cargo test -p shabka-cli --no-default-features`

**Step 6: Commit**

```bash
git add crates/shabka-cli/src/main.rs
git commit -m "feat(cli): add shabka demo command with sample memories"
```

---

### Task 6: Prepare MCP registry metadata

**Files:**
- Create: `docs/mcp-registry.md`

**Step 1: Write the metadata document**

Include the information needed for mcp.so and Smithery submissions:
- Name: Shabka
- Description: Persistent memory for LLM coding agents. 14 tools for saving, searching, and connecting knowledge across AI sessions.
- Install: `cargo install shabka-mcp` or `curl -sSf ... | sh`
- Tool list with one-line descriptions (all 14 tools)
- Categories: memory, knowledge-management, developer-tools
- Repository URL
- Screenshot paths

**Step 2: Commit**

```bash
git add docs/mcp-registry.md
git commit -m "docs: add MCP registry submission metadata"
```

---

### Task 7: Update README for launch

**Files:**
- Modify: `README.md`

**Step 1: Add install script to Quick Start**

Before the existing `cargo install` section, add:

```markdown
### Option A: Install script (recommended)

```bash
curl -sSf https://raw.githubusercontent.com/mehdig-dev/shabka/main/install.sh | sh
```

### Option B: From crates.io

```bash
cargo install shabka-cli shabka-mcp
```
```

**Step 2: Add "Try it in 30 seconds" section**

After Quick Start:

```markdown
## Try It in 30 Seconds

```bash
shabka demo        # Seed sample memories
shabka tui         # Browse interactively
shabka search "authentication"  # Search from CLI
```
```

**Step 3: Add TUI screenshot/reference**

Add a row to the screenshots table showing the TUI.

**Step 4: Commit**

```bash
git add README.md
git commit -m "docs: update README with install script and demo workflow"
```

---

### Task 8: Version bump and tag

**Files:**
- Modify: `Cargo.toml` (workspace version → `0.4.0`)
- Modify: `CHANGELOG.md`

**Step 1: Bump workspace version**

Change `version = "0.3.0"` to `version = "0.4.0"` in workspace `Cargo.toml`.

**Step 2: Add CHANGELOG entry**

```markdown
## [0.4.0] — 2026-02-XX

Launch readiness — distribution and onboarding.

- **Interactive TUI** — `shabka tui` for browsing, searching, and inspecting memories (ratatui-based).
- **`shabka demo`** — seed sample memories for instant first-run experience.
- **Install script** — `curl -sSf ... | sh` for Linux/macOS.
- **Release pipeline** — GitHub Actions cross-compiles for Linux, macOS (Intel + Apple Silicon), Windows.
- **CI expanded** — tests all 5 crates on every push/PR.
```

**Step 3: Commit and tag**

```bash
cargo check --workspace --no-default-features  # verify version bump compiles
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "release: v0.4.0 — launch readiness"
git tag v0.4.0
```

**Step 4: Push tag to trigger release**

```bash
git push origin main --tags
```

This triggers the release workflow, which builds binaries and creates the GitHub Release.

---

### Task 9: Publish to crates.io

**Step 1: Run the publish script**

```bash
./scripts/publish.sh
```

Requires `cargo login` with a valid crates.io token first.

**Step 2: Verify**

Run: `cargo install shabka-cli shabka-mcp` in a clean environment (or check crates.io web UI).

---

### Task 10: Submit to MCP registries

**Step 1: Submit to mcp.so**

Go to mcp.so, find "Submit" or "Add Server", fill in metadata from `docs/mcp-registry.md`.

**Step 2: Submit to Smithery**

Go to smithery.ai, follow their submission process with the same metadata.

These are manual steps — no code involved.
