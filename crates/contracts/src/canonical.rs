// SPDX-License-Identifier: AGPL-3.0-only

use serde_json::{Map, Value};
use std::io::Write;

pub fn canonical_json(value: &Value) -> Result<Vec<u8>, serde_json::Error> {
    let mut output = Vec::new();
    write_value(value, &mut output)?;
    Ok(output)
}

fn write_value<W: Write>(value: &Value, output: &mut W) -> Result<(), serde_json::Error> {
    match value {
        Value::Null => output.write_all(b"null").map_err(serde_json::Error::io),
        Value::Bool(value) => output.write_all(if *value { b"true" } else { b"false" }).map_err(serde_json::Error::io),
        Value::Number(value) => output.write_all(value.to_string().as_bytes()).map_err(serde_json::Error::io),
        Value::String(value) => serde_json::to_writer(output, value),
        Value::Array(values) => {
            output.write_all(b"[").map_err(serde_json::Error::io)?;
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.write_all(b",").map_err(serde_json::Error::io)?;
                }
                write_value(value, output)?;
            }
            output.write_all(b"]").map_err(serde_json::Error::io)
        }
        Value::Object(values) => write_object(values, output),
    }
}

fn write_object<W: Write>(values: &Map<String, Value>, output: &mut W) -> Result<(), serde_json::Error> {
    output.write_all(b"{").map_err(serde_json::Error::io)?;
    let mut keys = values.keys().collect::<Vec<_>>();
    keys.sort_unstable();
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            output.write_all(b",").map_err(serde_json::Error::io)?;
        }
        serde_json::to_writer(&mut *output, key)?;
        output.write_all(b":").map_err(serde_json::Error::io)?;
        write_value(&values[*key], output)?;
    }
    output.write_all(b"}").map_err(serde_json::Error::io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_key_order_does_not_change_canonical_bytes() {
        let first = serde_json::json!({"b": 2, "a": {"d": 4, "c": 3}});
        let second = serde_json::json!({"a": {"c": 3, "d": 4}, "b": 2});
        assert_eq!(canonical_json(&first).unwrap(), canonical_json(&second).unwrap());
    }

    #[test]
    fn array_order_remains_semantic() {
        let first = canonical_json(&serde_json::json!([1, 2])).unwrap();
        let second = canonical_json(&serde_json::json!([2, 1])).unwrap();
        assert_ne!(first, second);
    }
}
