// SPDX-License-Identifier: AGPL-3.0-only

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonPointerError {
    MustStartWithSlash,
    InvalidEscape,
    InvalidArrayIndex,
    CannotTraverse,
}

impl fmt::Display for JsonPointerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MustStartWithSlash => write!(f, "JSON Pointer must be empty or start with '/'"),
            Self::InvalidEscape => write!(f, "JSON Pointer contains an invalid '~' escape"),
            Self::InvalidArrayIndex => write!(f, "JSON Pointer contains an invalid array index"),
            Self::CannotTraverse => write!(f, "JSON Pointer cannot traverse the JSON value"),
        }
    }
}

impl std::error::Error for JsonPointerError {}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct JsonPointer(Vec<String>);

impl JsonPointer {
    pub fn root() -> Self {
        Self(Vec::new())
    }

    pub fn parse(value: &str) -> Result<Self, JsonPointerError> {
        if value.is_empty() {
            return Ok(Self::root());
        }
        if !value.starts_with('/') {
            return Err(JsonPointerError::MustStartWithSlash);
        }
        value
            .split('/')
            .skip(1)
            .map(unescape_token)
            .collect::<Result<Vec<_>, _>>()
            .map(Self)
    }

    pub fn tokens(&self) -> &[String] {
        &self.0
    }

    pub fn get<'a>(&self, value: &'a Value) -> Result<Option<&'a Value>, JsonPointerError> {
        let mut current = value;
        for token in &self.0 {
            current = match current {
                Value::Object(object) => match object.get(token) {
                    Some(value) => value,
                    None => return Ok(None),
                },
                Value::Array(array) => {
                    let index = array_index(token)?;
                    match array.get(index) {
                        Some(value) => value,
                        None => return Ok(None),
                    }
                }
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
                    return Err(JsonPointerError::CannotTraverse)
                }
            };
        }
        Ok(Some(current))
    }

    pub(crate) fn set(&self, root: &mut Value, replacement: Value) -> Result<(), JsonPointerError> {
        if self.0.is_empty() {
            *root = replacement;
            return Ok(());
        }

        let (last, parents) = self.0.split_last().expect("non-empty pointer");
        let mut current = root;
        for token in parents {
            current = match current {
                Value::Object(object) => object
                    .get_mut(token)
                    .ok_or(JsonPointerError::CannotTraverse)?,
                Value::Array(array) => {
                    let index = array_index(token)?;
                    array
                        .get_mut(index)
                        .ok_or(JsonPointerError::CannotTraverse)?
                }
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
                    return Err(JsonPointerError::CannotTraverse)
                }
            };
        }

        match current {
            Value::Object(object) => {
                let target = object
                    .get_mut(last)
                    .ok_or(JsonPointerError::CannotTraverse)?;
                *target = replacement;
                Ok(())
            }
            Value::Array(array) => {
                let index = array_index(last)?;
                let target = array
                    .get_mut(index)
                    .ok_or(JsonPointerError::CannotTraverse)?;
                *target = replacement;
                Ok(())
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
                Err(JsonPointerError::CannotTraverse)
            }
        }
    }
}

impl FromStr for JsonPointer {
    type Err = JsonPointerError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl TryFrom<String> for JsonPointer {
    type Error = JsonPointerError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<JsonPointer> for String {
    fn from(value: JsonPointer) -> Self {
        value.to_string()
    }
}

impl fmt::Display for JsonPointer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            return Ok(());
        }
        for token in &self.0 {
            write!(f, "/{}", escape_token(token))?;
        }
        Ok(())
    }
}

fn unescape_token(token: &str) -> Result<String, JsonPointerError> {
    let mut result = String::with_capacity(token.len());
    let mut chars = token.chars();
    while let Some(character) = chars.next() {
        if character != '~' {
            result.push(character);
            continue;
        }
        match chars.next() {
            Some('0') => result.push('~'),
            Some('1') => result.push('/'),
            _ => return Err(JsonPointerError::InvalidEscape),
        }
    }
    Ok(result)
}

fn escape_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

fn array_index(token: &str) -> Result<usize, JsonPointerError> {
    if token.is_empty() || (token.len() > 1 && token.starts_with('0')) || token == "-" {
        return Err(JsonPointerError::InvalidArrayIndex);
    }
    token
        .parse()
        .map_err(|_| JsonPointerError::InvalidArrayIndex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_rfc_6901_escapes() {
        let pointer = JsonPointer::parse("/a~1b/c~0d").unwrap();
        assert_eq!(pointer.tokens(), &["a/b".to_owned(), "c~d".to_owned()]);
        assert_eq!(pointer.to_string(), "/a~1b/c~0d");
    }

    #[test]
    fn rejects_bad_escape_and_prefix() {
        assert_eq!(
            JsonPointer::parse("a/b"),
            Err(JsonPointerError::MustStartWithSlash)
        );
        assert_eq!(
            JsonPointer::parse("/a~2b"),
            Err(JsonPointerError::InvalidEscape)
        );
    }

    #[test]
    fn resolves_object_and_array_values() {
        let value = serde_json::json!({"items": [{"value": 3}]});
        assert_eq!(
            JsonPointer::parse("/items/0/value")
                .unwrap()
                .get(&value)
                .unwrap(),
            Some(&serde_json::json!(3))
        );
    }

    #[test]
    fn serializes_as_an_rfc_6901_string() {
        let pointer = JsonPointer::parse("/a~1b").unwrap();
        let encoded = serde_json::to_string(&pointer).unwrap();
        assert_eq!(encoded, r#""/a~1b""#);
        assert_eq!(
            serde_json::from_str::<JsonPointer>(&encoded).unwrap(),
            pointer
        );
    }
}
