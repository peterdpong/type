# Type

A minimal native WYSIWYG Markdown editor built directly on GPUI.

## Run

On macOS, install Rust and the Xcode command-line tools.

```sh
cargo run -p type-app
cargo run -p type-app -- examples/welcome.md
cargo run -p type-app -- notes.md todo.md
```

## Format

Requires Node.js and npm.

```sh
npm ci
npm run format
```

## Checks

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm run format:check
```

Run the native header input checks on macOS:

```sh
cargo run -p type-app --example native_header_input
```
