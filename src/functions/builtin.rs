use crate::context::ExecutionContext;
use crate::expr::{interpolate, interpolate_value};
use anyhow::Context as _;
use serde_json::{json, Value};
use std::collections::HashMap;

pub async fn lookup(
    inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let input_tpl = inputs["input"].as_str().context("input required")?;
    let input = interpolate(input_tpl, ctx)?;
    let table_yaml = inputs["table"].as_mapping().context("table required")?;
    let table_json: Value = serde_json::to_value(table_yaml)?;
    let table = interpolate_value(&table_json, ctx)?;
    let result = table.as_object().and_then(|m| {
        m.iter()
            .find(|(k, _)| k.as_str() == input)
            .map(|(_, v)| v.clone())
    });
    match result {
        Some(v) => Ok(json!({"output": v})),
        None => {
            if let Some(default_yaml) = inputs.get("default") {
                let default_json: Value = serde_json::to_value(default_yaml)?;
                let default_val = interpolate_value(&default_json, ctx)?;
                Ok(json!({"output": default_val}))
            } else {
                anyhow::bail!("lookup: input {input:?} not found in table and no default provided")
            }
        }
    }
}

pub async fn jq(
    inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let input_id_tpl = inputs["input"]
        .as_str()
        .context("input must be a string (step id)")?;
    let input_id = interpolate(input_id_tpl, ctx)?;
    let expr_tpl = inputs["expr"].as_str().context("expr must be a string")?;
    let expr = interpolate(expr_tpl, ctx)?;
    let input_val = ctx.outputs.get(&input_id).cloned().unwrap_or(Value::Null);
    let result = run_jq(&expr, input_val)?;
    Ok(json!({"output": result}))
}

fn run_jq(expr: &str, input: Value) -> anyhow::Result<Value> {
    use jaq_core::load::{Arena, File, Loader};
    use jaq_core::{data, unwrap_valr, Compiler, Ctx, Vars};
    use jaq_json::{read, Val};

    let input_bytes = serde_json::to_vec(&input)?;
    let input_val =
        read::parse_single(&input_bytes).map_err(|e| anyhow::anyhow!("jq input: {e}"))?;

    let program = File {
        code: expr,
        path: (),
    };
    let defs = jaq_core::defs()
        .chain(jaq_std::defs())
        .chain(jaq_json::defs());
    let funs = jaq_core::funs()
        .chain(jaq_std::funs())
        .chain(jaq_json::funs());

    let loader = Loader::new(defs);
    let arena = Arena::default();
    let modules = loader
        .load(&arena, program)
        .map_err(|e| anyhow::anyhow!("jq load: {} error(s)", e.len()))?;
    let filter = Compiler::default()
        .with_funs(funs)
        .compile(modules)
        .map_err(|e| anyhow::anyhow!("jq compile: {} error(s)", e.len()))?;

    let ctx = Ctx::<data::JustLut<Val>>::new(&filter.lut, Vars::new([]));
    let mut results: Vec<_> = filter.id.run((ctx, input_val)).map(unwrap_valr).collect();

    fn to_json(v: Val) -> anyhow::Result<Value> {
        serde_json::from_str(&v.to_string()).map_err(|e| anyhow::anyhow!("{e}"))
    }

    match results.len() {
        0 => Ok(Value::Null),
        1 => to_json(results.remove(0).map_err(|e| anyhow::anyhow!("{e:?}"))?),
        _ => {
            let vals: anyhow::Result<Vec<Value>> = results
                .into_iter()
                .map(|r| r.map_err(|e| anyhow::anyhow!("{e:?}")).and_then(to_json))
                .collect();
            Ok(Value::Array(vals?))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ExecutionContext;
    use serde_json::json;

    fn empty_ctx() -> ExecutionContext {
        ExecutionContext {
            payload: json!({"repository": {"name": "mongodb-atlas-cli"}}),
            outputs: std::collections::HashMap::new(),
            inputs: std::collections::HashMap::new(),
        }
    }

    fn yaml_inputs(s: &str) -> HashMap<String, serde_yaml::Value> {
        serde_yaml::from_str(s).unwrap()
    }

    #[tokio::test]
    async fn lookup_returns_scalar_value() {
        let inputs = yaml_inputs(
            r#"
input: "{payload.repository.name}"
table:
  mongodb-atlas-cli:
    component: AtlasCLI
    fix_version_name: next-atlascli-release
  mongodb-atlas-local:
    component: local-atlas-experience
    fix_version_name: next-atlas-local-release
"#,
        );
        let result = lookup(&inputs, &empty_ctx()).await.unwrap();
        assert_eq!(result["output"]["component"], "AtlasCLI");
        assert_eq!(
            result["output"]["fix_version_name"],
            "next-atlascli-release"
        );
    }

    #[tokio::test]
    async fn lookup_missing_key_uses_default() {
        let inputs = yaml_inputs(
            r#"
input: unknown-repo
table:
  known-repo:
    component: KnownComponent
default:
  component: DefaultComponent
  fix_version_name: default-release
"#,
        );
        let result = lookup(&inputs, &empty_ctx()).await.unwrap();
        assert_eq!(result["output"]["component"], "DefaultComponent");
        assert_eq!(result["output"]["fix_version_name"], "default-release");
    }

    #[tokio::test]
    async fn lookup_missing_key_no_default_errors() {
        let inputs = yaml_inputs(
            r#"
input: unknown-repo
table:
  known-repo:
    component: KnownComponent
"#,
        );
        let result = lookup(&inputs, &empty_ctx()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn jq_identity() {
        let v = run_jq(".", json!({"a": 1})).unwrap();
        assert_eq!(v, json!({"a": 1}));
    }

    #[test]
    fn jq_field_access() {
        let v = run_jq(".a", json!({"a": 42})).unwrap();
        assert_eq!(v, json!(42));
    }

    #[test]
    fn jq_scan_finds_jira_key() {
        let v = run_jq(
            r#"first(.comments[] | select(.body | test("was created for internal tracking")) | .body | scan("CLOUDP-[0-9]+"))"#,
            json!({"comments": [
                {"body": "some unrelated comment mentioning CLOUDP-9999"},
                {"body": "Thanks for opening this issue. The ticket [CLOUDP-1234](https://jira.mongodb.org/browse/CLOUDP-1234) was created for internal tracking."}
            ]}),
        )
        .unwrap();
        assert_eq!(v, json!("CLOUDP-1234"));
    }

    #[test]
    fn jq_empty_result_is_null() {
        let v = run_jq("empty", json!(null)).unwrap();
        assert_eq!(v, Value::Null);
    }

    #[test]
    fn jq_multiple_outputs_become_array() {
        let v = run_jq(".[]", json!([1, 2, 3])).unwrap();
        assert_eq!(v, json!([1, 2, 3]));
    }

    #[test]
    fn jq_object_output_returned_directly() {
        let v = run_jq(
            r#"first(.comments[] | select(.body | test("was created for internal tracking")) | .body | scan("CLOUDP-[0-9]+")) | {key: .}"#,
            json!({"comments": [{"body": "The ticket [CLOUDP-1234](https://jira.mongodb.org/browse/CLOUDP-1234) was created for internal tracking."}]}),
        ).unwrap();
        assert_eq!(v, json!({"key": "CLOUDP-1234"}));
    }
}
