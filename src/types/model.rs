use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AvailableModel {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub max_input_tokens: Option<u64>,
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub capabilities: Option<Value>,
    #[serde(default)]
    pub overflow: bool,
    #[serde(default)]
    pub hard_limit: Option<u64>,
}

impl AvailableModel {
    pub fn openai_value(&self) -> Value {
        let mut value = Map::from_iter([
            ("id".to_string(), json!(self.id)),
            ("object".to_string(), json!("model")),
            ("created".to_string(), json!(0)),
            ("owned_by".to_string(), json!("anthropic")),
        ]);
        if let Some(display_name) = &self.display_name {
            value.insert("display_name".to_string(), json!(display_name));
        }
        if let Some(created_at) = &self.created_at {
            value.insert("created_at".to_string(), json!(created_at));
        }
        if let Some(max_input_tokens) = self.max_input_tokens {
            value.insert("max_input_tokens".to_string(), json!(max_input_tokens));
        }
        if let Some(max_tokens) = self.max_tokens {
            value.insert("max_tokens".to_string(), json!(max_tokens));
        }
        if let Some(capabilities) = &self.capabilities {
            value.insert("capabilities".to_string(), capabilities.clone());
        }
        if self.overflow {
            value.insert("overflow".to_string(), json!(true));
        }
        if let Some(hard_limit) = self.hard_limit {
            value.insert("hard_limit".to_string(), json!(hard_limit));
        }
        Value::Object(value)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WebModelConfig {
    pub model: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub inactive: bool,
    #[serde(default)]
    pub overflow: bool,
    #[serde(default)]
    pub hard_limit: Option<u64>,
    #[serde(default)]
    pub capabilities: Option<Value>,
}

impl WebModelConfig {
    fn into_available(self) -> Option<AvailableModel> {
        (!self.inactive && !self.model.trim().is_empty()).then_some(AvailableModel {
            id: self.model,
            display_name: self.name,
            overflow: self.overflow,
            hard_limit: self.hard_limit,
            capabilities: self.capabilities,
            ..Default::default()
        })
    }
}

pub fn parse_web_models(value: &Value) -> Vec<AvailableModel> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| serde_json::from_value::<WebModelConfig>(model.clone()).ok())
        .filter_map(WebModelConfig::into_available)
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::parse_web_models;

    #[test]
    fn parses_active_and_overflow_models_but_skips_inactive() {
        let models = parse_web_models(&json!([
            {"model": "claude-fable-5", "name": "Claude Fable 5", "hard_limit": 449000},
            {"model": "claude-opus-4-7", "overflow": true},
            {"model": "claude-opus-4-5-20251101", "inactive": true}
        ]));

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "claude-fable-5");
        assert_eq!(models[0].hard_limit, Some(449000));
        assert_eq!(models[1].id, "claude-opus-4-7");
        assert!(models[1].overflow);
    }
}
