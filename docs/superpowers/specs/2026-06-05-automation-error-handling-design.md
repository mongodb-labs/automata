# Automation Error Handling Design

**Date:** 2026-06-05  
**Status:** Approved

## Problem

When a step in an automation fails (API error, bad expression, missing field), execution silently stops. Operators are notified via tracing/logs, but the affected user (e.g. the PR author) has no visibility into what went wrong or why.

## Goal

Give automation authors an opt-in mechanism to run steps when an automation fails, so they can surface errors to affected users via GitHub comments, Jira tickets, or any other supported function.

## YAML Schema

`on_error:` is an optional sibling of `then:` on a pipeline entry. It accepts the same step syntax as `then:`.

```yaml
pipeline:
  - given:
      trigger: github
      repos: [org/repo]
    when:
      - event: pull_request
        action: opened
    then:
      - jira.create_issue:
          id: ticket
          summary: "[{payload.repository.name}] {payload.pull_request.title}"
    on_error:
      - github.post_comment:
          body: |
            Automation failed at step `{error.step}`.

            > {error.message}
```

Two interpolation variables are available exclusively inside `on_error:` steps:

| Variable | Value |
|---|---|
| `{error.step}` | The `id` of the failed step, or the function name if no `id` was set |
| `{error.message}` | The error string returned by the failed function |

Using `{error.*}` outside an `on_error:` block resolves as missing (same as any unknown path).

## Data Model

**[src/types.rs](../../../src/types.rs):** One new field on `PipelineEntry`:

```rust
pub struct PipelineEntry {
    pub given: Given,
    pub when: Vec<WhenGroup>,
    pub then: Vec<serde_yaml::Value>,
    pub on_error: Option<Vec<serde_yaml::Value>>,  // new
}
```

**[src/context.rs](../../../src/context.rs):** Two new optional fields on `ExecutionContext`, set only when entering error handling:

```rust
pub struct ExecutionContext {
    pub payload: serde_json::Value,
    pub outputs: HashMap<String, serde_json::Value>,
    pub inputs: HashMap<String, String>,
    pub error_step: Option<String>,     // new
    pub error_message: Option<String>,  // new
}
```

## Execution Flow

**[src/engine.rs](../../../src/engine.rs):** `run_automation` currently propagates step errors up the call stack. The change: catch the first failure, inject error context, and run `on_error` steps.

```
run_automation(entry, payload, clients):
  ctx = ExecutionContext::new(payload)
  for step in entry.then:
    result = execute_step(step, ctx, clients)
    if result is Err(e):
      log error (existing behaviour)
      if entry.on_error is Some(error_steps):
        ctx.error_step    = step.id ?? step.func
        ctx.error_message = e.to_string()
        for error_step in error_steps:
          if execute_step(error_step, ctx, clients) is Err(e2):
            log e2   // best-effort: no recursion
      return   // stop then: chain either way
```

Key invariants:
- `on_error` steps run only on the **first** step failure; the `then:` chain is always aborted.
- `on_error` steps are best-effort: a failure inside them logs but does not recurse.
- The HTTP handler continues to return `200 OK` regardless (existing behaviour).

## Expression Interpolation

**[src/expr.rs](../../../src/expr.rs):** `interpolate_value` already dispatches on the leading path segment (`payload`, `env`, named step outputs). Add `error` as a new top-level namespace resolved from `ctx.error_step` / `ctx.error_message`.

Supported paths:

| Expression | Resolves to |
|---|---|
| `{error.step}` | `ctx.error_step` |
| `{error.message}` | `ctx.error_message` |

Any other `{error.*}` path is treated as missing (returns an interpolation error, same as today).

## Testing

Tests use the existing wiremock harness.

| Scenario | Expected |
|---|---|
| Step fails, no `on_error` | Error logged, nothing else (existing behaviour preserved) |
| Step fails, `on_error` present | Error steps execute; `{error.step}` and `{error.message}` resolve correctly |
| `on_error` step itself fails | Logs the secondary failure, does not recurse or panic |
| `{error.*}` used in a `then:` step | Resolves as missing (no crash) |
| `on_error` with named step outputs | Outputs from `then:` steps completed before the failure are still accessible |
