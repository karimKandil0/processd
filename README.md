# processd

A declarative, reconciliation-driven init system for Linux — PID 1 that continuously converges actual system state toward declared desired state.

The Kubernetes reconciliation loop, brought down to PID 1 on a single machine.

## The Idea

Every existing init system — runit, s6, openrc, dinit, even systemd — is **imperative**: you tell it what to do and in what order. processd inverts this. You declare desired state in TOML; processd continuously reconciles actual state against it. A service dies? That's drift — processd corrects it. Config changes? processd diffs old vs new and applies only what changed.

The gap it fills: the NixOS-without-systemd ecosystem (Finix, sixos, NixNG) generates imperative init artifacts at boot from declarative config — none do **continuous runtime reconciliation**. processd sits in the empty cell: *continuous + declarative + PID 1*.

Full vision, prior-art analysis, and hard problems are in [`processd.md`](processd.md).

## Design

```toml
[service.postgres]
binary   = "/usr/bin/postgres"
args     = ["-D", "/var/lib/postgres"]
user     = "postgres"
wants    = ["network", "filesystem.var"]
provides = ["database"]
restart  = "on-failure"
```

- **Dependency graph inferred from `wants`/`provides`** — you never write startup order, you express dependencies and processd figures out sequencing
- **Desired state is the only control surface** — no imperative stop/start; change the declaration and let it reconcile
- **Event-driven** — `inotify` watches config, `pidfd` + `epoll` watch process death; the loop only wakes when something actually changes
- **Readiness probes** — a process being alive ≠ a service being healthy; dependents wait on readiness, not liveness

## Status

Early development. Cargo workspace:

- `processd/` — the init daemon (PID 1)
- `processctl/` — control & query CLI (`processctl status`)
- `processd-core/` — shared library

Docs: [`processd.md`](processd.md) (vision & braindump), [`implementation.md`](implementation.md), [`docs/`](docs/)

## Building

```bash
cargo build --release

# or via the Nix flake
nix develop
```

Rust, using the `nix` crate for Unix primitives (`process`, `signal`, `user`, `mount`, `event`, `fs`), `serde` + `toml` for configuration, `clap` for the CLI.

## Why Rust

PID 1 must never crash — the kernel panics if it does. Rust's memory safety eliminates the bug class most likely to take down an init system, an argument the lightweight C inits (runit, s6) can't make.

---

*Started May 2026, Cairo.*
