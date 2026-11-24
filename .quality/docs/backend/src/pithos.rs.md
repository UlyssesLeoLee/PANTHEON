# Technical Design: `backend/src/pithos.rs`

Source: `backend/src/pithos.rs`

Source-SHA256: `c4e761ad8c3a6b565bd13a11521c654340e2ab3435e993c2215324dd7e42a528`

## Responsibility

`pithos.rs` is the read-only adapter that lets the PANTHEON workbench browse a PITHOS (PolyDB V1) key-value or document store through the same `Schema` and `QueryResponse` JSON contracts that the SQLite backend uses. The frontend needs no change.

- **Store discovery.** The configured root is either one `*.wal` file or a directory. A directory that holds no `*.wal` file itself but whose `data` subdirectory holds one resolves to that `data` subdirectory. Every regular file with the extension `wal` (case-insensitive, non-recursive) is one store. The store name is the file stem, for example `kv` for `kv.wal` and `document` for `document.wal`. Resolution and discovery run on every request, so data written after PANTHEON starts becomes visible without a restart.
- **WAL decoding.** The adapter decodes the PITHOS append-only log itself. Each request reads the whole file into memory with a read-only handle, replays it, and keeps the live `(namespace, key) -> value` map. It never opens a file for append, never creates a file or directory, and never calls the PITHOS engine, so a typo in the configured path cannot create data and a running PITHOS process is never disturbed.
- **Schema mapping.** Each live namespace of each store becomes one table named `<store>/<namespace>`. Every table has two columns: `key` (`TEXT`, primary key, not null) and `value` (`BLOB`, not null, not a primary key; an empty value is an empty byte string, never NULL). No table has a foreign key. A namespace whose keys were all deleted does not appear.
- **Query language.** A deliberately small read-only SQL subset is translated into point lookups and prefix scans over the in-memory map. Anything else is rejected with an explanation of the supported grammar.
- **Fail-closed integrity.** A torn tail, bad magic, CRC mismatch, oversized frame, or malformed record produces an error that names the file. The adapter never guesses, repairs, or skips damaged data. If a PITHOS writer is mid-append, the reader may see a torn tail and the request fails; retrying succeeds once the write completes.

## Dependencies

- Upstream callers: `backend/src/lib.rs` dispatches `/api/schema` and `/api/query` to `inspect` and `run_query` when the configured backend is PITHOS.
- Crate types reused from `lib.rs`: `ApiError`, `Schema`, `TableInfo`, `Column`, `QueryResponse`.
- Third-party crates: `serde_json` only, for `Value` cells. Everything else is `std` (`fs`, `io`, `path`, `collections::BTreeMap`).
- External format contract: the PITHOS V1 WAL written by `E:\PITHOS\src\store.rs` (`WalEngine`). The adapter does not link the `polydb` crate. The reasons are that `WalEngine::open` creates files and opens for append, that its public API cannot list namespaces, and that a path dependency on another drive would break CI and other machines. Compatibility is pinned by golden WAL fixtures produced by the real `polydb` binary and checked in the unit tests.
- No network, no subprocess, no environment access.

## Public Contract

**Constants.** `MAX_WAL_BYTES = 256 MiB` is the largest WAL file the workbench will load. `MAX_PAYLOAD = 4 MiB` is the largest single record, as in PITHOS.

**WAL v1 frame.** A file is a concatenation of frames: magic `PDB1`, `u32` little-endian payload length, payload, `u32` little-endian CRC-32 (IEEE, reflected polynomial `0xEDB88320`) of the payload. The payload is `op:u8` (1 put, 2 delete), `namespace_len:u16`, `key_len:u32`, `value_len:u32`, then the namespace, key and value bytes. Namespace and key are non-empty UTF-8. A delete carries an empty value. Later frames override earlier ones.

**`pub fn inspect(root: &Path) -> Result<Schema, ApiError>`.** Returns `Schema { database: "PITHOS · <name of root>", tables }` with tables ordered by store name and then namespace. An existing directory without any WAL file yields an empty table list. A missing or unreadable root yields `ApiError::Database`.

**`pub fn run_query(root: &Path, sql: &str, requested_limit: Option<usize>) -> Result<QueryResponse, ApiError>`.** Grammar (keywords case-insensitive, whitespace free, one optional trailing semicolon):

```
query      := SELECT projection FROM table [WHERE predicate] [LIMIT integer] [;]
projection := * | column {, column}          column := key | value
table      := "<store>/<namespace>"          double-quoted, "" escapes a quote
predicate  := key = 'text'                   point lookup, '' escapes a quote
            | key LIKE 'prefix%'             prefix scan, one trailing % only, _ is literal
```

- Columns of the response follow the projection (`*` means `key`, `value`). Rows are ordered by ascending key.
- A value cell is a JSON string when the bytes are valid UTF-8, otherwise the text `[BLOB <n> bytes]`.
- The row cap is `requested_limit.unwrap_or(200).clamp(1, 1000)`, the same rule as the SQLite backend. A SQL `LIMIT n` can only lower it. `truncated` is true only when more matching rows exist and the row cap, not a SQL `LIMIT`, stopped the result.
- Errors: grammar violations, unknown tables (the message lists the available tables), unknown columns, `LIKE` patterns without exactly one trailing `%`, and `LIMIT` values that are not non-negative integers return `ApiError::Unsupported`. I/O failures, oversized files and WAL corruption return `ApiError::Database`.

**Invariants.** No write path exists in the module. A query never builds a file path from query text, because tables are resolved against the discovered stores. Calls are stateless and safe to run concurrently.

## Change Notes

New module. The SQLite backend and its JSON contracts are unchanged. The only crate-level additions are the module declaration in `lib.rs` and the `Unsupported` error variant that this module uses. Relational PITHOS data (the experimental Turso worker) is outside this module and is not supported by it.
