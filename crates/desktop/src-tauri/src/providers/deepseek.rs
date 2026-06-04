use serde_json::Value;
use super::adapter::{ChatRequest, ProviderAdapter};

pub struct DeepSeekAdapter;

impl ProviderAdapter for DeepSeekAdapter {
  fn build_request(&self, req: &ChatRequest) -> Value {
    serde_json::json!({
      "model": req.model,
      "messages": [{ "role": "user", "content": req.prompt }],
      "stream": true
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn build_request_contains_model() {
    let adapter = DeepSeekAdapter;
    let value = adapter.build_request(&ChatRequest {
      model: "deepseek-v4-pro".into(),
      prompt: "hi".into(),
    });
    assert_eq!(value["model"], "deepseek-v4-pro");
  }
}
