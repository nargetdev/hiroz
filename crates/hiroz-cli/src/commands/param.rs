//! `hros2 param` — parameter get/set/list via `ParameterClient`.

use anyhow::{Context, Result};
use hiroz::parameter::{
    Parameter, ParameterClient, ParameterTarget, ParameterType, ParameterValue,
};
use serde_json::json;

use crate::cli::{GlobalOpts, ParamCmd};
use crate::conn::Conn;
use crate::output::emit;

pub async fn run(conn: &Conn, g: &GlobalOpts, cmd: ParamCmd) -> Result<()> {
    match cmd {
        ParamCmd::List { node_name } => list(conn, g, &node_name).await,
        ParamCmd::Get {
            node_name,
            param_name,
        } => get(conn, g, &node_name, &param_name).await,
        ParamCmd::Set {
            node_name,
            param_name,
            value,
        } => set(conn, g, &node_name, &param_name, &value).await,
        ParamCmd::Describe {
            node_name,
            param_name,
        } => describe(conn, g, &node_name, &param_name).await,
        ParamCmd::Dump { node_name } => dump(conn, g, &node_name).await,
        ParamCmd::Delete {
            node_name,
            param_name,
        } => delete(conn, g, &node_name, &param_name).await,
    }
}

fn client(conn: &Conn, node_name: &str) -> Result<ParameterClient> {
    let target = ParameterTarget::from_fqn(node_name)
        .ok_or_else(|| anyhow::anyhow!("invalid node name '{node_name}'"))?;
    ParameterClient::new(conn.node.clone(), target)
        .map_err(|e| anyhow::anyhow!("failed to create parameter client: {e}"))
}

async fn list(conn: &Conn, g: &GlobalOpts, node_name: &str) -> Result<()> {
    let c = client(conn, node_name)?;
    let list = c
        .list(&[] as &[&str], None)
        .await
        .map_err(|e| anyhow::anyhow!("list failed: {e}"))?;
    let mut names = list.names;
    names.sort();

    emit(g.format, &json!(names), || {
        for n in &names {
            println!("  {n}");
        }
    })
}

async fn get(conn: &Conn, g: &GlobalOpts, node_name: &str, param: &str) -> Result<()> {
    let c = client(conn, node_name)?;
    let values = c
        .get(&[param])
        .await
        .map_err(|e| anyhow::anyhow!("get failed: {e}"))?;
    let value = values.into_iter().next().unwrap_or(ParameterValue::NotSet);

    emit(g.format, &value_to_json(&value), || {
        println!("{}", human_value(&value));
    })
}

async fn set(conn: &Conn, g: &GlobalOpts, node_name: &str, param: &str, raw: &str) -> Result<()> {
    let c = client(conn, node_name)?;
    let value = parse_value(raw)?;
    let result = c
        .set_atomically(&[Parameter::new(param, value)])
        .await
        .map_err(|e| anyhow::anyhow!("set failed: {e}"))?;

    let ok = result.successful;
    emit(
        g.format,
        &json!({"successful": ok, "reason": result.reason}),
        || {
            if ok {
                println!("Set parameter successful");
            } else {
                println!("Set parameter failed: {}", result.reason);
            }
        },
    )?;
    if !ok {
        anyhow::bail!("set rejected: {}", result.reason);
    }
    Ok(())
}

async fn delete(conn: &Conn, g: &GlobalOpts, node_name: &str, param: &str) -> Result<()> {
    let c = client(conn, node_name)?;
    let result = c
        .set_atomically(&[Parameter::new(param, ParameterValue::NotSet)])
        .await
        .map_err(|e| anyhow::anyhow!("delete failed: {e}"))?;
    let ok = result.successful;
    emit(
        g.format,
        &json!({"successful": ok, "reason": result.reason}),
        || {
            if ok {
                println!("Deleted parameter successful");
            } else {
                println!("Delete failed: {}", result.reason);
            }
        },
    )
}

async fn describe(conn: &Conn, g: &GlobalOpts, node_name: &str, param: &str) -> Result<()> {
    let c = client(conn, node_name)?;
    let descs = c
        .describe(&[param])
        .await
        .map_err(|e| anyhow::anyhow!("describe failed: {e}"))?;
    let d = descs
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("parameter '{param}' not found"))?;

    let value = json!({
        "name": d.name,
        "type": type_name(d.type_),
        "description": d.description,
        "constraints": d.additional_constraints,
        "read_only": d.read_only,
    });
    emit(g.format, &value, || {
        println!("Parameter name: {}", d.name);
        println!("  Type: {}", type_name(d.type_));
        if !d.description.is_empty() {
            println!("  Description: {}", d.description);
        }
        if !d.additional_constraints.is_empty() {
            println!("  Constraints: {}", d.additional_constraints);
        }
        println!("  Read only: {}", d.read_only);
    })
}

