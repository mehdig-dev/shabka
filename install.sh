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
