# Installation

## Option A: Install script (recommended)

Downloads the latest pre-built binary for your platform (Linux x86_64, macOS Intel/ARM).

```bash
curl -sSf https://raw.githubusercontent.com/mehdig-dev/shabka/main/install.sh | sh
```

Installs `shabka` and `shabka-mcp` to `~/.shabka/bin`. Override with `SHABKA_INSTALL_DIR`.

## Option B: Homebrew (macOS / Linux)

```bash
brew install mehdig-dev/tap/shabka
```

## Option C: From crates.io

Requires [Rust](https://rustup.rs/).

```bash
cargo install shabka-cli shabka-mcp
```

## Verify

```bash
shabka status
```

If the command isn't found, make sure the install directory is in your `PATH`:

```bash
# Install script
export PATH="$HOME/.shabka/bin:$PATH"

# Cargo
export PATH="$HOME/.cargo/bin:$PATH"
```

## Next Steps

- [Quick Start](quickstart.md) — register the MCP server and try it
- [Configuration](configuration.md) — embedding providers, storage backend, LLM features
