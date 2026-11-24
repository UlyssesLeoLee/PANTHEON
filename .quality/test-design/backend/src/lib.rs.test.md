# Test Design: `backend/src/lib.rs`

Source: `backend/src/lib.rs`

Source-SHA256: `389e18c9715c8a6dcec95cc2b33d435701a31b4b1e1b6abe3ad79aad5647a0c1`

## Test Strategy

Three levels. (1) Inline unit tests in `backend/src/lib.rs` for private helpers and pure functions (`Backend::detect`, `Settings::from_lookup`, `blocking`, `ApiError` mapping) and the existing SQLite suite. (2) HTTP tests in `backend/tests/api_http.rs` that drive the real `router` in-process with `tower::ServiceExt::oneshot` against a SQLite database created in a temp directory and against the golden PITHOS fixtures, plus one test that serves over a real TCP socket on `127.0.0.1:0`. (3) Process-level tests of the compiled binary live in `backend/tests/cli_main.rs` and are described in the `main.rs` test design. Environment access is injected through a lookup closure, so no test mutates process environment variables. Ports are always chosen by the operating system. Coverage is measured with `cargo llvm-cov` and must be strictly above 90 percent for this file.

## Test Cases

| Case ID | Input / Setup | Expected Outcome | Test Location |
| --- | --- | --- | --- |
| TC-001 | Existing SQLite suite: schema read, SELECT, DML blocked, CTE write blocked, limit, multi-statement denial, quoted semicolon, ATTACH blocked | All eight original tests pass unchanged | `backend/src/lib.rs::tests::schema_has_table` |
| TC-002 | `Backend::detect` on a directory, `x.wal`, `X.WAL`, an extension-less file starting with `PDB1`, a SQLite file, an empty extension-less file, a missing path | Pithos, Pithos, Pithos, Pithos, Sqlite, Sqlite, error text naming the path | `backend/src/lib.rs::tests::detect_backend_kinds` |
| TC-003 | `Settings::from_lookup` with a SQLite file and with a PITHOS directory, no origin and no bind given | Backend kind matches, origin defaults to `http://localhost:3000`, bind defaults to `127.0.0.1:8788` | `backend/src/lib.rs::tests::settings_defaults_and_kinds` |
| TC-004 | Same function with explicit origin and bind, and with a token of exactly 24 characters | Values are taken as given and the 24-character token is accepted | `backend/src/lib.rs::tests::settings_explicit_values_and_minimum_token` |
| TC-005 | Missing `PANTHEON_DB_PATH`, nonexistent path, missing token, 23-character token, origin containing a newline | Each returns an error that names the variable or the path | `backend/src/lib.rs::tests::settings_rejects_bad_configuration` |
| TC-006 | `GET /health` without any header | 200 with `status` `ok`, `product` `PANTHEON` and a `version` | `backend/tests/api_http.rs::health_is_public` |
| TC-007 | `/api/schema` with no header, a wrong token, a token without the `Bearer ` prefix, and the correct token | 401, 401, 401, 200 with the schema | `backend/tests/api_http.rs::schema_requires_exact_bearer_token` |
| TC-008 | `POST /api/query` against SQLite with `SELECT`, `DELETE`, a malformed JSON body and a missing content type | 200 with rows, 403 with the fixed message, 400-range status, 415 | `backend/tests/api_http.rs::sqlite_query_endpoint_behaviour` |
| TC-009 | `/api/schema` against the golden PITHOS fixtures | 200 with tables `document/app`, `kv/app`, `kv/demo` and the key and value columns | `backend/tests/api_http.rs::pithos_schema_over_http` |
| TC-010 | `/api/query` against PITHOS with a scan, a lookup, an unsupported statement and an unknown table | Rows with exact JSON field names, then 400 with the grammar hint, then 400 listing the available tables | `backend/tests/api_http.rs::pithos_query_over_http` |
| TC-011 | Preflight `OPTIONS /api/query` from the configured origin and from a different origin | The allow-origin header is always the configured origin and never echoes the foreign origin, so a browser page at another origin does not match and is refused | `backend/tests/api_http.rs::cors_allows_only_configured_origin` |
| TC-012 | `serve` on `127.0.0.1:0`, raw TCP `GET /health`, then the shutdown signal fires | Response is `200` with the JSON body and `serve` returns `Ok` after shutdown | `backend/tests/api_http.rs::serve_answers_over_tcp_and_shuts_down` |
| TC-013 | `run_with_shutdown` with valid settings, bind `127.0.0.1:0` and an already-completed shutdown future | Returns `Ok` | `backend/tests/api_http.rs::run_with_shutdown_starts_and_stops` |
| TC-014 | `run_with_shutdown` with invalid configuration, and with a bind address that is already in use | `Err` naming the variable, and `Err` containing `cannot bind` | `backend/tests/api_http.rs::run_with_shutdown_reports_errors` |
| TC-015 | `blocking` given a closure that panics, and one that returns an `ApiError` | `ApiError::Database` containing `worker task failed`, and the original error passes through | `backend/src/lib.rs::tests::blocking_maps_worker_failures` |
| TC-016 | Convert each `ApiError` variant to a response | Status 401, 403, 400, 400, 400 and body `{"error": "<message>"}` | `backend/src/lib.rs::tests::api_error_status_and_body` |
| TC-017 | `/api/schema` against a PITHOS directory whose `kv.wal` holds a truncated frame | 400 and an error message that names `kv.wal`; the corrupt data is never skipped | `backend/tests/api_http.rs::pithos_corruption_is_reported_over_http` |

## Boundary & Failure Cases

- Tokens of 23, 24 and empty length; an empty `Bearer ` header; a header with lower-case `bearer`.
- Origin value that is not a valid HTTP header value, and a request from an origin other than the configured one.
- `PANTHEON_DB_PATH` pointing at an empty file, at a directory without WAL files (valid, shows no tables), and at a missing path.
- Port already in use during startup, and a shutdown signal that fires while no request is in flight.
- SQLite request that reaches the blocking pool while the database file is locked or malformed returns `Database`, never a panic.
- Large result sets continue to be capped at 1000 rows for SQLite.

## Regression Scope

- SQLite endpoints, status codes and JSON shape used by `frontend/app/page.tsx`.
- `backend/src/pithos.rs` through the dispatch.
- `backend/src/main.rs`, which now calls `run` and prints its error.
- Startup documentation in `README.md` and `scripts/start-dev.ps1`, which rely on the environment variable names and the default bind address.
