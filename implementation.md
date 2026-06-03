# processd — Implementation Plan

## Overview

processd is a declarative, reconciliation-driven Linux init system (PID 1) written in Rust. Instead of imperative service management, it continuously reconciles actual process state against declared desired state — the Kubernetes reconciliation loop brought to a single machine.

- **Target:** bare-metal Linux (full system init, including early boot)
- **Event loop:** manual epoll — no tokio in the PID 1 binary
- **Crates:** `processd` (PID 1), `processctl` (CLI client), `processd-core` (shared types + reconciler logic)

---

## Phase 0 — Workspace Scaffold ✅

3 crates (`processd`, `processctl`, `processd-core`), workspace `Cargo.toml` with shared dependency versions, Nix dev shell (`flake.nix`).

---

## Phase 1 — Minimal PID 1

**Goal:** A binary that can be passed as `init=` to the kernel and not cause a kernel panic.

**In `processd`:**
- `main()` mounts essential virtual filesystems via `mount(2)`: `/proc` (procfs), `/sys` (sysfs), `/dev` (devtmpfs)
- Mask signals with `sigprocmask` so they queue to `signalfd`
- Create a `signalfd` watching: `SIGCHLD`, `SIGTERM`, `SIGHUP`
- Minimal `epoll` loop: single fd (the signalfd), blocks forever
- `SIGCHLD` handler: call `waitpid(-1, WNOHANG)` in a loop until `ECHILD` — reaps all zombies
- `SIGTERM` handler: log "shutdown requested", exit 0 (real shutdown in a later phase)
- Emergency shell fallback: if any setup step fails, `execve("/bin/sh", [], [])` rather than kernel panic

**Test:** Boot QEMU VM with `init=/path/to/processd`. Verify process is PID 1, zombies are reaped, system doesn't panic.

---

## Phase 2 — Config Parsing + Dependency Graph

**Goal:** Parse a TOML config into a validated, topologically sorted dependency graph.

**In `processd-core`:**

Types:
- `ServiceConfig` — binary, args, user, wants, provides, restart policy
- `SystemConfig` — collection of `ServiceConfig`s
- `DependencyGraph` — adjacency list built from wants/provides pairs

Functions:
- `parse_config(path: &Path) -> Result<SystemConfig>`
- `build_dependency_graph(config: &SystemConfig) -> Result<DependencyGraph>`
  - Validates all names in `wants` exist as a `provides` somewhere
  - Detects cycles (return `Err`)
- `topological_sort(graph: &DependencyGraph) -> Vec<String>` — startup order

**Test:** Pure `cargo test` unit tests in `processd-core`. No VM needed. Cover cycle detection, unknown deps, correct sort order.

---

## Phase 3 — Basic Service Supervision

**Goal:** Spawn declared services in dependency order; restart them on death.

**In `processd`:**

Types:
- `ServiceState` enum: `Stopped`, `Starting`, `Running { pid }`, `Failed { status, attempts }`
- `ProcessTable`: `HashMap<String, ServiceState>`

Logic:
- `spawn_service(config: &ServiceConfig) -> Result<Pid>`: `fork()` → in child: `setuid/setgid`, `execve()`; in parent: record pid in `ProcessTable`
- On `SIGCHLD`: `waitpid` loop identifies which pid died, looks up service name in `ProcessTable`
- Restart policy: `always` → re-spawn; `on-failure` → re-spawn on non-zero exit only; `never` → leave as `Stopped`
- Exponential backoff: per-service attempt counter, 1s → 2s → 4s → capped at 30s

**Test:** VM boot, declare 2 services (`/bin/sleep 5` or similar), kill one, verify restart with backoff.

---

## Phase 4 — Reconciliation Loop

**Goal:** Replace ad-hoc restart logic with a proper diff-and-apply reconciler.

**In `processd-core`:**

Types:
- `DesiredState` — parsed config: what should be running
- `ActualState` — snapshot of current `ProcessTable`
- `Action` enum: `Start(name)`, `Stop(name)`, `Restart(name)`, `NoOp(name)`

Functions:
- `diff(desired: &DesiredState, actual: &ActualState) -> Vec<Action>`
  - `Start`: in desired, not running in actual
  - `Stop`: running in actual, not in desired
  - `Restart`: config hash changed for a running service
  - `NoOp`: already matches desired state
- `apply(actions: &[Action], process_table: &mut ProcessTable, configs: &SystemConfig)`

**In `processd`:**
- Each loop iteration: snapshot `ProcessTable` → `diff` → `apply`
- Replaces the per-SIGCHLD restart logic from Phase 3

**Test:** Remove a service from config on a live system — verify it stops. Add a new one — verify it starts.

---

## Phase 5 — Event-Driven (inotify + pidfd + epoll)

**Goal:** Zero polling. The event loop wakes only on actual events.

**In `processd`:**
- `inotify_init()` watching `/etc/processd/system.toml` — `IN_CLOSE_WRITE` or `IN_MODIFY` triggers reconciliation
- `pidfd_open(pid)` per spawned service — added to epoll, fires on process death (replaces SIGCHLD-based detection)
- `epoll_wait` watches: `signalfd`, `inotify_fd`, one `pidfd` per running service
- On spawn: add new pidfd to epoll
- On death: close pidfd, remove from epoll, run reconciler

