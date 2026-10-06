## Context

Release builds use `panic = "abort"`; the per-connection task model means most failures can be confined to one connection if they are returned as errors instead of panics.

## Goals / Non-Goals

**Goals:** no input from a client can stop the broker; memory stays bounded by default; the service comes back by itself after a failure.
**Non-Goals:** persistence of in-flight messages across a restart (the broker stays in-memory by design).

## Decisions

To be designed in 0.4.0: fuzz targets and corpus, memory default formula, recovery-action values and options, launchd integration.

## Risks / Trade-offs

- [A default memory limit changes behaviour for users with very large backlogs] → Documented; `max_memory_mb = 0` keeps the current unlimited behaviour.
