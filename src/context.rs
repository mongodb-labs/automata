use std::collections::HashMap;

/// Runtime execution context — passed to every step.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// Raw GitHub webhook payload.
    pub payload: serde_json::Value,
    /// Step outputs keyed by step id.
    pub outputs: HashMap<String, serde_json::Value>,
    /// Inputs for named functions (from uses: call site).
    pub inputs: HashMap<String, String>,
}

impl ExecutionContext {
    pub fn new(payload: serde_json::Value) -> Self {
        Self {
            payload,
            outputs: HashMap::new(),
            inputs: HashMap::new(),
        }
    }
}
