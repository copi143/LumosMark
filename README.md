# LumosMark

LumosMark is a lightweight markup language plus tooling: a Rust parser library and a language server with a VS Code extension.

## Repository layout

- `docs/lumosmark.lmm`: language reference and examples (in LumosMark format)
- `lmm.rs`: Rust library that parses LumosMark
- `lsp`: LumosMark language server (LSP)
- `vscode-extension`: VS Code extension for `.lmm` files

## Quick start

Build all Rust crates:

```bash
cargo build
```

Build only the language server:

```bash
cargo build -p lumosmark-analyzer
```

## VS Code extension

The extension starts the language server automatically. By default it runs the `lumosmark-analyzer` binary from your PATH.

Common setup flow:

1. Build the analyzer: `cargo build -p lumosmark-analyzer`
2. Point the extension at the binary in settings:
   - `lmm.languageServerPath` (e.g. `target/debug/lumosmark-analyzer`)

You can also set `LUMOSMARK_ANALYZER` in the environment to override the server path.

## Language reference

See `docs/lumosmark.lmm` for the syntax, examples, and formatting rules.

## Development notes

- The VS Code extension exposes commands to start/stop/restart the server and view logs.
- The LSP watches `**/*.lmm` and uses standard LSP stdio transport.
