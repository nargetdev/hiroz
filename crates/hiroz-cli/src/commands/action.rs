//! `hros2 action` — action introspection (graph reads).
//!
//! Action names/types are derived from the graph's feedback-topic convention
//! (see `Graph::get_action_names_and_types`). Counting servers/clients reuses
//! the per-node action queries across all nodes.

use anyhow::Result;
use serde_json::json;

use crate::cli::{ActionCmd, GlobalOpts};
use crate::conn::Conn;
use crate::output::emit;
use crate::util::{demangle_type, node_key_from_fqn};

pub async fn run(conn: &Conn, g: &GlobalOpts, cmd: ActionCmd) -> Result<()> {
    match cmd {
        ActionCmd::List { show_types } => list(conn, g, show_types),
        ActionCmd::Info { action_name } => info(conn, g, &action_name),
        ActionCmd::Type { action_name } => type_of(conn, g, &action_name),
    }
}

fn list(conn: &Conn, g: &GlobalOpts, show_types: bool) -> Result<()> {
    let actions: Vec<(String, String)> = conn
        .graph
        .get_action_names_and_types()
        .into_iter()
        .map(|(n, t)| (n, demangle_type(&t)))
        .collect();
    let value = json!(
        actions
            .iter()
            .map(|(n, t)| json!({"name": n, "type": t}))
            .collect::<Vec<_>>()
    );
    emit(g.format, &value, || {
        for (n, t) in &actions {
            if show_types {
                println!("{n} [{t}]");
            } else {
                println!("{n}");
            }
        }
    })
}

/// Count servers and clients of `action_name` by scanning every node.
fn count_endpoints(conn: &Conn, action_name: &str) -> (Vec<String>, Vec<String>) {
    let mut servers = Vec::new();
    let mut clients = Vec::new();
    for (name, ns) in conn.graph.get_node_names() {
        let key = node_key_from_fqn(&crate::util::fqn(&ns, &name));
        if conn
            .graph
            .get_action_server_names_and_types_by_node(key.clone())
            .iter()
            .any(|(a, _)| a == action_name)
        {
            servers.push(crate::util::fqn(&ns, &name));
        }
        if conn
            .graph
            .get_action_client_names_and_types_by_node(key)
            .iter()
            .any(|(a, _)| a == action_name)
        {
            clients.push(crate::util::fqn(&ns, &name));
        }
    }
    servers.sort();
    servers.dedup();
    clients.sort();
    clients.dedup();
    (servers, clients)
}

fn info(conn: &Conn, g: &GlobalOpts, action_name: &str) -> Result<()> {
    let ty = conn
        .graph
        .get_action_names_and_types()
        .into_iter()
        .find(|(n, _)| n == action_name)
        .map(|(_, t)| demangle_type(&t));
    let (servers, clients) = count_endpoints(conn, action_name);

    let value = json!({
        "name": action_name,
        "type": ty,
        "action_servers": servers,
        "action_clients": clients,
    });
    emit(g.format, &value, || {
        println!("Action: {action_name}");
        if let Some(t) = &ty {
            println!("  Type: {t}");
        }
        println!("Action servers: {}", servers.len());
        for s in &servers {
            println!("    {s}");
        }
        println!("Action clients: {}", clients.len());
        for c in &clients {
            println!("    {c}");
        }
    })
}

fn type_of(conn: &Conn, g: &GlobalOpts, action_name: &str) -> Result<()> {
    let ty = conn
        .graph
        .get_action_names_and_types()
        .into_iter()
        .find(|(n, _)| n == action_name)
        .map(|(_, t)| demangle_type(&t));
    match ty {
        Some(t) => emit(g.format, &json!(t), || println!("{t}")),
        None => anyhow::bail!("action '{action_name}' not found"),
    }
}
