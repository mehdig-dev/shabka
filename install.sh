#!/usr/bin/env bash
set -euo pipefail

# Shabka installer — builds from source and installs CLI + MCP server
# Usage: curl -sSL https://raw.githubusercontent.com/mehdig-dev/shabka/main/install.sh | bash

REPO="https://github.com/mehdig-dev/shabka.git"
INSTALL_DIR="${SHABKA_INSTALL_DIR:-$HOME/.local/bin}"
CLONE_DIR="${SHABKA_CLONE_DIR:-$HOME/.local/share/shabka}"

info()  { printf '\033[1;34m=>\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m=>\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m=>\033[0m %s\n' "$*"; }
error() { printf '\033[1;31m=>\033[0m %s\n' "$*" >&2; exit 1; }

# --- Prerequisites ---

command -v cargo >/dev/null 2>&1 || error "Rust toolchain not found. Install from https://rustup.rs"
command -v git   >/dev/null 2>&1 || error "git not found."
command -v docker >/dev/null 2>&1 || warn "Docker not found — you'll need it to run HelixDB."

info "Installing Shabka to $INSTALL_DIR"

# --- Clone or update ---

if [ -d "$CLONE_DIR/.git" ]; then
    info "Updating existing clone at $CLONE_DIR"
    git -C "$CLONE_DIR" pull --ff-only
else
    info "Cloning $REPO"
    git clone "$REPO" "$CLONE_DIR"
fi

# --- Build ---

info "Building CLI and MCP server (this may take a few minutes)..."
cargo install --path "$CLONE_DIR/crates/shabka-cli" --root "$HOME/.local" --no-default-features --locked 2>/dev/null \
  || cargo install --path "$CLONE_DIR/crates/shabka-cli" --root "$HOME/.local" --no-default-features
cargo install --path "$CLONE_DIR/crates/shabka-mcp" --root "$HOME/.local" --no-default-features --locked 2>/dev/null \
  || cargo install --path "$CLONE_DIR/crates/shabka-mcp" --root "$HOME/.local" --no-default-features

# --- Verify ---

if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
    warn "$INSTALL_DIR is not in your PATH. Add it:"
    warn "  export PATH=\"$INSTALL_DIR:\$PATH\""
fi

ok "Installed:"
ok "  shabka     — CLI tool"
ok "  shabka-mcp — MCP server"
echo ""
info "Next steps:"
echo "  1. Start HelixDB:    cd $CLONE_DIR && just db"
echo "  2. Register MCP:     claude mcp add shabka -- $INSTALL_DIR/shabka-mcp"
echo "  3. Init config:      shabka init"
echo "  4. Check setup:      shabka init --check"
