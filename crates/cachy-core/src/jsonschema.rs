//! A small JSON Schema check, enough for `--json-schema`: the model is asked
//! for JSON, and the answer is checked before it is handed back.
//!
//! It understands `type`, `required`, `properties`, `items`, `enum` and
//! `additionalProperties: false` — the parts that make an answer usable in a
//! script. Anything else in the schema is ignored rather than guessed at.

use serde_json::Value;

/// Check `value` against `schema`, describing the first problem found.
pub fn check(schema: &Value, value: &Value) -> Result<(), String> {
    check_at(schema, value, "$")
}

fn check_at(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    let Some(object) = schema.as_object() else {
        // A schema that is not an object constrains nothing we understand.
        return Ok(());
    };

    if let Some(expected) = object.get("type") {
        let types: Vec<&str> = match expected {
            Value::String(one) => vec![one.as_str()],
            Value::Array(list) => list.iter().filter_map(|t| t.as_str()).collect(),
            _ => Vec::new(),
        };
        if !types.is_empty() && !types.iter().any(|t| matches_type(t, value)) {
            return Err(format!(
                "{path}: expected {}, got {}",
                types.join(" or "),
                kind_of(value)
            ));
        }
    }

    if let Some(allowed) = object.get("enum").and_then(|e| e.as_array()) {
        if !allowed.iter().any(|candidate| candidate == value) {
            return Err(format!("{path}: {value} is not one of the allowed values"));
        }
    }

    if let Some(required) = object.get("required").and_then(|r| r.as_array()) {
        let Some(map) = value.as_object() else {
            return Err(format!("{path}: expected an object with the required keys"));
        };
        for key in required.iter().filter_map(|k| k.as_str()) {
            if !map.contains_key(key) {
                return Err(format!("{path}: missing required key {key:?}"));
            }
        }
    }

    if let (Some(properties), Some(map)) = (
        object.get("properties").and_then(|p| p.as_object()),
        value.as_object(),
    ) {
        for (key, subschema) in properties {
            if let Some(field) = map.get(key) {
                check_at(subschema, field, &format!("{path}.{key}"))?;
            }
        }
        if object.get("additionalProperties") == Some(&Value::Bool(false)) {
            for key in map.keys() {
                if !properties.contains_key(key) {
                    return Err(format!("{path}: unexpected key {key:?}"));
                }
            }
        }
    }

    if let (Some(items), Some(list)) = (object.get("items"), value.as_array()) {
        for (index, item) in list.iter().enumerate() {
            check_at(items, item, &format!("{path}[{index}]"))?;
        }
    }

    Ok(())
}

fn matches_type(expected: &str, value: &Value) -> bool {
    match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.is_i64() || value.is_u64(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => true,
    }
}

fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Pull the JSON out of an answer: the value itself, or the first fenced block.
pub fn extract(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value);
    }
    let start = trimmed.find("```")?;
    let rest = &trimmed[start + 3..];
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    let end = rest.find("```")?;
    serde_json::from_str::<Value>(rest[..end].trim()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_what_matches_and_explains_what_does_not() {
        let schema: Value = serde_json::json!({
            "type": "object",
            "required": ["name", "count"],
            "additionalProperties": false,
            "properties": {
                "name": { "type": "string" },
                "count": { "type": "integer" },
                "tags": { "type": "array", "items": { "type": "string" } }
            }
        });

        let good = serde_json::json!({ "name": "cachy", "count": 3, "tags": ["a", "b"] });
        assert!(check(&schema, &good).is_ok());

        let missing = serde_json::json!({ "name": "cachy" });
        assert!(check(&schema, &missing).unwrap_err().contains("count"));

        let wrong_type = serde_json::json!({ "name": 1, "count": 3 });
        assert!(check(&schema, &wrong_type).unwrap_err().contains("expected string"));

        let extra = serde_json::json!({ "name": "x", "count": 1, "nope": true });
        assert!(check(&schema, &extra).unwrap_err().contains("unexpected key"));

        let bad_item = serde_json::json!({ "name": "x", "count": 1, "tags": ["ok", 2] });
        assert!(check(&schema, &bad_item).unwrap_err().contains("tags[1]"));

        // A schema we do not understand constrains nothing.
        assert!(check(&serde_json::json!({ "$ref": "#/nope" }), &good).is_ok());
    }

    #[test]
    fn finds_json_in_an_answer() {
        assert_eq!(extract("{\"a\": 1}"), Some(serde_json::json!({"a": 1})));
        assert_eq!(
            extract("Claro:\n```json\n{\"a\": 2}\n```\npronto"),
            Some(serde_json::json!({"a": 2}))
        );
        assert_eq!(
            extract("```\n{\"a\": 3}\n```"),
            Some(serde_json::json!({"a": 3}))
        );
        assert_eq!(extract("sem json aqui"), None);
    }
}
