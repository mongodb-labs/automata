use crate::context::ExecutionContext;
use regex::Regex;
use std::sync::OnceLock;

static PATH_RE: OnceLock<Regex> = OnceLock::new();

fn path_re() -> &'static Regex {
    PATH_RE.get_or_init(|| Regex::new(r"\{([\w.]+)\}").unwrap())
}

/// Recursively interpolate all string values inside a JSON value.
pub fn interpolate_value(
    v: &serde_json::Value,
    ctx: &ExecutionContext,
) -> anyhow::Result<serde_json::Value> {
    match v {
        serde_json::Value::String(s) => Ok(serde_json::Value::String(interpolate(s, ctx)?)),
        serde_json::Value::Array(arr) => {
            let out: anyhow::Result<Vec<_>> =
                arr.iter().map(|v| interpolate_value(v, ctx)).collect();
            Ok(serde_json::Value::Array(out?))
        }
        serde_json::Value::Object(map) => {
            let out: anyhow::Result<serde_json::Map<_, _>> = map
                .iter()
                .map(|(k, v)| interpolate_value(v, ctx).map(|v| (k.clone(), v)))
                .collect();
            Ok(serde_json::Value::Object(out?))
        }
        other => Ok(other.clone()),
    }
}

/// Replace all `{path}` spans in `template` with values resolved from `ctx`.
/// Use `{{` / `}}` to emit a literal `{` / `}` without triggering interpolation.
pub fn interpolate(template: &str, ctx: &ExecutionContext) -> anyhow::Result<String> {
    // Shield {{ and }} before regex runs, then restore them after.
    const L: &str = "\x00L";
    const R: &str = "\x00R";
    let escaped = template.replace("{{", L).replace("}}", R);
    let mut result = escaped.clone();
    for cap in path_re().captures_iter(&escaped) {
        let span = &cap[0]; // e.g. "{payload.pull_request.title}"
        let path = &cap[1]; // e.g. "payload.pull_request.title"
        let value = resolve(path, ctx)?;
        result = result.replacen(span, &value, 1);
    }
    Ok(result.replace(L, "{").replace(R, "}"))
}

/// Resolve a dotted path like "payload.pull_request.title" against the context.
pub fn resolve(path: &str, ctx: &ExecutionContext) -> anyhow::Result<String> {
    let mut parts = path.splitn(2, '.');
    let root = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("");

    let node = match root {
        "payload" => resolve_json(rest, &ctx.payload),
        "inputs" => {
            return ctx
                .inputs
                .get(rest)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("input not found: {rest}"));
        }
        "env" => {
            anyhow::ensure!(
                rest.starts_with("AUTOMATA_"),
                "env var must be prefixed with AUTOMATA_: {rest}"
            );
            return std::env::var(rest).map_err(|_| anyhow::anyhow!("env var not set: {rest}"));
        }
        step_id => {
            let outputs = ctx
                .outputs
                .get(step_id)
                .ok_or_else(|| anyhow::anyhow!("no outputs for step: {step_id}"))?;
            resolve_json(rest, outputs)
        }
    };

    node.ok_or_else(|| anyhow::anyhow!("path not found: {path}"))
}