No `sleep()` calls anywhere. The loop only wakes when something actually changes.

**Test:** Modify config file on a running system. Reconciliation should happen in under 100ms.

---

## Phase 6 — Readiness Probes

**Goal:** Dependents only start once their dependencies are actually healthy, not just alive.

**In `processd-core`:**

Types:
- `ReadinessProbe` enum:
  - `None` — ready as soon as process is alive (default)
  - `Exec { command, args }` — exit 0 = ready
  - `Tcp { host, port }` — successful connect = ready
- `ServiceState` gains `Ready` (probe passed) distinct from `Running` (alive, probe pending)

**In `processd`:**
- After spawning a service with a probe: retry probe on a `timerfd`-based loop via epoll
- Reconciler: a `wants` dependency must be `Ready` before a dependent can be started
- Configurable `probe_interval` and `probe_timeout`

**Test:** Service B `wants` A, A has a TCP probe. Verify B does not start until A's port accepts connections.

---

## Phase 7 — processctl (Unix Socket Interface)

**Goal:** Read-only live state query from outside PID 1.

**In `processd`:**
- Unix domain socket at `/run/processd.sock`, added to epoll on startup
- Protocol: 4-byte length prefix + JSON body
- Request type `Status` → serialize `ProcessTable` snapshot → send response

**In `processctl`:**
- `processctl status` — connect to socket, print table:
  ```
  postgres  READY    pid=1234  uptime=3d
  api       RUNNING  pid=1235  uptime=2m
  redis     STOPPED  -         -
  ```
- `clap` for subcommand parsing
- tokio is fine here (client binary, not PID 1)

**Test:** `processctl status` output matches actual `/proc` state.

---

## Phase 8 — Stage 1 / Early Boot (initramfs)

**Goal:** Handle the pre-filesystem phase: mount root, pivot, hand off to Stage 2.

Runs from the initramfs before the real root is mounted.

**In `processd` (feature-flagged or separate `stage1` binary):**
- Read `bootstrap.toml` (embedded in the initramfs at build time)
- Mount sequence: proc/sysfs/devtmpfs first, then real root device to `/mnt/newroot`
- `fsck` root device if needed
- `pivot_root("/mnt/newroot", "/mnt/newroot/oldroot")` or `switch_root`
- `execve` Stage 2 processd (now on the real root) as PID 1

`bootstrap.toml` shape:
```toml
[[mount]]
source = "proc"
target = "/proc"
fstype = "proc"

[[mount]]
source = "/dev/sda1"
target = "/mnt/newroot"
fstype  = "ext4"
options = "ro"
```

**Test:** Boot VM from initramfs, verify switch_root to real root, Stage 2 takes over normally.

---

## Phase 9 — Task Primitive

**Goal:** One-shot and change-triggered jobs alongside long-running services.

**In `processd-core`:**
- `Task` struct: same fields as `ServiceConfig` plus `run: RunPolicy`
- `RunPolicy` enum: `Once`, `OnConfigChange`
- `TaskState` enum: `Pending`, `Running { pid }`, `Done { status }`, `Failed { status }`
- Reconciler: `Once` tasks start if `Pending`, never restart; `OnConfigChange` tasks re-run when their stanza content hash changes

**In `processd`:**
- `ProcessTable` tracks tasks alongside services
- Tasks respect `wants` ordering — won't run until dependencies are `Ready`

**Test:** One-shot migration task that `wants = ["database"]` runs exactly once after the database is ready, does not restart.

---

## Phase 10 — Typed Failure + Conditional Recovery

**Goal:** Structured failure classification with per-reason restart policies.

**In `processd-core`:**
- `FailureReason` enum: `ExitCode(i32)`, `Signal(nix::sys::signal::Signal)`, `Timeout`, `ProbeTimeout`
- `RestartPolicy` extended with per-reason overrides:
  ```toml
  [service.api.restart]
  on-exit-code.1  = "stop"     # permanent error, don't retry
  on-exit-code.2  = "restart"  # transient, retry
  on-signal.segv  = "restart"
  default         = "on-failure"
  ```
- `decide_action(reason: FailureReason, policy: &RestartPolicy) -> Action`
- Reconciler calls `decide_action` instead of the simple restart field from Phase 3

**Test:** Exit code 1 → stops permanently. Exit code 2 → restarts. SIGSEGV → restarts. Verify each.

---

## Testing Strategy

| Level | How | Covers |
|---|---|---|
| Unit tests | `cargo test` in `processd-core` | Reconciler logic, config parsing, dependency graph |
| Container | `docker run` with custom init | Process supervision, fork/exec, zombie reaping |
| VM | QEMU with `init=/path/to/processd` | Full PID 1 behaviour, real kernel interaction |
| VM (fast) | `virtme-ng --init ./target/debug/processd` | Same as above, faster iteration |

---

## Deferred

- Device dependency type (`wants = ["device:/dev/sda"]`) — udev netlink integration
- `mode = "maintenance"` — system-wide maintenance mode
- Structured logging / journal integration
- LUKS/cryptsetup in Stage 1
