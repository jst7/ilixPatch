# ilixpatch-history

Local request/response history for ilixPatch (decision D22). Every request sent
from the desktop app is recorded in a SQLite database outside any project repo,
so it is never committed.

## Files

| File | Location (app) |
| --- | --- |
| Database | `<app_data_dir>/history.sqlite3` (+ `-wal`/`-shm`) |
| Settings | `<app_config_dir>/history.toml` (next to `projects.toml`) |

On macOS both dirs are `~/Library/Application Support/<bundle id>/`.

SQLite is compiled in (`rusqlite` with `bundled`), so no system library is needed.
The database uses WAL and `auto_vacuum = INCREMENTAL`.

## Schema

- `entries`: one small row per request (`id`, `timestamp` ms, `name`, `method`,
  `url`, `status`, `elapsed_ms`, `size_bytes`, `error`, `project_path`,
  `request_path`, `environment`). Indexed on `timestamp`. Listing reads only this.
- `payloads` (1:1, `ON DELETE CASCADE`): `request` / `resolved_request` JSON,
  `response_meta` JSON (`status`, `status_text`, `headers`), `body` BLOB,
  `body_compressed` (deflate via pure-Rust `miniz_oxide`, for bodies > 4 KB),
  `body_truncated`, `tests` JSON, `logs` JSON.

`PRAGMA user_version` holds the schema version for future migrations.

## Settings (defaults)

```toml
enabled = true
max_days = 30            # 0 = keep forever (null in the API)
max_size_mb = 200        # 0 = unlimited   (null in the API)
save_response_body = true
max_body_kb = 1024       # larger bodies are truncated on a UTF-8 boundary
```

A missing or unreadable file means defaults.

## Retention

`HistoryStore::prune` deletes entries older than `max_days`, then the oldest
entries while the live data size (`(page_count - freelist_count) * page_size`)
exceeds `max_size_mb`, then runs `PRAGMA incremental_vacuum` and a WAL
checkpoint so the space goes back to the OS. It runs:

- on app start (background thread),
- every 50 inserts or ~4 MB of inserted payload,
- immediately when settings change (`history_set_settings`).

## API

```rust
let store = HistoryStore::open(&db_path, &settings_path)?; // Clone + Send + Sync
store.record(new_entry)?;            // -> Option<i64>; None when disabled
store.list(Some("users"), None, 50)?; // newest first, keyset pagination on id
store.get(id)?; store.delete(&[id])?; store.clear()?;
store.stats()?; store.settings(); store.set_settings(s)?;
```

`record` does blocking I/O; call it via `spawn_blocking` (the app has
`history_commands::record_in_background`).