fn resolve_json(path: &str, root: &serde_json::Value) -> Option<String> {
    let mut node = root;
    for key in path.split('.') {
        if key.is_empty() {
            break;
        }
        node = node.get(key)?;
    }
    match node {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> ExecutionContext {
        ExecutionContext {
            payload: json!({
                "repository": { "name": "mongodb-atlas-cli" },
                "pull_request": {
                    "title": "Fix bug",
                    "head": { "ref": "fix/my-branch" },
                    "comments_url": "https://api.github.com/repos/mongodb/mongodb-atlas-cli/issues/1/comments"
                },
                "action": "opened",
                "sender": { "login": "alice" }
            }),
            outputs: std::collections::HashMap::new(),
            inputs: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn interpolate_single_path() {
        let result = interpolate("{payload.repository.name}", &ctx()).unwrap();
        assert_eq!(result, "mongodb-atlas-cli");
    }

    #[test]
    fn interpolate_in_string() {
        let result = interpolate(
            "[{payload.repository.name}] {payload.pull_request.title}",
            &ctx(),
        )
        .unwrap();
        assert_eq!(result, "[mongodb-atlas-cli] Fix bug");
    }

    #[test]
    fn interpolate_step_output() {
        let mut c = ctx();
        c.outputs.insert(
            "ticket".to_string(),
            json!({"url": "https://jira.mongodb.org/browse/CLOUDP-123", "key": "CLOUDP-123"}),
        );
        let result = interpolate("Jira ticket: {ticket.url}", &c).unwrap();
        assert_eq!(
            result,
            "Jira ticket: https://jira.mongodb.org/browse/CLOUDP-123"
        );
    }

    #[test]
    fn interpolate_input() {
        let mut c = ctx();
        c.inputs.insert("channel".to_string(), "C12345".to_string());
        let result = interpolate("{inputs.channel}", &c).unwrap();
        assert_eq!(result, "C12345");
    }

    #[test]
    fn interpolate_missing_path_returns_error() {
        let result = interpolate("{payload.nonexistent.field}", &ctx());
        assert!(result.is_err());
    }

    #[test]
    fn env_resolves_with_automata_prefix() {
        std::env::set_var("AUTOMATA_MY_SECRET", "hunter2");
        let result = interpolate("{env.AUTOMATA_MY_SECRET}", &ctx()).unwrap();
        assert_eq!(result, "hunter2");
        std::env::remove_var("AUTOMATA_MY_SECRET");
    }

    #[test]
    fn env_missing_var_returns_error() {
        std::env::remove_var("AUTOMATA_DEFINITELY_NOT_SET");
        let result = interpolate("{env.AUTOMATA_DEFINITELY_NOT_SET}", &ctx());
        assert!(result.is_err());
    }

    #[test]
    fn env_without_prefix_returns_error() {
        let result = interpolate("{env.HOME}", &ctx());
        assert!(result.is_err());
    }

    #[test]
    fn interpolate_no_expressions_is_passthrough() {
        let result = interpolate("plain string", &ctx()).unwrap();
        assert_eq!(result, "plain string");
    }

    #[test]
    fn double_braces_escape_to_literal_braces() {
        let result = interpolate("{{key: .}}", &ctx()).unwrap();
        assert_eq!(result, "{key: .}");
    }

    #[test]
    fn double_braces_mixed_with_interpolation() {
        // {{...}} produces literal braces; {path} interpolates
        let result = interpolate("name={payload.repository.name} jq={{key: .}}", &ctx()).unwrap();
        assert_eq!(result, "name=mongodb-atlas-cli jq={key: .}");
    }

    #[test]
    fn double_open_brace_only() {
        let result = interpolate("{{", &ctx()).unwrap();
        assert_eq!(result, "{");
    }

    #[test]
    fn resolve_nested_path() {
        let c = ctx();
        assert_eq!(
            resolve("payload.pull_request.head.ref", &c).unwrap(),
            "fix/my-branch"
        );
    }

    #[test]
    fn interpolate_value_string() {
        let v = json!("{payload.repository.name}");
        let result = interpolate_value(&v, &ctx()).unwrap();
        assert_eq!(result, json!("mongodb-atlas-cli"));
    }

    #[test]
    fn interpolate_value_nested_object() {
        let mut c = ctx();
        c.outputs
            .insert("find".to_string(), json!({"key": "CLOUDP-123"}));
        let v = json!({"resolution": {"name": "Fixed"}, "key": "{find.key}"});
        let result = interpolate_value(&v, &c).unwrap();
        assert_eq!(
            result,
            json!({"resolution": {"name": "Fixed"}, "key": "CLOUDP-123"})
        );
    }

    #[test]
    fn interpolate_value_array_of_objects() {
        let mut c = ctx();
        c.outputs.insert("item".to_string(), json!({"id": "99"}));
        let v = json!([{"id": "{item.id}"}]);
        let result = interpolate_value(&v, &c).unwrap();
        assert_eq!(result, json!([{"id": "99"}]));
    }

    #[test]
    fn interpolate_value_non_string_passthrough() {
        let v = json!({"count": 42, "active": true, "data": null});
        let result = interpolate_value(&v, &ctx()).unwrap();
        assert_eq!(result, v);
    }
}
