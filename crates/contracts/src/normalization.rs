// SPDX-License-Identifier: AGPL-3.0-only

use crate::pointer::{JsonPointer, JsonPointerError};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use std::{fmt, str::FromStr};

/// Declarative, ordered normalizer applied to a JSON Pointer target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
pub enum Normalizer {
    TrimWhitespace,
    CollapseWhitespace,
    Lowercase,
    Uppercase,
    NullIfEmpty,
    ParseInteger,
    ParseNumber,
    StripCurrency,
    NormalizeDate { formats: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizerRule {
    pointer: JsonPointer,
    normalizer: Normalizer,
}

impl NormalizerRule {
    pub fn new(pointer: JsonPointer, normalizer: Normalizer) -> Self {
        Self { pointer, normalizer }
    }

    pub fn pointer(&self) -> &JsonPointer {
        &self.pointer
    }

    pub fn normalizer(&self) -> &Normalizer {
        &self.normalizer
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizationError {
    pub pointer: JsonPointer,
    pub message: String,
}

impl fmt::Display for NormalizationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.pointer, self.message)
    }
}

impl std::error::Error for NormalizationError {}

/// Apply `rules` in declaration order. Missing optional pointers are skipped.
/// The input value is never mutated.
pub fn normalize(data: &Value, rules: &[NormalizerRule]) -> Result<Value, NormalizationError> {
    let mut normalized = data.clone();
    for rule in rules {
        let Some(value) = rule
            .pointer
            .get(&normalized)
            .map_err(pointer_error(&rule.pointer))?
        else {
            continue;
        };
        let replacement = apply_normalizer(value, &rule.normalizer, &rule.pointer)?;
        rule.pointer
            .set(&mut normalized, replacement)
            .map_err(pointer_error(&rule.pointer))?;
    }
    Ok(normalized)
}

fn apply_normalizer(
    value: &Value,
    normalizer: &Normalizer,
    pointer: &JsonPointer,
) -> Result<Value, NormalizationError> {
    let string_value = || {
        value.as_str().ok_or_else(|| NormalizationError {
            pointer: pointer.clone(),
            message: "normalizer requires a string value".to_owned(),
        })
    };

    match normalizer {
        Normalizer::TrimWhitespace => Ok(Value::String(string_value()?.trim().to_owned())),
        Normalizer::CollapseWhitespace => Ok(Value::String(
            string_value()?.split_whitespace().collect::<Vec<_>>().join(" "),
        )),
        Normalizer::Lowercase => Ok(Value::String(string_value()?.to_lowercase())),
        Normalizer::Uppercase => Ok(Value::String(string_value()?.to_uppercase())),
        Normalizer::NullIfEmpty => {
            if string_value()?.trim().is_empty() {
                Ok(Value::Null)
            } else {
                Ok(value.clone())
            }
        }
        Normalizer::ParseInteger => {
            let text = string_value()?.trim();
            let integer = text.parse::<i64>().map_err(|error| NormalizationError {
                pointer: pointer.clone(),
                message: format!("invalid integer: {error}"),
            })?;
            Ok(Value::Number(integer.into()))
        }
        Normalizer::ParseNumber => parse_json_number(string_value()?.trim(), pointer),
        Normalizer::StripCurrency => parse_json_number(&strip_currency(string_value()?), pointer),
        Normalizer::NormalizeDate { formats } => {
            let text = string_value()?.trim();
            let date = formats
                .iter()
                .find_map(|format| NaiveDate::parse_from_str(text, format).ok())
                .ok_or_else(|| NormalizationError {
                    pointer: pointer.clone(),
                    message: format!("value {text:?} matched none of the declared date formats"),
                })?;
            Ok(Value::String(date.format("%Y-%m-%d").to_string()))
        }
    }
}

fn strip_currency(text: &str) -> String {
    text.chars()
        .filter(|ch| ch.is_ascii_digit() || *ch == '.' || *ch == '-' || *ch == '+')
        .collect()
}

fn parse_json_number(text: &str, pointer: &JsonPointer) -> Result<Value, NormalizationError> {
    let number = Number::from_str(text.trim()).map_err(|error| NormalizationError {
        pointer: pointer.clone(),
        message: format!("invalid JSON number: {error}"),
    })?;
    Ok(Value::Number(number))
}

fn pointer_error(pointer: &JsonPointer) -> impl FnOnce(JsonPointerError) -> NormalizationError + '_ {
    move |error| NormalizationError {
        pointer: pointer.clone(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_rules_in_declared_order() {
        let pointer = JsonPointer::parse("/name").unwrap();
        let rules = [
            NormalizerRule::new(pointer.clone(), Normalizer::TrimWhitespace),
            NormalizerRule::new(pointer, Normalizer::CollapseWhitespace),
        ];
        let result = normalize(&serde_json::json!({"name": "  Ada   Lovelace  "}), &rules).unwrap();
        assert_eq!(result, serde_json::json!({"name": "Ada Lovelace"}));
    }

    #[test]
    fn strip_currency_and_date_are_deterministic() {
        let rules = [
            NormalizerRule::new(JsonPointer::parse("/total").unwrap(), Normalizer::StripCurrency),
            NormalizerRule::new(
                JsonPointer::parse("/date").unwrap(),
                Normalizer::NormalizeDate {
                    formats: vec!["%m/%d/%Y".to_owned(), "%Y-%m-%d".to_owned()],
                },
            ),
        ];
        let result = normalize(
            &serde_json::json!({"total": "$1,234.50", "date": "03/15/2024"}),
            &rules,
        )
        .unwrap();
        assert_eq!(
            result,
            serde_json::json!({"total": 1234.50, "date": "2024-03-15"})
        );
    }

    #[test]
    fn normalization_does_not_mutate_input() {
        let input = serde_json::json!({"name": " Ada "});
        let rule = NormalizerRule::new(JsonPointer::parse("/name").unwrap(), Normalizer::TrimWhitespace);
        let _ = normalize(&input, &[rule]).unwrap();
        assert_eq!(input, serde_json::json!({"name": " Ada "}));
    }
}
