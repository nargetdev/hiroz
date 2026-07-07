//! Rendering and construction of dynamic messages for `topic echo` / `topic pub`.
//!
//! The JSON and pretty-block renderers are ports of
//! `hiroz-console`'s `core::message_formatter` (which lives in the console
//! binary, not the library). `build_message` is the inverse used by
//! `topic pub`: it maps a YAML value onto a `MessageSchema` to produce a
//! `DynamicMessage`.

use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use hiroz::dynamic::{DynamicMessage, DynamicValue, FieldType, MessageSchema};

/// Convert a `DynamicMessage` to a `serde_json::Value`.
pub fn message_to_json(msg: &DynamicMessage) -> serde_json::Value {
    let mut fields = serde_json::Map::new();
    for (name, value) in msg.iter() {
        fields.insert(name.to_string(), value_to_json(value));
    }
    serde_json::Value::Object(fields)
}

/// Convert a `DynamicValue` to a `serde_json::Value`.
pub fn value_to_json(value: &DynamicValue) -> serde_json::Value {
    use serde_json::Value;
    match value {
        DynamicValue::Bool(b) => Value::Bool(*b),
        DynamicValue::Int8(i) => Value::Number((*i).into()),
        DynamicValue::Int16(i) => Value::Number((*i).into()),
        DynamicValue::Int32(i) => Value::Number((*i).into()),
        DynamicValue::Int64(i) => Value::Number((*i).into()),
        DynamicValue::Uint8(u) => Value::Number((*u).into()),
        DynamicValue::Uint16(u) => Value::Number((*u).into()),
        DynamicValue::Uint32(u) => Value::Number((*u).into()),
        DynamicValue::Uint64(u) => Value::Number((*u).into()),
        DynamicValue::Float32(f) => serde_json::Number::from_f64(*f as f64)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        DynamicValue::Float64(f) => serde_json::Number::from_f64(*f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        DynamicValue::String(s) => Value::String(s.clone()),
        DynamicValue::Bytes(b) => {
            Value::Array(b.iter().map(|&byte| Value::Number(byte.into())).collect())
        }
        DynamicValue::Message(msg) => message_to_json(msg),
        DynamicValue::Array(arr) => Value::Array(arr.iter().map(value_to_json).collect()),
    }
}

/// Render a message as a ROS 2-style indented YAML-ish block.
pub fn format_pretty(msg: &DynamicMessage) -> String {
    let mut out = String::new();
    for (name, value) in msg.iter() {
        fmt_value(&mut out, name, value, 0);
    }
    out
}

fn fmt_value(out: &mut String, name: &str, value: &DynamicValue, indent: usize) {
    let prefix = "  ".repeat(indent);
    match value {
        DynamicValue::Bool(v) => out.push_str(&format!("{prefix}{name}: {v}\n")),
        DynamicValue::Int8(v) => out.push_str(&format!("{prefix}{name}: {v}\n")),
        DynamicValue::Int16(v) => out.push_str(&format!("{prefix}{name}: {v}\n")),
        DynamicValue::Int32(v) => out.push_str(&format!("{prefix}{name}: {v}\n")),
        DynamicValue::Int64(v) => out.push_str(&format!("{prefix}{name}: {v}\n")),
        DynamicValue::Uint8(v) => out.push_str(&format!("{prefix}{name}: {v}\n")),
        DynamicValue::Uint16(v) => out.push_str(&format!("{prefix}{name}: {v}\n")),
        DynamicValue::Uint32(v) => out.push_str(&format!("{prefix}{name}: {v}\n")),
        DynamicValue::Uint64(v) => out.push_str(&format!("{prefix}{name}: {v}\n")),
        DynamicValue::Float32(v) => out.push_str(&format!("{prefix}{name}: {v}\n")),
        DynamicValue::Float64(v) => out.push_str(&format!("{prefix}{name}: {v}\n")),
        DynamicValue::String(v) => out.push_str(&format!("{prefix}{name}: \"{v}\"\n")),
        DynamicValue::Bytes(b) => out.push_str(&format!("{prefix}{name}: <{} bytes>\n", b.len())),
        DynamicValue::Message(msg) => {
            out.push_str(&format!("{prefix}{name}:\n"));
            for (n, v) in msg.iter() {
                fmt_value(out, n, v, indent + 1);
            }
        }
        DynamicValue::Array(arr) => {
            if arr.is_empty() {
                out.push_str(&format!("{prefix}{name}: []\n"));
            } else {
                out.push_str(&format!("{prefix}{name}:\n"));
                for v in arr.iter() {
                    fmt_value(out, "-", v, indent + 1);
                }
            }
        }
    }
}

/// Build a `DynamicMessage` for `schema` from a YAML value.
///
/// Fields omitted from the YAML keep their schema/zero defaults. Unknown fields
/// are rejected.
pub fn build_message(
    schema: &Arc<MessageSchema>,
    yaml: &serde_yaml::Value,
) -> Result<DynamicMessage> {
    let mut msg = DynamicMessage::new(schema);

    let map = match yaml {
        serde_yaml::Value::Mapping(m) => m,
        serde_yaml::Value::Null => return Ok(msg),
        other => bail!(
            "expected a YAML mapping for {}, got {other:?}",
            schema.type_name
        ),
    };

    // Reject unknown keys up front for clear errors.
    for key in map.keys() {
        let key = key
            .as_str()
            .ok_or_else(|| anyhow!("non-string field name in {}", schema.type_name))?;
        if schema.field(key).is_none() {
            bail!("unknown field '{key}' for type {}", schema.type_name);
        }
    }

    for field in &schema.fields {
        if let Some(v) = map.get(serde_yaml::Value::String(field.name.clone())) {
            let dv = yaml_to_dynamic(&field.field_type, v)
                .map_err(|e| anyhow!("field '{}': {e}", field.name))?;
            msg.set_dynamic(&field.name, dv)
                .map_err(|e| anyhow!("failed to set field '{}': {e}", field.name))?;
        }
    }
    Ok(msg)
}

fn yaml_to_dynamic(field_type: &FieldType, v: &serde_yaml::Value) -> Result<DynamicValue> {
    use serde_yaml::Value as Y;
    let want_int = |v: &Y| -> Result<i64> {
        v.as_i64()
            .or_else(|| v.as_u64().map(|u| u as i64))
            .ok_or_else(|| anyhow!("expected an integer, got {v:?}"))
    };
    let want_float = |v: &Y| -> Result<f64> {
        v.as_f64()
            .or_else(|| v.as_i64().map(|i| i as f64))
            .ok_or_else(|| anyhow!("expected a number, got {v:?}"))
    };
    Ok(match field_type {
        FieldType::Bool => DynamicValue::Bool(
            v.as_bool()
                .ok_or_else(|| anyhow!("expected a bool, got {v:?}"))?,
        ),
        FieldType::Int8 => DynamicValue::Int8(want_int(v)? as i8),
        FieldType::Int16 => DynamicValue::Int16(want_int(v)? as i16),
        FieldType::Int32 => DynamicValue::Int32(want_int(v)? as i32),
        FieldType::Int64 => DynamicValue::Int64(want_int(v)?),
        FieldType::Uint8 => DynamicValue::Uint8(want_int(v)? as u8),
        FieldType::Uint16 => DynamicValue::Uint16(want_int(v)? as u16),
        FieldType::Uint32 => DynamicValue::Uint32(want_int(v)? as u32),
        FieldType::Uint64 => DynamicValue::Uint64(want_int(v)? as u64),
        FieldType::Float32 => DynamicValue::Float32(want_float(v)? as f32),
        FieldType::Float64 => DynamicValue::Float64(want_float(v)?),
        FieldType::String | FieldType::BoundedString(_) => DynamicValue::String(
            v.as_str()
                .ok_or_else(|| anyhow!("expected a string, got {v:?}"))?
                .to_string(),
        ),
        FieldType::Message(inner) => {
            let sub = build_message(inner, v)?;
            DynamicValue::Message(Box::new(sub))
        }
        FieldType::Array(inner, _)
        | FieldType::Sequence(inner)
        | FieldType::BoundedSequence(inner, _) => {
            let seq = v
                .as_sequence()
                .ok_or_else(|| anyhow!("expected a sequence, got {v:?}"))?;
            // uint8[] / byte[] map to the optimized Bytes variant.
            if matches!(**inner, FieldType::Uint8) {
                let mut bytes = Vec::with_capacity(seq.len());
                for e in seq {
                    bytes.push(want_int(e)? as u8);
                }
                DynamicValue::Bytes(bytes)
            } else {
                let mut arr = Vec::with_capacity(seq.len());
                for e in seq {
                    arr.push(yaml_to_dynamic(inner, e)?);
                }
                DynamicValue::Array(arr)
            }
        }
    })
}
