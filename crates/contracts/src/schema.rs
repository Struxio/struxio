// SPDX-License-Identifier: AGPL-3.0-only

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// Error compiling or applying a JSON Schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaError(String);

impl SchemaError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for SchemaError {}

/// Compiled-at-construction JSON Schema. The compiled validator is not stored
/// because it is not serializable; validation recompiles from the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonSchema(Value);

impl JsonSchema {
    /// Rejects non-schema values and schemas the `jsonschema` crate cannot compile.
    pub fn new(schema: Value) -> Result<Self, SchemaError> {
        if !schema.is_object() && !schema.is_boolean() {
            return Err(SchemaError::new("json schema must be an object or boolean"));
        }
        jsonschema::validator_for(&schema).map_err(|error| SchemaError::new(error.to_string()))?;
        Ok(Self(schema))
    }

    pub fn as_value(&self) -> &Value {
        &self.0
    }

    /// Validate `data` with the latest `jsonschema` crate (draft auto-detected).
    pub fn validate(&self, data: &Value) -> SchemaValidationReport {
        match jsonschema::validator_for(&self.0) {
            Ok(validator) => SchemaValidationReport {
                errors: validator
                    .iter_errors(data)
                    .map(|error| SchemaValidationError {
                        instance_pointer: error.instance_path().to_string(),
                        schema_pointer: error.schema_path().to_string(),
                        message: error.to_string(),
                    })
                    .collect(),
            },
            Err(error) => SchemaValidationReport {
                errors: vec![SchemaValidationError {
                    instance_pointer: String::new(),
                    schema_pointer: String::new(),
                    message: error.to_string(),
                }],
            },
        }
    }
}

impl Serialize for JsonSchema {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for JsonSchema {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaValidationError {
    pub instance_pointer: String,
    pub schema_pointer: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaValidationReport {
    errors: Vec<SchemaValidationError>,
}

impl SchemaValidationReport {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn errors(&self) -> &[SchemaValidationError] {
        &self.errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_result_data_against_schema() {
        let schema = JsonSchema::new(serde_json::json!({
            "type": "object",
            "required": ["name"],
            "properties": {"name": {"type": "string"}}
        }))
        .unwrap();
        assert!(schema
            .validate(&serde_json::json!({"name": "Ada"}))
            .is_valid());
        assert!(!schema.validate(&serde_json::json!({})).is_valid());
        assert!(!schema.validate(&serde_json::json!({"name": 1})).is_valid());
    }

    #[test]
    fn rejects_invalid_and_non_object_schemas() {
        assert!(JsonSchema::new(serde_json::json!({"type": "not-a-type"})).is_err());
        assert!(JsonSchema::new(serde_json::json!(true)).is_err());
        assert!(JsonSchema::new(serde_json::json!([])).is_err());
    }

    #[test]
    fn nested_required_and_additional_properties() {
        let schema = JsonSchema::new(serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "additionalProperties": false,
            "required": ["vendor"],
            "properties": {
                "vendor": {
                    "type": "object",
                    "required": ["name"],
                    "properties": { "name": { "type": "string" } }
                }
            }
        }))
        .unwrap();
        assert!(schema
            .validate(&serde_json::json!({"vendor": {"name": "Acme"}}))
            .is_valid());
        assert!(!schema
            .validate(&serde_json::json!({"vendor": {}, "extra": true}))
            .is_valid());
    }
}
