# Technical Design: `backend/src/main.rs`

Source: `backend/src/main.rs`

Source-SHA256: `0123383cc99a3aad38c55220b5d363b28fc27ef32ab890afe8bfa34c4551602c`

## Responsibility

`main.rs` is the process entry point of the `pantheon-api` binary and contains no business logic. It installs the tracing subscriber, awaits `pantheon_api::run()`, and converts a startup or runtime error into one readable line on standard error and exit status 1. A clean shutdown returns exit status 0. All configuration, validation, binding and serving live in `backend/src/lib.rs` so they can be tested in-process.

## Dependencies

- `pantheon_api::run` from `backend/src/lib.rs`.
- `tokio` for the `#[tokio::main]` runtime and `tracing_subscriber` for log output.
- `std::process::exit` for the status code.
- Runtime environment: `PANTHEON_DB_PATH`, `PANTHEON_API_TOKEN`, `PANTHEON_UI_ORIGIN`, `PANTHEON_API_BIND`, consumed through `run`.

## Public Contract

- Input: the process environment only. No command-line arguments.
- Output on success: the banner `PANTHEON API on http://<address> (backend: <sqlite or pithos>)` on standard output, printed after the listener is bound, with the real bound address even when the bind port was `0`. The process then serves until Ctrl+C.
- Output on failure: `PANTHEON API failed to start: <reason>` on standard error, nothing on standard output, exit status 1. The reason names the variable or path at fault. The process never panics for configuration problems.
- Exit codes: 0 for graceful shutdown, 1 for any error returned by `run`.

## Change Notes

The previous entry point validated the environment with `expect` and `assert!`, which printed panic traces and could only handle a SQLite file. Validation now lives in `Settings::from_lookup`, the banner prints the actual bound address and the backend kind, and errors are reported as one line with exit status 1. Environment variable names, the default origin and the default bind address are unchanged, so existing launch commands keep working. Pointing `PANTHEON_DB_PATH` at a PITHOS directory or WAL file is newly supported.
