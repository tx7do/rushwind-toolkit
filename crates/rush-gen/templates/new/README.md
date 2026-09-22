# @@NAME@@

A [RushWind](https://github.com/tx7do/rushwind) service scaffolded by
[rushwind-toolkit](https://github.com/tx7do/rushwind-toolkit).

## Run

```shell
cargo run
```

Then `curl http://<printed-endpoint>/health` and `/wired`, and try the
storage-backed CRUD edge at `/items` (`POST` a JSON `{"name": "..."}`,
then `GET /items`).

## Layout

- `src/main.rs` — one YAML document assembles storage, the CRUD edge,
  and the HTTP server under the RushWind lifecycle.
- Storage starts on the memory engine; swap `engine:` in `CONFIG` (or
  move the document to a file and use `Bootstrap::from_yaml_path`) when
  you outgrow it.

## Next steps

- `rush gen entity` (from [rushwind-toolkit](https://github.com/tx7do/rushwind-toolkit))
  scaffolds a full CRUD domain into a rushwind-admin style contract-driven
  service.