async fn dump(conn: &Conn, g: &GlobalOpts, node_name: &str) -> Result<()> {
    let c = client(conn, node_name)?;
    let list = c
        .list(&[] as &[&str], None)
        .await
        .map_err(|e| anyhow::anyhow!("list failed: {e}"))?;
    let mut names = list.names;
    names.sort();

    let values = if names.is_empty() {
        Vec::new()
    } else {
        c.get(&names)
            .await
            .map_err(|e| anyhow::anyhow!("get failed: {e}"))?
    };

    let mut params = serde_json::Map::new();
    for (name, value) in names.iter().zip(values.iter()) {
        params.insert(name.clone(), value_to_json(value));
    }
    let doc = json!({
        node_name: { "ros__parameters": serde_json::Value::Object(params.clone()) }
    });

    // Dump always renders as a YAML parameter file, regardless of --format,
    // to match `ros2 param dump`. JSON is available via --format json.
    match g.format {
        crate::cli::OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&doc)?),
        _ => print!("{}", serde_yaml::to_string(&doc)?),
    }
    Ok(())
}

fn type_name(t: ParameterType) -> &'static str {
    match t {
        ParameterType::NotSet => "not set",
        ParameterType::Bool => "bool",
        ParameterType::Integer => "integer",
        ParameterType::Double => "double",
        ParameterType::String => "string",
        ParameterType::ByteArray => "byte array",
        ParameterType::BoolArray => "bool array",
        ParameterType::IntegerArray => "integer array",
        ParameterType::DoubleArray => "double array",
        ParameterType::StringArray => "string array",
        _ => "unknown",
    }
}

fn value_to_json(v: &ParameterValue) -> serde_json::Value {
    match v {
        ParameterValue::NotSet => serde_json::Value::Null,
        ParameterValue::Bool(b) => json!(b),
        ParameterValue::Integer(i) => json!(i),
        ParameterValue::Double(d) => json!(d),
        ParameterValue::String(s) => json!(s),
        ParameterValue::ByteArray(a) => json!(a),
        ParameterValue::BoolArray(a) => json!(a),
        ParameterValue::IntegerArray(a) => json!(a),
        ParameterValue::DoubleArray(a) => json!(a),
        ParameterValue::StringArray(a) => json!(a),
    }
}

fn human_value(v: &ParameterValue) -> String {
    match v {
        ParameterValue::NotSet => "Parameter not set".to_string(),
        ParameterValue::Bool(b) => format!("Boolean value is: {b}"),
        ParameterValue::Integer(i) => format!("Integer value is: {i}"),
        ParameterValue::Double(d) => format!("Double value is: {d}"),
        ParameterValue::String(s) => format!("String value is: {s}"),
        other => format!("Value is: {}", value_to_json(other)),
    }
}

/// Parse a YAML scalar / sequence into a `ParameterValue`, matching
/// `ros2 param set` coercion (bool, int, float, string, or a homogeneous array).
fn parse_value(raw: &str) -> Result<ParameterValue> {
    let y: serde_yaml::Value =
        serde_yaml::from_str(raw).context("failed to parse value as YAML")?;
    Ok(match &y {
        serde_yaml::Value::Bool(b) => ParameterValue::Bool(*b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                ParameterValue::Integer(i)
            } else {
                ParameterValue::Double(n.as_f64().unwrap_or_default())
            }
        }
        serde_yaml::Value::String(s) => ParameterValue::String(s.clone()),
        serde_yaml::Value::Sequence(seq) => sequence_value(seq)?,
        serde_yaml::Value::Null => ParameterValue::NotSet,
        other => anyhow::bail!("unsupported parameter value: {other:?}"),
    })
}

fn sequence_value(seq: &[serde_yaml::Value]) -> Result<ParameterValue> {
    match seq.first() {
        None => Ok(ParameterValue::StringArray(Vec::new())),
        Some(serde_yaml::Value::Bool(_)) => Ok(ParameterValue::BoolArray(
            seq.iter().filter_map(|v| v.as_bool()).collect(),
        )),
        Some(serde_yaml::Value::Number(n)) if n.is_i64() => Ok(ParameterValue::IntegerArray(
            seq.iter().filter_map(|v| v.as_i64()).collect(),
        )),
        Some(serde_yaml::Value::Number(_)) => Ok(ParameterValue::DoubleArray(
            seq.iter().filter_map(|v| v.as_f64()).collect(),
        )),
        Some(serde_yaml::Value::String(_)) => Ok(ParameterValue::StringArray(
            seq.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
        )),
        Some(other) => anyhow::bail!("unsupported array element: {other:?}"),
    }
}
