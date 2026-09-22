# @@NAME@@

A [RushWind](https://github.com/tx7do/rushwind) service scaffolded by
[rushwind-toolkit](https://github.com/tx7do/rushwind-toolkit), backed by
PostgreSQL.

## Prerequisites

A reachable PostgreSQL instance. Quick start with docker:

```shell
docker run -d --name @@NAME@@-pg \
  -e POSTGRES_USER=@@NAME@@ -e POSTGRES_PASSWORD=@@NAME@@ \
  -e POSTGRES_DB=@@NAME@@ -p 5432:5432 postgres:16-alpine
```

## Run

```shell
cargo run
```

Tables are created on startup (`migrate_create`, create-if-missing). The
DSN lives in the `storage.settings.url` node of `CONFIG` in
`src/main.rs`; point it at your instance or move the document to a file
and use `Bootstrap::from_yaml_path`.

Then `curl http://<printed-endpoint>/health` and `/wired`, and try the
storage-backed CRUD edge at `/items` (`POST` a JSON `{"name": "..."}`,
then `GET /items`).

## Next steps

- `rush gen entity` (from [rushwind-toolkit](https://github.com/tx7do/rushwind-toolkit))
  scaffolds a full CRUD domain into a rushwind-admin style contract-driven
  service.
