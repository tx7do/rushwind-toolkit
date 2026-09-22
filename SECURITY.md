# Security Policy

## Supported versions

The toolkit tracks `main`; only the latest commit is supported.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting
(<https://github.com/tx7do/rushwind-toolkit/security/advisories/new>)
rather than a public issue.

Scope notes: the CLI writes files only inside the repository or
directory it is pointed at (`--repo` / `--dir`), spawns processes only
for `cargo`, `git`, and the repo's own testbed binaries, and never
starts or touches docker containers (the differential rig's container
discipline is enforced in code, not just documented).
