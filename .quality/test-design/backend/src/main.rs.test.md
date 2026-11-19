# Test Design: `backend/src/main.rs`

Source: `backend/src/main.rs`

Source-SHA256: `0123383cc99a3aad38c55220b5d363b28fc27ef32ab890afe8bfa34c4551602c`

## Test Strategy

`main.rs` is exercised only as a compiled process. `backend/tests/cli_main.rs` spawns the real `pantheon-api` binary through `env!("CARGO_BIN_EXE_pantheon-api")` with `env_clear()` plus the variables each case needs, so the host environment never leaks in. Failure cases run to completion and assert exit status and streams. Startup cases read the first standard-output line to learn the bound address (the tests bind port `0`), send a raw HTTP request over TCP, and then kill the child. Fixtures are a temp-directory SQLite file and the golden PITHOS directory in `backend/tests/fixtures/pithos`. Under `cargo llvm-cov` the instrumented child processes write their profile data on normal exit, which is how the failure-path runs attribute coverage to `main.rs`. Coverage must be strictly above 90 percent for this file.

## Test Cases

| Case ID | Input / Setup | Expected Outcome | Test Location |
| --- | --- | --- | --- |
| TC-001 | Empty environment | Exit status 1, standard error mentions `PANTHEON_DB_PATH`, standard output empty | `backend/tests/cli_main.rs::fails_without_database_path` |
| TC-002 | `PANTHEON_DB_PATH` set to a path that does not exist | Exit status 1 and the message names the path | `backend/tests/cli_main.rs::fails_for_missing_database_path` |
| TC-003 | Valid path with a 23-character `PANTHEON_API_TOKEN` | Exit status 1 and the message mentions `PANTHEON_API_TOKEN` | `backend/tests/cli_main.rs::fails_for_short_token` |
| TC-004 | Valid PITHOS fixture directory, bind `127.0.0.1:0` | Banner names the real address and `backend: pithos`; `GET /health` returns 200; authorized `/api/schema` lists the fixture tables | `backend/tests/cli_main.rs::starts_with_pithos_directory` |
| TC-005 | Valid temp SQLite file, bind `127.0.0.1:0` | Banner says `backend: sqlite`; `GET /health` returns 200; authorized `/api/query` returns the inserted row | `backend/tests/cli_main.rs::starts_with_sqlite_file` |
| TC-006 | Bind address already occupied by the test | Exit status 1 and the message contains `cannot bind` | `backend/tests/cli_main.rs::fails_when_port_is_in_use` |

## Boundary & Failure Cases

- Token of exactly 24 characters starts successfully (covered by the startup cases) and 23 characters is rejected.
- A PITHOS directory that holds no WAL file is a valid start and reports an empty schema.
- Failures must not print a panic trace: standard error contains the single `failed to start` line and no `panicked at` text.
- The banner uses the actual bound address, which differs from the requested address when port `0` is requested.
- The child process is always killed in a cleanup guard so a failed assertion cannot leave a server running.

## Regression Scope

- Environment variable names, the default origin and the default bind address, which the README, `scripts/start-dev.ps1` and the frontend connection dialog depend on.
- `backend/src/lib.rs` (`run`, `Settings`, `serve`) and the SQLite and PITHOS request paths reached through a real socket.
