// src/schema_inference.rs

use serde_json::Value;
use crate::site_knowledge::{EndpointEntry, EndpointSchema, SchemaConfidence};

/// Infer a JSON Schema from a runtime JSON value.
pub fn infer(value: &Value) -> Value {
    match value {
        Value::Null => serde_json::json!({"type": "null"}),
        Value::Bool(_) => serde_json::json!({"type": "boolean"}),
        Value::Number(_) => serde_json::json!({"type": "number"}),
        Value::String(_) => serde_json::json!({"type": "string"}),
        Value::Array(items) => {
            let item_schema = if items.is_empty() {
                serde_json::json!({})
            } else {
                items.iter()
                    .skip(1)
                    .fold(infer(&items[0]), |acc, v| merge(&acc, &infer(v)))
            };
            serde_json::json!({"type": "array", "items": item_schema})
        }
        Value::Object(map) => {
            let mut properties = serde_json::Map::new();
            let required: Vec<Value> = map.keys().map(|k| Value::String(k.clone())).collect();
            for (key, val) in map {
                properties.insert(key.clone(), infer(val));
            }
            serde_json::json!({
                "type": "object",
                "properties": properties,
                "required": required
            })
        }
    }
}

/// Merge two JSON Schemas — union of properties, required = intersection.
/// For non-object types or type mismatches, returns the first schema unchanged.
pub fn merge(a: &Value, b: &Value) -> Value {
    if a.get("type") != Some(&Value::String("object".into()))
        || b.get("type") != Some(&Value::String("object".into()))
    {
        return a.clone();
    }

    let a_props = a["properties"].as_object().cloned().unwrap_or_default();
    let b_props = b["properties"].as_object().cloned().unwrap_or_default();

    let a_req: std::collections::HashSet<String> = a["required"]
        .as_array().unwrap_or(&vec![])
        .iter().filter_map(|v| v.as_str().map(String::from)).collect();
    let b_req: std::collections::HashSet<String> = b["required"]
        .as_array().unwrap_or(&vec![])
        .iter().filter_map(|v| v.as_str().map(String::from)).collect();

    let mut merged_props = serde_json::Map::new();
    let all_keys: std::collections::HashSet<String> =
        a_props.keys().chain(b_props.keys()).cloned().collect();

    for key in &all_keys {
        match (a_props.get(key), b_props.get(key)) {
            (Some(av), Some(bv)) => { merged_props.insert(key.clone(), merge(av, bv)); }
            (Some(av), None) => { merged_props.insert(key.clone(), av.clone()); }
            (None, Some(bv)) => { merged_props.insert(key.clone(), bv.clone()); }
            (None, None) => {}
        }
    }

    let required: Vec<Value> = a_req.intersection(&b_req)
        .map(|s| Value::String(s.clone())).collect();

    serde_json::json!({
        "type": "object",
        "properties": merged_props,
        "required": required
    })
}

/// Map observation count to schema confidence level.
pub fn confidence(count: u32) -> SchemaConfidence {
    match count {
        0..=2 => SchemaConfidence::Provisional,
        3..=5 => SchemaConfidence::Likely,
        _ => SchemaConfidence::Confirmed,
    }
}

/// Update or create the schema on an endpoint entry given new request/response observations.
/// Called from endpoint_mapper::ingest after each completed request.
pub fn update_endpoint_schema(
    ep: &mut EndpointEntry,
    req_body: Option<&str>,
    resp_val: &Value,
) {
    let new_resp_schema = infer(resp_val);
    let new_req_schema = req_body
        .and_then(|b| serde_json::from_str::<Value>(b).ok())
        .map(|v| infer(&v));

    match ep.schema.take() {
        None => {
            ep.schema = Some(EndpointSchema {
                request_body: new_req_schema,
                response_body: Some(new_resp_schema),
                confidence: confidence(1),
                observation_count: 1,
            });
        }
        Some(mut existing) => {
            existing.observation_count += 1;
            existing.confidence = confidence(existing.observation_count);
            if let Some(existing_resp) = &existing.response_body {
                existing.response_body = Some(merge(existing_resp, &new_resp_schema));
            } else {
                existing.response_body = Some(new_resp_schema);
            }
            if let (Some(existing_req), Some(new_req)) = (&existing.request_body, new_req_schema) {
                existing.request_body = Some(merge(existing_req, &new_req));
            }
            ep.schema = Some(existing);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn infer_string() {
        assert_eq!(infer(&json!("hello")), json!({"type": "string"}));
    }

    #[test]
    fn infer_number() {
        assert_eq!(infer(&json!(42.0)), json!({"type": "number"}));
    }

    #[test]
    fn infer_boolean() {
        assert_eq!(infer(&json!(true)), json!({"type": "boolean"}));
    }

    #[test]
    fn infer_null() {
        assert_eq!(infer(&json!(null)), json!({"type": "null"}));
    }

    #[test]
    fn infer_object_has_properties_and_required() {
        let schema = infer(&json!({"id": 1, "name": "Alice"}));
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["id"].is_object());
        assert!(schema["properties"]["name"].is_object());
        // required contains both keys
        let req = schema["required"].as_array().unwrap();
        assert_eq!(req.len(), 2);
    }

    #[test]
    fn infer_array_with_items() {
        let schema = infer(&json!([{"id": 1}, {"id": 2}]));
        assert_eq!(schema["type"], "array");
        assert!(schema["items"].is_object());
    }

    #[test]
    fn merge_objects_union_keys() {
        let a = infer(&json!({"id": 1, "name": "Alice"}));
        let b = infer(&json!({"id": 2, "email": "a@b.com"}));
        let merged = merge(&a, &b);
        // Should have id, name, email
        assert!(merged["properties"]["id"].is_object());
        assert!(merged["properties"]["name"].is_object());
        assert!(merged["properties"]["email"].is_object());
        // required = intersection = ["id"] only
        let req: Vec<&str> = merged["required"].as_array().unwrap()
            .iter().filter_map(|v| v.as_str()).collect();
        assert!(req.contains(&"id"), "got: {:?}", req);
        assert!(!req.contains(&"name"), "name should not be required after merge: {:?}", req);
    }

    #[test]
    fn confidence_levels() {
        use crate::site_knowledge::SchemaConfidence;
        assert_eq!(confidence(1), SchemaConfidence::Provisional);
        assert_eq!(confidence(2), SchemaConfidence::Provisional);
        assert_eq!(confidence(3), SchemaConfidence::Likely);
        assert_eq!(confidence(5), SchemaConfidence::Likely);
        assert_eq!(confidence(6), SchemaConfidence::Confirmed);
        assert_eq!(confidence(100), SchemaConfidence::Confirmed);
    }
}
