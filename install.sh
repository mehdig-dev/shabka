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

    local _archive="shabka-$_version-$_target.tar.gz"
    local _url="https://github.com/$REPO/releases/download/$_version/$_archive"
    local _sums_url="https://github.com/$REPO/releases/download/$_version/SHA256SUMS.txt"

    printf "Installing shabka %s for %s...\n" "$_version" "$_target"

    local _tmpdir
    _tmpdir="$(mktemp -d)"
    trap 'rm -rf "$_tmpdir"' EXIT

    curl -sSfL "$_url" -o "$_tmpdir/$_archive"
    curl -sSfL "$_sums_url" -o "$_tmpdir/SHA256SUMS.txt"

    verify_checksum "$_tmpdir" "$_archive"

    mkdir -p "$INSTALL_DIR"
    tar xzf "$_tmpdir/$_archive" -C "$INSTALL_DIR"
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

verify_checksum() {
    local _dir="$1"
    local _file="$2"

    local _expected
    _expected="$(grep "$_file" "$_dir/SHA256SUMS.txt" | awk '{print $1}')"
    if [ -z "$_expected" ]; then
        printf "warning: no checksum found for %s, skipping verification\n" "$_file"
        return 0
    fi

    local _actual
    if command -v sha256sum > /dev/null 2>&1; then
        _actual="$(cd "$_dir" && sha256sum "$_file" | awk '{print $1}')"
    elif command -v shasum > /dev/null 2>&1; then
        _actual="$(cd "$_dir" && shasum -a 256 "$_file" | awk '{print $1}')"
    else
        printf "warning: no SHA256 tool found (sha256sum or shasum), skipping verification\n"
        return 0
    fi

    if [ "$_actual" != "$_expected" ]; then
        printf "error: checksum mismatch for %s\n" "$_file" >&2
        printf "  expected: %s\n" "$_expected" >&2
        printf "  got:      %s\n" "$_actual" >&2
        exit 1
    fi

    printf "✓ Checksum verified\n"
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
