# processd — Braindump & Vision

> A declarative, reconciliation-driven init system for Linux. PID 1 that continuously converges actual system state toward declared desired state.

-----

## The Core Idea

Every existing init system — runit, s6, openrc, dinit, even systemd — is fundamentally **imperative**. You tell it what to do and in what order. Configuration files are instructions, not declarations.

processd flips this. You declare **desired state**. processd continuously reconciles actual state against it. If a service dies, that’s drift — processd corrects it. If you change the config, processd diffs old vs new and applies only what changed.

This is the **Kubernetes reconciliation loop** brought down to PID 1 on a single machine.

-----

## Prior Art & The Gap

### NixOS-without-systemd ecosystem (active as of 2026)

|Project    |Init Used|Approach                                                      |Gap                        |
|-----------|---------|--------------------------------------------------------------|---------------------------|
|**Finix**  |finit    |NixOS spin, replaces systemd with finit                       |Still imperative at runtime|
|**sixos**  |s6       |nixpkgs-based, replaces NixOS modules with `infuse` combinator|Still imperative at runtime|
|**NixNG**  |various  |Alternative NixOS module system                               |Still imperative at runtime|
|**Cockpit**|systemd  |Web UI for systemd management                                 |Not an init system         |

**The gap:** All of these generate imperative init artifacts at boot from declarative config. None do **continuous runtime reconciliation**. processd does.

### Philosophical parallels

- **Ansible / Chef / Puppet** — declarative + idempotent, but runs on demand and exits. Not continuous.
- **NixOS** — declarative config generates systemd units. Declarativeness stops at boot.
- **Kubernetes** — continuous reconciliation loop, but at cluster/container layer. Way above PID 1.
- **systemd** — closest to PID 1, has dependency graphs, but fundamentally imperative. No desired state model.

processd sits in the empty cell: **continuous + declarative + PID 1**.

-----

## How It Works

### The reconciliation loop

```
loop:
    desired = parse_config("/etc/processd/system.toml")
    actual  = snapshot_process_tree()
    diff    = reconcile(desired, actual)
    apply(diff)
    sleep(interval) OR wake_on_event()
```

In practice, fully event-driven:

- `inotify` watches config file — changes trigger immediate reconciliation
- `pidfd` + `epoll` watches process death — no polling needed
- Loop only wakes when something actually changes

### Config (proposed)

```toml
[service.postgres]
binary   = "/usr/bin/postgres"
args     = ["-D", "/var/lib/postgres"]
user     = "postgres"
wants    = ["network", "filesystem.var"]
provides = ["database"]
restart  = "on-failure"
backoff  = "exponential"

[service.api]
binary   = "/usr/bin/myapp"
wants    = ["database"]
provides = ["api"]
restart  = "always"
```

processd builds the dependency graph from `wants`/`provides`, infers startup order automatically. You never write “start postgres first” — you express the dependency and processd figures out sequencing.

### Desired state is the only control surface

You never imperatively tell processd to “stop” something. You change desired state and let it reconcile. If a service is declared as running and you kill it manually, processd restarts it. To stop it — remove it from the declaration or set `enabled = false`.

-----

## Hard Problems

### Signal safety

PID 1 has severe restrictions on what signal handlers can safely call. Solution: `signalfd` to handle signals from the main epoll loop safely, treating signals as file descriptor events.

### Early boot

Before `/etc` is readable, a minimal hardcoded bootstrap phase runs. The declarative model only kicks in once the system is stable enough to read config.

### Zombie reaping

PID 1 must always reap orphaned children — every process whose parent dies gets reparented to PID 1. Must happen continuously regardless of reconciliation loop state. Use `waitpid(WNOHANG)` in a non-blocking loop integrated with the event loop.

### Readiness vs liveness

A process being alive ≠ a service being healthy. Need readiness probes (is postgres actually accepting connections?) before declaring dependents startable. Same concept as Kubernetes liveness/readiness probes.

### Ordering without imperative instructions

Dependency graph is inferred from `wants`/`provides`. processd won’t start `api` until `database`’s readiness probe passes. No user-specified ordering needed.

-----

## Why Rust

- PID 1 must **never crash** — this is the strongest correctness requirement in systems programming. Kernel panics immediately if PID 1 dies.
- Rust’s memory safety eliminates the class of bugs most likely to cause unexpected crashes.
- `zbus` for D-Bus if needed, `nix` crate for low-level Unix primitives, `tokio` or manual epoll for the event loop.
- Existing lightweight inits (runit, s6) are C — Rust is a genuine differentiator on correctness grounds.

-----

## What Makes It Novel

1. **Continuous reconciliation at PID 1** — nobody has done this. Kubernetes did it for clusters. processd does it for single machines.
1. **Declaration is the only interface** — no imperative commands. State changes only through config changes.
1. **Typed failure** — exit codes, signals, stdout patterns as structured failure reasons, with conditional recovery logic per failure type. No existing init does this.
1. **Built-in observability** — structured logging and a Unix socket state query interface from day one, not bolted on.
1. **Written in Rust** — correctness argument for PID 1 that C-based inits can’t make.

-----

## Interface (proposed)

```bash
# All control happens through config
vim /etc/processd/system.toml

# processd notices via inotify, reconciles automatically

# Query live state
processctl status

# Output:
# postgres  RUNNING  healthy   pid=1234  uptime=3d
# api       RUNNING  healthy   pid=1235  uptime=3d
# redis     STOPPED  desired   -         -
```

-----

## PID 1 Specifics (Linux)

- Kernel executes exactly one process after boot — that’s you
- `SIGKILL` and `SIGSTOP` are silently ignored by the kernel when sent to PID 1 (by design — killing PID 1 = kernel panic)
- All orphaned processes are reparented to PID 1 automatically
- PID 1 is responsible for coordinating orderly shutdown before kernel pulls the plug
- Runs at **ring 3** (userspace) — not a kernel component. Crosses to ring 0 only via syscalls like `fork()`, `execve()`, `waitpid()`, `signalfd()`

-----

## Relevant Syscalls

|Syscall                |What it does                                                                |
|-----------------------|----------------------------------------------------------------------------|
|`fork()`               |Creates child process via `copy_process()`, COW memory pages                |
|`execve()`             |Replaces process image with new binary, preserves PID                       |
|`setuid()` / `setgid()`|Changes process credentials, requires `CAP_SETUID`                          |
|`waitpid()`            |Reaps zombie children                                                       |
|`signalfd()`           |Converts signals to file descriptor events for safe handling                |
|`pidfd_open()`         |Gets a file descriptor representing a process for epoll-based death watching|
|`inotify_init()`       |Watches filesystem for config file changes                                  |
|`epoll_wait()`         |Single event loop watching all of the above                                 |

-----

## Build Plan (rough)

1. **Bootstrap** — minimal PID 1 that doesn’t crash. Reaps zombies, handles `SIGCHLD`.
1. **Config parsing** — read TOML, build dependency graph.
1. **Basic supervision** — spawn declared services, restart on death.
1. **Reconciliation loop** — diff desired vs actual, apply changes.
1. **inotify** — watch config file, trigger reconciliation on change.
1. **pidfd + epoll** — event-driven process death detection.
1. **Readiness probes** — health check before marking service ready.
1. **processctl** — Unix socket query interface for live state.
1. **Typed failure** — structured failure reasons and conditional recovery.
1. **Early boot** — hardcoded bootstrap phase before filesystem is up.

-----

*Started: May 2026. Cairo.*