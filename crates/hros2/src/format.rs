//! Convert a decoded `DynamicMessage` into a `serde_json::Value`.
//!
//! From this JSON value hros2 renders either ros2-style YAML (`serde_yaml`) or
//! compact JSON. Ported from `hiroz-console`'s message_formatter (that crate is a
//! binary and can't be depended on as a library). The JSON `Map` preserves field
//! insertion order via serde_json's `preserve_order` feature, so output matches the
//! message-definition order the way `ros2 topic echo` does.

use hiroz::dynamic::{DynamicMessage, DynamicValue};

/// Convert a `DynamicMessage` to a JSON object, recursing into nested messages/arrays.
pub fn dynamic_message_to_json(msg: &DynamicMessage) -> serde_json::Value {
    let mut fields = serde_json::Map::new();
    for (name, value) in msg.iter() {
        fields.insert(name.to_string(), dynamic_value_to_json(value));
    }
    serde_json::Value::Object(fields)
}

/// Convert a single `DynamicValue` to JSON, handling every variant.
pub fn dynamic_value_to_json(value: &DynamicValue) -> serde_json::Value {
    match value {
        DynamicValue::Bool(b) => serde_json::Value::Bool(*b),
        DynamicValue::Int8(i) => serde_json::Value::Number((*i).into()),
        DynamicValue::Int16(i) => serde_json::Value::Number((*i).into()),
        DynamicValue::Int32(i) => serde_json::Value::Number((*i).into()),
        DynamicValue::Int64(i) => serde_json::Value::Number((*i).into()),
        DynamicValue::Uint8(u) => serde_json::Value::Number((*u).into()),
        DynamicValue::Uint16(u) => serde_json::Value::Number((*u).into()),
        DynamicValue::Uint32(u) => serde_json::Value::Number((*u).into()),
        DynamicValue::Uint64(u) => serde_json::Value::Number((*u).into()),
        DynamicValue::Float32(f) => serde_json::Number::from_f64(*f as f64)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        DynamicValue::Float64(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        DynamicValue::String(s) => serde_json::Value::String(s.clone()),
        DynamicValue::Bytes(b) => serde_json::Value::Array(
            b.iter()
                .map(|&byte| serde_json::Value::Number(byte.into()))
                .collect(),
        ),
        DynamicValue::Message(msg) => dynamic_message_to_json(msg),
        DynamicValue::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(dynamic_value_to_json).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hiroz::dynamic::{FieldType, MessageSchema};

    #[test]
    fn json_primitives() {
        let schema = MessageSchema::builder("test_msgs/msg/Primitives")
            .field("flag", FieldType::Bool)
            .field("count", FieldType::Int32)
            .field("value", FieldType::Float64)
            .field("name", FieldType::String)
            .build()
            .unwrap();

        let mut msg = DynamicMessage::new(&schema);
        msg.set("flag", true).unwrap();
        msg.set("count", 42i32).unwrap();
        msg.set("value", std::f64::consts::PI).unwrap();
        msg.set("name", "test".to_string()).unwrap();

        let json = dynamic_message_to_json(&msg);
        assert_eq!(json["flag"], true);
        assert_eq!(json["count"], 42);
        assert_eq!(json["value"], std::f64::consts::PI);
        assert_eq!(json["name"], "test");
    }

    #[test]
    fn nested_message_and_field_order() {
        let inner = MessageSchema::builder("geometry_msgs/msg/Vector3")
            .field("x", FieldType::Float64)
            .field("y", FieldType::Float64)
            .field("z", FieldType::Float64)
            .build()
            .unwrap();
        let outer = MessageSchema::builder("geometry_msgs/msg/Twist")
            .field("linear", FieldType::Message(inner.clone()))
            .field("angular", FieldType::Message(inner))
            .build()
            .unwrap();

        let mut msg = DynamicMessage::new(&outer);
        msg.set("linear.x", 1.0f64).unwrap();

        let json = dynamic_message_to_json(&msg);
        assert_eq!(json["linear"]["x"], 1.0);

        // preserve_order: fields must serialize in declaration order (linear before angular),
        // matching ros2 rosidl output rather than alphabetical.
        let s = serde_json::to_string(&json).unwrap();
        let linear_at = s.find("linear").unwrap();
        let angular_at = s.find("angular").unwrap();
        assert!(linear_at < angular_at, "fields must keep declaration order: {s}");
    }
}
