# Technical Design: `backend/src/lib.rs`

Source: `backend/src/lib.rs`

Source-SHA256: `21e640ca74dbace7d01e113c46aba2915ca752e4a71bda15918aa62fb9b2161a`

## Responsibility

`lib.rs` is the PANTHEON API library: configuration, authentication, the two data endpoints, and the HTTP server lifecycle. The binary in `main.rs` only reports startup errors.

- **Backend selection.** `Backend::detect` canonicalizes the configured path and classifies it. A directory is a PITHOS store root. A regular file with the extension `wal` (case-insensitive), or whose first four bytes are `PDB1`, is a single PITHOS WAL file. Any other regular file is a SQLite database. A path that does not exist is a configuration error.
- **Endpoints.** `GET /health` is public. `GET /api/schema` and `POST /api/query` require `Authorization: Bearer <token>` and dispatch on the backend: SQLite requests use the existing read-only rusqlite path (`inspect_db`, `run_query`), PITHOS requests use `pithos::inspect` and `pithos::run_query`. Both backends return the same `Schema` and `QueryResponse` JSON.
- **Blocking work.** Database access runs on tokio's blocking pool through `blocking`, so file reads never stall the async workers. A failed worker task becomes an `ApiError::Database`.
- **Configuration.** `Settings::from_lookup` turns environment values into validated settings with explicit error text: `PANTHEON_DB_PATH` (required), `PANTHEON_API_TOKEN` (required, at least 24 characters), `PANTHEON_UI_ORIGIN` (default `http://localhost:3000`, must be a valid header value), `PANTHEON_API_BIND` (default `127.0.0.1:8788`).
- **Lifecycle.** `serve` runs the router on a bound listener until a shutdown future completes, then stops gracefully. `run_with_shutdown` wires settings, bind, the startup banner and `serve`. `run` supplies the process environment and Ctrl+C as the shutdown signal.
- **Security posture preserved.** Bearer token check, exact-origin CORS, read-only SQLite connection with `PRAGMA query_only`, single-statement SELECT/WITH/EXPLAIN only, 1000-row cap and 5-second progress timeout are unchanged.

## Dependencies

- Crates: `axum` (router, extractors, `serve` with graceful shutdown), `tokio` (listener, `spawn_blocking`, `signal::ctrl_c`), `rusqlite` (SQLite), `serde`/`serde_json`, `tower-http` (CORS), `thiserror`.
- Internal: `backend/src/pithos.rs` for the PITHOS adapter.
- Callers: `backend/src/main.rs` (calls `run`), the frontend (`frontend/app/page.tsx` consumes the JSON), and the tests in `backend/tests`.
- Environment: the four `PANTHEON_*` variables above, read only through the injected lookup function.

## Public Contract

- `enum Backend { Sqlite(PathBuf), Pithos(PathBuf) }`, `Backend::detect(&Path) -> Result<Backend, String>` and `Backend::kind(&self) -> &'static str` (`"sqlite"` or `"pithos"`).
- `struct AppState { backend: Backend, api_token: String }`.
- `struct Settings { backend, api_token, ui_origin, bind }` and `Settings::from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Settings, String>`. Error messages name the offending variable.
- `fn router(state: AppState, origin: &str) -> Router` with the routes above.
- `async fn serve(listener, state, origin, shutdown) -> std::io::Result<()>`; the future resolves after `shutdown` completes and in-flight requests finish.
- `async fn run_with_shutdown(get, shutdown) -> Result<(), String>` and `async fn run() -> Result<(), String>`. Errors are returned, never panicked, so the binary can print them and exit with status 1.
- `enum ApiError`: `Unauthorized` 401, `ForbiddenSql` 403, `InvalidConfig` 400, `Database(String)` 400, `Unsupported(String)` 400. The body is always `{"error": "<message>"}`.
- Types `Schema`, `TableInfo`, `Column`, `ForeignKey`, `QueryRequest`, `QueryResponse` keep their field names and JSON shape.
- Invariants: the SQLite path never writes; no endpoint other than `/health` works without the exact bearer token; a PITHOS root is only ever read.

## Change Notes

Adds the PITHOS backend, the `Backend`/`Settings` configuration types, graceful `serve`/`run` entry points, the `Unsupported` error variant and the blocking-pool wrapper. `AppState` now holds a `Backend` instead of a bare SQLite path, which only affects `main.rs`. Existing SQLite behavior, endpoints, status codes and JSON are unchanged. A redundant `use` inside `ApiError::into_response` that the compiler flagged is removed.
