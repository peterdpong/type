# Type

A minimal native WYSIWYG Markdown editor built directly on GPUI.

## Run

On macOS, install Rust and the Xcode command-line tools.

```sh
cargo run -p type-app
```

## Build a macOS app

```sh
./scripts/bundle-macos.sh
open target/debug/Type.app
```

For a release build:

```sh
./scripts/bundle-macos.sh release
open target/release/Type.app
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
