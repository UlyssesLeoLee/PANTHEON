# Test Design: `backend/src/pithos.rs`

Source: `backend/src/pithos.rs`

Source-SHA256: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`

## Test Strategy

Unit tests live inline in `backend/src/pithos.rs` (`mod tests`) because they exercise private decoder and parser functions. Every test builds its data in a fresh `tempfile` directory, so tests are isolated and parallel-safe. Two golden fixtures, `backend/tests/fixtures/pithos/kv.wal` and `backend/tests/fixtures/pithos/document.wal`, were written by the real `polydb` binary of the PITHOS project and are read-only inputs. They pin byte-level compatibility with the real writer. Synthetic WAL files are produced by a test-local encoder that follows the documented frame layout, which lets the tests damage frames precisely. The HTTP view of the same behavior is covered in `backend/tests/api_http.rs`. Coverage is measured with `cargo llvm-cov` and must be strictly above 90 percent for this file.

## Test Cases

| Case ID | Input / Setup | Expected Outcome | Test Location |
| --- | --- | --- | --- |
| TC-001 | Golden `kv.wal` written by the real `polydb` binary (namespaces `app` and `demo`) | Decoded live entries equal the values that were put, byte for byte | `backend/src/pithos.rs::tests::golden_kv_wal_matches_real_polydb_output` |
| TC-002 | Golden `document.wal` with a JSON text value | Value bytes round-trip unchanged and render as a JSON string cell | `backend/src/pithos.rs::tests::golden_document_wal_matches_real_polydb_output` |
| TC-003 | Frames: put k=1, put k=2, delete k, put other | Last write wins and the deleted key is absent | `backend/src/pithos.rs::tests::last_write_wins_and_delete_removes_key` |
| TC-004 | Directory with `kv.wal` (namespace `app`) and `document.wal` (namespace `app`) | `inspect` returns tables `document/app` and `kv/app` in that order, columns `key` (TEXT, primary key, not null) and `value` (BLOB) | `backend/src/pithos.rs::tests::inspect_maps_namespaces_to_tables` |
| TC-005 | Empty directory, then a directory whose `data` subdirectory holds a WAL file | Empty table list for the first, tables from `data` for the second | `backend/src/pithos.rs::tests::inspect_empty_directory_and_data_subdirectory_fallback` |
| TC-006 | Single file path `store.WAL` (upper-case extension) used as the root | Treated as one store named `store` | `backend/src/pithos.rs::tests::inspect_single_wal_file_and_extension_case` |
| TC-007 | Directory also holding `notes.txt`, a subdirectory named `x.wal`, and a nested `deep/kv.wal` | Only top-level regular `*.wal` files become stores | `backend/src/pithos.rs::tests::inspect_ignores_foreign_files` |
| TC-008 | `SELECT * FROM "kv/app"` over three keys, one binary value | Rows ascend by key, text values are strings, the binary value is the marker `[BLOB 3 bytes]` | `backend/src/pithos.rs::tests::select_star_returns_ordered_rows_and_blob_marker` |
| TC-009 | `WHERE key = 'user:1'` for a present and an absent key | One row for the present key, zero rows and no error for the absent key | `backend/src/pithos.rs::tests::where_key_equals` |
| TC-010 | `WHERE key LIKE 'user:%'`, `LIKE 'a_b%'`, `LIKE '%'` | Prefix scan results, underscore treated literally, empty prefix returns all keys | `backend/src/pithos.rs::tests::where_key_like_prefix` |
| TC-011 | `SELECT value, key`, lower-case keywords, newlines, trailing semicolon, quoted name containing `""`, string containing `''` | Projection order honored and all spellings accepted | `backend/src/pithos.rs::tests::projections_case_quotes_and_semicolon` |
| TC-012 | Five keys with requested limits 0, 2 and 5000 and SQL `LIMIT 1` and `LIMIT 0` | Cap clamps to 1..1000, `truncated` true only when the cap stopped the result, SQL `LIMIT` never sets it | `backend/src/pithos.rs::tests::limit_and_row_cap_semantics` |
| TC-013 | Frame whose last byte was removed | Error that names the file and mentions the truncated entry | `backend/src/pithos.rs::tests::torn_tail_is_rejected` |
| TC-014 | Payload byte flipped after the CRC was computed | Checksum mismatch error | `backend/src/pithos.rs::tests::crc_mismatch_is_rejected` |
| TC-015 | File starting with a wrong magic such as `XXXX` | Invalid magic error | `backend/src/pithos.rs::tests::bad_magic_is_rejected` |
| TC-016 | Header declaring a payload larger than 4 MiB | Oversized entry error, no allocation of that size | `backend/src/pithos.rs::tests::oversized_declared_length_is_rejected` |
| TC-017 | Valid CRC frames with a short record, inconsistent lengths, non-UTF-8 namespace, non-UTF-8 key, empty namespace, empty key, unknown op, delete carrying a value | Each yields a corruption error | `backend/src/pithos.rs::tests::malformed_records_are_rejected` |
| TC-018 | File one byte larger than the size cap passed to the internal loader | Error naming the limit | `backend/src/pithos.rs::tests::oversized_file_is_rejected` |
| TC-019 | `DELETE`, `INSERT`, `DROP`, two statements, `JOIN`, `ORDER BY`, `GROUP BY`, empty text, stray characters, unterminated string, unterminated quoted name, `LIMIT abc`, `LIKE` without trailing `%`, `LIKE` with inner `%` | Every statement returns `Unsupported` and the message shows the supported grammar | `backend/src/pithos.rs::tests::rejects_unsupported_statements` |
| TC-020 | Query against table `"kv/missing"` and against an unknown column | `Unsupported` error that lists the available tables or the valid columns | `backend/src/pithos.rs::tests::unknown_table_and_column_errors` |
| TC-021 | Run `inspect` and several queries, comparing file bytes, modification time and directory listing before and after | Nothing changes and no file is created | `backend/src/pithos.rs::tests::read_only_has_no_side_effects` |
| TC-022 | Query, append a frame with the test encoder, query again | Second query sees the new record because each request re-reads the file | `backend/src/pithos.rs::tests::each_request_rereads_the_log` |
| TC-023 | Root path that does not exist | `Database` error and the path is not created | `backend/src/pithos.rs::tests::missing_root_errors_without_creating_anything` |

## Boundary & Failure Cases

- Empty WAL file: zero frames decode to an empty store and produce no tables.
- Frame boundary arithmetic: payload length of exactly 4 MiB is accepted by the length check, one byte more is rejected before any allocation.
- Header that is shorter than twelve bytes at the tail, and a payload that is shorter than its declared length.
- Namespaces and keys containing slashes, quotes, spaces and non-ASCII text map to table names and match through quoted identifiers.
- A value that is invalid UTF-8 never panics and never produces lossy text.
- A directory read race: a file vanishing between listing and reading yields a `Database` error rather than a panic.
- Case-insensitive keywords, and a table name whose store part contains `.` or `-` (for example `external-kv/ns`).

## Regression Scope

- `backend/src/lib.rs`: the `Backend` dispatch, `ApiError::Unsupported` status mapping and the unchanged SQLite handlers (covered by `backend/tests/api_http.rs`).
- JSON contracts consumed by `frontend/app/page.tsx` (`Schema`, `QueryResponse`) must stay byte-compatible; the HTTP tests assert the exact field names.
- The golden fixtures must be regenerated with the real `polydb` binary if the PITHOS WAL magic or frame layout ever changes. A failing golden test is the signal that the format contract moved.
