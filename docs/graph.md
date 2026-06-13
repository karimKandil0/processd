# Dependency Graph — `processd-core/src/graph.rs`

## What it does

Takes a parsed `SystemConfig` (the full set of declared services) and produces a `DependencyGraph` — a validated, cycle-free adjacency list that describes which services must start before which others.

Also provides `topological_sort`, which turns that graph into an ordered startup sequence.

---

## Why capabilities, not service names

Services don't `want` other services by name. They `want` capabilities:

```toml
[service.postgres]
provides = ["database"]

[service.api]
wants = ["database"]
```

`api` doesn't know or care that `postgres` is what provides `database`. You could swap in `mysql` and `api`'s config stays unchanged. The graph layer resolves capabilities to service names internally.

---

## `DependencyGraph`

```rust
pub struct DependencyGraph {
    pub edges: HashMap<String, Vec<String>>,
}
```

An adjacency list. `edges["api"] = ["postgres"]` means `api` depends on `postgres`. Edges point from dependent to dependency.

---

## `build_dependency_graph`

Three steps:

### 1. Build the capability map

Iterates all services and builds `capability → service_name`. For example:

```
"database" → "postgres"
"network"  → "networkd"
```

### 2. Resolve `wants` into edges

For each service, looks up each entry in its `wants` list through the capability map. If a capability has no provider, returns `ConfigError::UnknownDependency` immediately — you can't reference something that doesn't exist.

### 3. Cycle detection (DFS)

Runs a depth-first search over the resolved edges. Each node is in one of three states:

| State | Meaning |
|---|---|
| `Unvisited` | Not yet explored |
| `InProgress` | Currently on the DFS call stack |
| `Done` | Fully explored, no cycle found |

If the DFS reaches a node that is already `InProgress`, it has found a back-edge — a cycle. Returns `ConfigError::CycleDetected` with the name of the node involved.

Example cycle: `A wants B`, `B wants A`. DFS marks A as `InProgress`, descends into B, tries to visit A again, finds `InProgress` → error.

---

## `topological_sort`

Implements Kahn's algorithm to produce a linear startup order where every dependency appears before the services that need it.

### How Kahn's works

1. **Compute in-degrees** — for each node, count how many other nodes depend on it. A node with in-degree 0 has no dependents; nothing needs to finish before it starts.

2. **Seed the queue** — all zero-in-degree nodes can start immediately. Sorted alphabetically so output is deterministic regardless of `HashMap` iteration order.

3. **Process the queue** — pop a node, add it to the result, then decrement the in-degree of every node it depends on. When a node's in-degree hits 0, all its dependents are already scheduled, so it joins the queue.

The result is a sequence where if `api` depends on `postgres`, `postgres` always appears earlier in the list.

### Example

Given:

```toml
[service.postgres]
provides = ["database"]

[service.redis]
provides = ["cache"]

[service.api]
wants    = ["database", "cache"]
provides = ["http"]

[service.worker]
wants = ["database"]
```

Topological sort produces something like:

```
["postgres", "redis", "worker", "api"]
```

`postgres` and `redis` before `api`, `postgres` before `worker`.

---

## Errors

| Error | When |
|---|---|
| `ConfigError::UnknownDependency` | A service `wants` a capability nobody `provides` |
| `ConfigError::CycleDetected` | Two or more services form a circular dependency |
