# {{crate}}

[![Test Status](https://github.com/DeterminateSystems/srv-rs/workflows/Rust/badge.svg?event=push)](https://github.com/DeterminateSystems/srv-rs/actions)
[![Crate](https://img.shields.io/crates/v/srv-rs-upd.svg)](https://crates.io/crates/srv-rs-upd)

{{readme}}

## Usage

Add {{crate}} to your dependencies in `Cargo.toml`, enabling at least one of
the DNS resolver backends (see [Alternative Resolvers](README.md#alternative-resolvers-and-target-selection-policies)).

```toml
[dependencies]
{{crate}} = { version = "{{version}}", features = ["trust-dns"] }
```

## Contributing

1. Clone the repo
2. Make some changes
3. Test: `cargo test --all-features`
4. Format: `cargo fmt`
5. Clippy: `cargo clippy --all-features --tests -- -Dclippy::all`
6. Bench: `cargo bench --all-features`
7. If modifying crate-level docs (`src/lib.rs`) or `README.tpl`, update `README.md`:
    1. `cargo install cargo-readme`
    2. `cargo readme > README.md`

## History

Forked from https://github.com/deshaw/srv-rs/.
