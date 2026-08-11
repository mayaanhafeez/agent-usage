# Contributing

Contributions are welcome. Keep changes focused and include tests for behavior
that can be exercised without real provider credentials.

## Development

Rust 1.85 or newer is required.

```sh
git clone https://github.com/mayaanhafeez/agent-usage.git
cd agent-usage
cargo test
```

Before opening a pull request, run the same checks as CI:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

To inspect data for one provider without starting the TUI, use plain mode:

```sh
cargo run -- --provider codex --plain
```

Never commit credentials, provider history, local databases, or output that
contains account details. Use synthetic fixtures for parser tests.

## Pull requests

- Explain the user-visible behavior and why the change is needed.
- Keep provider-specific parsing isolated in `src/usage.rs`.
- Update `README.md` and `docs/data-sources.md` when adding a provider or data
  source.
- Confirm formatting, Clippy, and tests pass on your platform.
