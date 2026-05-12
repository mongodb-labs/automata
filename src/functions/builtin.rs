use crate::context::ExecutionContext;
use anyhow::Context as _;
use serde_json::{json, Value};
use std::collections::HashMap;

pub async fn jq(
    inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &ExecutionContext,
) -> anyhow::Result<Value> {
    let input_id = inputs["input"].as_str().context("input must be a string (step id)")?;
    let expr = inputs["expr"].as_str().context("expr must be a string")?;
    let input_val = ctx.outputs.get(input_id).cloned().unwrap_or(Value::Null);
    let result = run_jq(expr, input_val)?;
    match result {
        Value::Object(_) => Ok(result),
        other => Ok(json!({ "result": other })),
    }
}

fn run_jq(expr: &str, input: Value) -> anyhow::Result<Value> {
    use jaq_interpret::{Ctx, FilterT, ParseCtx, RcIter, Val};

    let mut defs = ParseCtx::new(Vec::new());
    defs.insert_natives(jaq_core::core());
    defs.insert_defs(jaq_std::std());

    let (f, errs) = jaq_parse::parse(expr, jaq_parse::main());
    if !errs.is_empty() {
        anyhow::bail!("jq parse errors: {} error(s)", errs.len());
    }
    let f = f.context("jq parse failed")?;
    let f = defs.compile(f);
    if !defs.errs.is_empty() {
        anyhow::bail!("jq compile errors: {} error(s)", defs.errs.len());
    }

    let jq_inputs = RcIter::new(std::iter::empty::<Result<Val, String>>());
    let val = Val::from(input);
    let run_ctx = Ctx::new([], &jq_inputs);

    let mut results: Vec<_> = f.run((run_ctx, val)).collect();
    match results.len() {
        0 => Ok(Value::Null),
        1 => {
            let v = results.remove(0).map_err(|e| anyhow::anyhow!("{e:?}"))?;
            Ok(Value::from(v))
        }
        _ => {
            let vals: anyhow::Result<Vec<Value>> = results
                .into_iter()
                .map(|r| r.map(Value::from).map_err(|e| anyhow::anyhow!("{e:?}")))
                .collect();
            Ok(Value::Array(vals?))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
            r#"first(.comments[].body | scan("CLOUDP-[0-9]+"))"#,
            json!({"comments": [
                {"body": "some unrelated comment"},
                {"body": "Jira ticket: https://jira.mongodb.org/browse/CLOUDP-1234"}
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
        let v = run_jq(r#"first(.comments[].body | scan("CLOUDP-[0-9]+")) | {key: .}"#,
            json!({"comments": [{"body": "Jira ticket: CLOUDP-1234"}]}),
        ).unwrap();
        assert_eq!(v, json!({"key": "CLOUDP-1234"}));
    }
}
