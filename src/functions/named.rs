use crate::context::ExecutionContext;
use crate::types::Step;
use anyhow::Context as _;
use glob::glob;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct NamedFunction {
    pub name: String,
    pub inputs: Option<Vec<InputSpec>>,
    pub steps: Vec<serde_yaml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InputSpec {
    pub name: String,
    pub required: Option<bool>,
}

pub fn load_named_functions(dir: &str) -> anyhow::Result<HashMap<String, NamedFunction>> {
    let pattern = format!("{dir}/*.yaml");
    let mut map = HashMap::new();
    for entry in glob(&pattern).context("invalid glob")? {
        let path = entry?;
        let src = std::fs::read_to_string(&path)?;
        let f: NamedFunction = serde_yaml::from_str(&src)
            .with_context(|| format!("parsing {}", path.display()))?;
        map.insert(f.name.clone(), f);
    }
    Ok(map)
}

pub async fn run(
    func_name: &str,
    call_inputs: &HashMap<String, serde_yaml::Value>,
    ctx: &mut ExecutionContext,
    clients: &super::Clients,
) -> anyhow::Result<Value> {
    let functions = load_named_functions("functions/")?;
    let func = functions
        .get(func_name)
        .with_context(|| format!("named function not found: {func_name}"))?;

    // Bind inputs into ctx.inputs
    let saved_inputs = std::mem::take(&mut ctx.inputs);
    for (k, v) in call_inputs {
        ctx.inputs.insert(k.clone(), v.as_str().unwrap_or_default().to_string());
    }

    let mut last_output = serde_json::json!({});
    for raw_step in &func.steps {
        let step = Step::from_yaml(raw_step)?;
        super::execute_step(&step, ctx, clients).await?;
        if let Some(id) = &step.id {
            last_output = ctx.outputs.get(id).cloned().unwrap_or_default();
        }
    }

    ctx.inputs = saved_inputs;
    Ok(last_output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_named_functions_parses_notify_slack() {
        let fns = load_named_functions("functions/").unwrap();
        assert!(fns.contains_key("notify-slack"));
        let f = &fns["notify-slack"];
        assert_eq!(f.steps.len(), 1);
    }
}
