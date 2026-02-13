# Kaizen — Development task automation

# -- Development --

# Build all workspace crates
build:
    cargo build --workspace

# Run tests (no-default-features avoids fastembed/ONNX on WSL2)
test:
    cargo test -p kaizen-core --no-default-features

# Clippy lint + test
check:
    cargo clippy --workspace --no-default-features -- -D warnings
    cargo test -p kaizen-core --no-default-features

# Run integration tests (requires HelixDB: just db)
test-integration:
    cargo test -p kaizen-core --no-default-features -- --ignored

# Run only Ollama embedding tests (requires Ollama + HelixDB)
test-ollama:
    cargo test -p kaizen-core --no-default-features --test ollama_embedding -- --ignored

# Run only HelixDB roundtrip tests (requires HelixDB)
test-helix:
    cargo test -p kaizen-core --no-default-features --test helix_roundtrip -- --ignored

# Run everything: unit + integration (requires HelixDB)
test-all:
    cargo test -p kaizen-core --no-default-features -- --include-ignored

# Format code
fmt:
    cargo fmt --all

# -- HelixDB --

# Install the helix CLI (build from source to avoid OpenSSL 3 issues on WSL2)
db-setup:
    cargo install --git https://github.com/HelixDB/helix-db helix-cli

# Build and deploy HelixDB locally (port 6969)
db:
    cd helix && helix push dev

# Stop the local HelixDB instance
db-stop:
    cd helix && helix stop dev

# View HelixDB logs
db-logs:
    cd helix && helix logs dev

# -- MCP Server --

# Build and run the MCP server (no-default-features for WSL2 compat)
mcp:
    cargo run -p kaizen-mcp --no-default-features

# Print the claude mcp add command for registration
mcp-register:
    @echo 'Run this command to register Kaizen with Claude Code:'
    @echo ''
    @echo '  claude mcp add kaizen -- cargo run -p kaizen-mcp --no-default-features'
    @echo ''

# -- Hooks --

# Build the hooks binary
hooks-build:
    cargo build -p kaizen-hooks --no-default-features

# Install hooks binary to ~/.local/bin
hooks-install: hooks-build
    mkdir -p ~/.local/bin
    cp target/debug/kaizen-hooks ~/.local/bin/

# Print hook registration instructions
hooks-register:
    @echo 'Add to your .claude/settings.json (project-level) or ~/.claude/settings.json (global):'
    @echo ''
    @echo '  "hooks": {'
    @echo '    "PostToolUse": [{ "type": "command", "command": "kaizen-hooks", "async": true }],'
    @echo '    "PostToolUseFailure": [{ "type": "command", "command": "kaizen-hooks", "async": true }],'
    @echo '    "Stop": [{ "type": "command", "command": "kaizen-hooks", "async": true }]'
    @echo '  }'
    @echo ''
    @echo 'Make sure kaizen-hooks is in your PATH (run: just hooks-install)'

# -- Web Dashboard --

# Run the web dashboard (port 37737)
web:
    cargo run -p kaizen-web --no-default-features

# -- CLI --

# Build and install the CLI
cli-install:
    cargo install --path crates/kaizen-cli --no-default-features

# -- E2E Tests --

# Run Playwright E2E tests (requires: just db && just web)
web-test:
    cd tests/e2e && npx playwright test

# Run Playwright E2E tests in headed mode
web-test-headed:
    cd tests/e2e && npx playwright test --headed

# Install E2E test dependencies
web-test-setup:
    cd tests/e2e && npm install && npx playwright install chromium

# -- Full Dev Flow --

# Start DB then run MCP server
dev: db mcp

# Clean build artifacts
clean:
    cargo clean
