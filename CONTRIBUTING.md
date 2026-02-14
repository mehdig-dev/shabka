# Contributing to Shabka

Thanks for your interest in contributing!

## Getting Started

```bash
git clone https://github.com/mehdig-dev/shabka.git
cd shabka
just build
just test
```

### Prerequisites

- Rust 1.80+
- Docker (for HelixDB)
- [just](https://github.com/casey/just) (task runner)

### Running Tests

```bash
just test                 # Unit tests (243 tests, no external services)
just check                # Clippy + unit tests
just test-integration     # Integration tests (requires: just db)
```

## Making Changes

1. Fork the repo and create a branch from `main`
2. Make your changes
3. Run `just check` to verify clippy + tests pass
4. Commit with a clear message (e.g. `feat:`, `fix:`, `docs:`, `chore:`)
5. Open a pull request

### Code Style

- Run `cargo fmt` before committing (enforced by pre-commit hook)
- No clippy warnings (`-D warnings`)
- Use `--no-default-features` to skip the `fastembed` crate (ONNX linking issues on some systems)

### Architecture

See [README.md](README.md#architecture) for the workspace layout. Key conventions:

- **HelixDB field names**: `memory_id`, `session_id` (not `id` — it's reserved)
- **Config defaults**: hardcoded in `shabka-core/src/config/mod.rs`
- **Integration tests**: use `#[ignore]` so they're skipped without services running

## Reporting Issues

- **Bug reports**: Include steps to reproduce, expected vs actual behavior, and your environment
- **Feature requests**: Describe the use case, not just the solution

## License

By contributing, you agree that your contributions will be licensed under the MIT OR Apache-2.0 license.
