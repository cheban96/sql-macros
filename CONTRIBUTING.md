# Contributing

## Running tests

```
cargo test --workspace
```

This currently runs unit tests for the attribute parser in
`sql-macros-derive/src/parser.rs`. Those tests exercise the `#[table(...)]`
parsing logic directly and don't need a database.

## Examples / end-to-end coverage

[`examples/`](examples/) is a separate workspace member with one runnable
example per derive macro (`SqlSelect`, `SqlInsert`, `SqlUpdate`, ...),
verified against a real Postgres instance and checked in with a `.sqlx`
offline query cache — see [`examples/README.md`](examples/README.md) for how
to run them for real, and how to regenerate the cache after changing any
generated query text. `cargo build`/`test`/`clippy` on this crate need no
`DATABASE_URL` at all; they build from the cache.

## Code style

`cargo fmt` and `cargo clippy --workspace --all-targets -- -D warnings`
must be clean; both run in CI.
