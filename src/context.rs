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
    /// Set when entering on_error: the id (or func name) of the step that failed.
    pub error_step: Option<String>,
    /// Set when entering on_error: the error message from the failed step.
    pub error_message: Option<String>,
}

impl ExecutionContext {
    pub fn new(payload: serde_json::Value) -> Self {
        Self {
            payload,
            outputs: HashMap::new(),
            inputs: HashMap::new(),
            error_step: None,
            error_message: None,
        }
    }

    pub fn set_error(&mut self, step: String, message: String) {
        self.error_step = Some(step);
        self.error_message = Some(message);
    }
}
