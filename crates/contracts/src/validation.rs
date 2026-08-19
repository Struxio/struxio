// SPDX-License-Identifier: AGPL-3.0-only

use crate::error::ContractError;
use crate::pointer::JsonPointer;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JsonValueType {
    Null,
    Boolean,
    Number,
    Integer,
    String,
    Array,
    Object,
}

/// Declarative validator applied to a JSON Pointer. Document-level JSON Schema
/// validation is performed separately by [`crate::schema::JsonSchema`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum Validator {
    Required,
    NonNull,
    NonEmptyString,
    Type(JsonValueType),
    Equals(Value),
    Minimum(f64),
    Maximum(f64),
    Regex {
        pattern: String,
    },
    NumericTolerance {
        other: JsonPointer,
        tolerance: f64,
    },
    SumEquals {
        addends: Vec<JsonPointer>,
        tolerance: f64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidatorRule {
    pointer: JsonPointer,
    validator: Validator,
}

impl ValidatorRule {
    pub fn new(pointer: JsonPointer, validator: Validator) -> Result<Self, ContractError> {
        if let Validator::Regex { pattern } = &validator {
            Regex::new(pattern).map_err(|error| ContractError::InvalidRegex(error.to_string()))?;
        }
        Ok(Self { pointer, validator })
    }

    pub fn pointer(&self) -> &JsonPointer {
        &self.pointer
    }

    pub fn validator(&self) -> &Validator {
        &self.validator
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationFailure {
    pub pointer: JsonPointer,
    pub validator: Validator,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStatus {
    Valid,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationReport {
    status: ValidationStatus,
    issues: Vec<ValidationFailure>,
}

impl ValidationReport {
    pub fn from_failures(issues: Vec<ValidationFailure>) -> Self {
        let status = if issues.is_empty() {
            ValidationStatus::Valid
        } else {
            ValidationStatus::Invalid
        };
        Self { status, issues }
    }

    pub fn status(&self) -> ValidationStatus {
        self.status
    }

    pub fn is_valid(&self) -> bool {
        self.status == ValidationStatus::Valid
    }

    pub fn issues(&self) -> &[ValidationFailure] {
        &self.issues
    }
}

/// Run `rules` in declaration order. Failures are collected, never short-circuited.
pub fn validate(data: &Value, rules: &[ValidatorRule]) -> Vec<ValidationFailure> {
    rules
        .iter()
        .filter_map(|rule| validate_rule(data, rule))
        .collect()
}

fn validate_rule(data: &Value, rule: &ValidatorRule) -> Option<ValidationFailure> {
    let value = rule.pointer.get(data).ok().flatten();
    let failure = |message: String| ValidationFailure {
        pointer: rule.pointer.clone(),
        validator: rule.validator.clone(),
        message,
    };

    match &rule.validator {
        Validator::Required => match value {
            Some(_) => None,
            None => Some(failure("value is required".to_owned())),
        },
        Validator::NonNull => match value {
            Some(Value::Null) | None => {
                Some(failure("value must not be null or missing".to_owned()))
            }
            Some(_) => None,
        },
        Validator::NonEmptyString => match value {
            Some(Value::String(text)) if !text.trim().is_empty() => None,
            Some(Value::String(_)) => Some(failure("string must not be empty".to_owned())),
            _ => Some(failure("value must be a non-empty string".to_owned())),
        },
        Validator::Type(expected) => match value {
            Some(value) if value_type(value) == *expected => None,
            Some(value) => Some(failure(format!(
                "expected {expected:?}, got {:?}",
                value_type(value)
            ))),
            None => Some(failure(format!("expected {expected:?}, got missing"))),
        },
        Validator::Equals(expected) => match value {
            Some(value) if value == expected => None,
            Some(value) => Some(failure(format!("expected {expected}, got {value}"))),
            None => Some(failure(format!("expected {expected}, got missing"))),
        },
        Validator::Minimum(minimum) => numeric_bound(value, *minimum, true, &failure),
        Validator::Maximum(maximum) => numeric_bound(value, *maximum, false, &failure),
        Validator::Regex { pattern } => match value {
            Some(Value::String(text)) => match Regex::new(pattern) {
                Ok(regex) if regex.is_match(text) => None,
                Ok(_) => Some(failure(format!("value does not match /{pattern}/"))),
                Err(error) => Some(failure(format!("invalid regex: {error}"))),
            },
            _ => Some(failure("regex validator requires a string".to_owned())),
        },
        Validator::NumericTolerance { other, tolerance } => {
            let Some(left) = value.and_then(Value::as_f64) else {
                return Some(failure("left-hand numeric value is missing".to_owned()));
            };
            match other.get(data).ok().flatten().and_then(Value::as_f64) {
                Some(right) if (left - right).abs() <= *tolerance => None,
                Some(right) => Some(failure(format!(
                    "{left} is not within {tolerance} of {right}"
                ))),
                None => Some(failure("right-hand numeric value is missing".to_owned())),
            }
        }
        Validator::SumEquals { addends, tolerance } => {
            let Some(total) = value.and_then(Value::as_f64) else {
                return Some(failure("total is not numeric".to_owned()));
            };
            let mut sum = 0.0;
            for addend in addends {
                match addend.get(data).ok().flatten().and_then(Value::as_f64) {
                    Some(number) => sum += number,
                    None => {
                        return Some(failure(format!(
                            "addend {addend} is missing or not numeric"
                        )))
                    }
                }
            }
            if (sum - total).abs() <= *tolerance {
                None
            } else {
                Some(failure(format!(
                    "sum {sum} is not within {tolerance} of total {total}"
                )))
            }
        }
    }
}

fn numeric_bound(
    value: Option<&Value>,
    bound: f64,
    is_minimum: bool,
    failure: &impl Fn(String) -> ValidationFailure,
) -> Option<ValidationFailure> {
    let Some(value) = value else {
        return Some(failure("numeric value is missing".to_owned()));
    };
    let Some(number) = value.as_f64() else {
        return Some(failure("value must be numeric".to_owned()));
    };
    if (is_minimum && number < bound) || (!is_minimum && number > bound) {
        let direction = if is_minimum { "at least" } else { "at most" };
        Some(failure(format!("value must be {direction} {bound}")))
    } else {
        None
    }
}

fn value_type(value: &Value) -> JsonValueType {
    match value {
        Value::Null => JsonValueType::Null,
        Value::Bool(_) => JsonValueType::Boolean,
        Value::Number(number) if number.is_i64() || number.is_u64() => JsonValueType::Integer,
        Value::Number(_) => JsonValueType::Number,
        Value::String(_) => JsonValueType::String,
        Value::Array(_) => JsonValueType::Array,
        Value::Object(_) => JsonValueType::Object,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validators_preserve_declaration_order() {
        let pointer = JsonPointer::parse("/name").unwrap();
        let rules = [
            ValidatorRule::new(pointer.clone(), Validator::Required).unwrap(),
            ValidatorRule::new(pointer, Validator::NonEmptyString).unwrap(),
        ];
        let failures = validate(&serde_json::json!({}), &rules);
        assert_eq!(failures.len(), 2);
        assert_eq!(failures[0].validator, Validator::Required);
        assert_eq!(failures[1].validator, Validator::NonEmptyString);
    }

    #[test]
    fn required_passes_when_present() {
        let rule =
            ValidatorRule::new(JsonPointer::parse("/name").unwrap(), Validator::Required).unwrap();
        assert!(validate(&serde_json::json!({"name": "Ada"}), &[rule]).is_empty());
    }

    #[test]
    fn regex_and_sum_tolerance() {
        let invoice = serde_json::json!({
            "number": "INV-1042",
            "total": 30.0,
            "lines": [{"amt": 10.0}, {"amt": 20.0}]
        });
        let rules = [
            ValidatorRule::new(
                JsonPointer::parse("/number").unwrap(),
                Validator::Regex {
                    pattern: r"^INV-\d+$".to_owned(),
                },
            )
            .unwrap(),
            ValidatorRule::new(
                JsonPointer::parse("/total").unwrap(),
                Validator::SumEquals {
                    addends: vec![
                        JsonPointer::parse("/lines/0/amt").unwrap(),
                        JsonPointer::parse("/lines/1/amt").unwrap(),
                    ],
                    tolerance: 0.01,
                },
            )
            .unwrap(),
        ];
        assert!(validate(&invoice, &rules).is_empty());
    }

    #[test]
    fn rejects_invalid_regex_at_construction() {
        assert!(ValidatorRule::new(
            JsonPointer::parse("/n").unwrap(),
            Validator::Regex {
                pattern: "(".to_owned(),
            },
        )
        .is_err());
    }
}
