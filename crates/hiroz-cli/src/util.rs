//! Small shared helpers.

use std::time::Duration;

use hiroz::entity::NodeKey;

/// Parse a possibly-fully-qualified node name into the graph's `NodeKey`
/// (normalized namespace, name). The root namespace is stored as an empty
/// string, matching `hiroz::entity::normalize_node_namespace`.
pub fn node_key_from_fqn(fqn: &str) -> NodeKey {
    let fqn = fqn.trim();
    let (ns, name) = match fqn.rfind('/') {
        Some(0) => ("/".to_string(), fqn[1..].to_string()),
        Some(idx) => (fqn[..idx].to_string(), fqn[idx + 1..].to_string()),
        None => ("/".to_string(), fqn.to_string()),
    };
    let ns = if ns == "/" { String::new() } else { ns };
    (ns, name)
}

/// Join a (denormalized) namespace and name into a fully-qualified name.
pub fn fqn(namespace: &str, name: &str) -> String {
    if namespace.is_empty() || namespace == "/" {
        format!("/{name}")
    } else {
        format!("{}/{name}", namespace.trim_end_matches('/'))
    }
}

/// A name is "hidden" (ros2 convention) if any path segment starts with '_'.
pub fn is_hidden(name: &str) -> bool {
    name.split('/').any(|seg| seg.starts_with('_'))
}

pub fn secs(f: f64) -> Duration {
    Duration::from_secs_f64(f.max(0.0))
}

/// Demangle a DDS-mangled type name into the ros2 `pkg/msg/Type` form.
///
/// `std_msgs::msg::dds_::String_` → `std_msgs/msg/String`. Names already in
/// slash form (from schema discovery) are returned unchanged.
pub fn demangle_type(t: &str) -> String {
    if !t.contains("::") {
        return t.to_string();
    }
    let mut parts: Vec<String> = t
        .split("::")
        .filter(|p| !p.is_empty() && *p != "dds_")
        .map(|p| p.to_string())
        .collect();
    if let Some(last) = parts.last_mut()
        && last.ends_with('_')
    {
        last.pop();
    }
    parts.join("/")
}
