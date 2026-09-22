# Contributing to rushwind-toolkit

Thanks for your interest. The quick version:

## Dev loop

```shell
cargo test --workspace
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
```

All three gates must pass, matching CI (ubuntu + windows matrix).

## Conventions

- Templates in `crates/rush-gen` must mirror the house patterns of the
  repositories they generate into — the template baseline for
  `rush gen entity` is rushwind-admin's `dict_type` entity chain, and
  template drift should surface here first. When changing a template,
  verify end-to-end: generate an entity on a scratch branch of a real
  rushwind-admin checkout and run `cargo check -p admin-api`.
- Manifest algorithms (`rush manifest`) are byte-faithful ports of the
  shell scripts they replaced; do not change their output format.
- Commits follow the ecosystem style: `type(scope): narrative — details`.

## Reporting issues

Open a GitHub issue with the command, the flags, and (for generation
bugs) the generated file excerpts.
