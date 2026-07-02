//! `hros2 node` — node introspection.

use anyhow::Result;
use hiroz::entity::EndpointKind;
use serde_json::json;

use crate::cli::{GlobalOpts, NodeCmd};
use crate::conn::Conn;
use crate::output::emit;
use crate::util::{demangle_type, fqn, is_hidden, node_key_from_fqn};

pub async fn run(conn: &Conn, g: &GlobalOpts, cmd: NodeCmd) -> Result<()> {
    match cmd {
        NodeCmd::List { all, count_nodes } => list(conn, g, all, count_nodes),
        NodeCmd::Info { node_name } => info(conn, g, &node_name),
    }
}

fn list(conn: &Conn, g: &GlobalOpts, all: bool, count_nodes: bool) -> Result<()> {
    let mut names: Vec<String> = conn
        .graph
        .get_node_names()
        .into_iter()
        .map(|(name, ns)| fqn(&ns, &name))
        .filter(|n| all || !is_hidden(n))
        .collect();
    names.sort();
    names.dedup();

    if count_nodes {
        return emit(g.format, &json!(names.len()), || {
            println!("{}", names.len())
        });
    }
    emit(g.format, &json!(names), || {
        for n in &names {
            println!("{n}");
        }
    })
}

fn info(conn: &Conn, g: &GlobalOpts, node_name: &str) -> Result<()> {
    let key = node_key_from_fqn(node_name);
    if !conn.graph.node_exists(key.clone()) {
        anyhow::bail!("node '{node_name}' not found");
    }

    let dm = |v: Vec<(String, String)>| -> Vec<(String, String)> {
        v.into_iter().map(|(n, t)| (n, demangle_type(&t))).collect()
    };
    let pubs = dm(conn
        .graph
        .get_names_and_types_by_node(key.clone(), EndpointKind::Publisher));
    let subs = dm(conn
        .graph
        .get_names_and_types_by_node(key.clone(), EndpointKind::Subscription));
    let srvs = dm(conn
        .graph
        .get_names_and_types_by_node(key.clone(), EndpointKind::Service));
    let clis = dm(conn
        .graph
        .get_names_and_types_by_node(key.clone(), EndpointKind::Client));
    let action_servers = dm(conn
        .graph
        .get_action_server_names_and_types_by_node(key.clone()));
    let action_clients = dm(conn
        .graph
        .get_action_client_names_and_types_by_node(key.clone()));

    let pairs = |v: &[(String, String)]| -> serde_json::Value {
        json!(
            v.iter()
                .map(|(n, t)| json!({"name": n, "type": t}))
                .collect::<Vec<_>>()
        )
    };
    let value = json!({
        "node": node_name,
        "publishers": pairs(&pubs),
        "subscribers": pairs(&subs),
        "service_servers": pairs(&srvs),
        "service_clients": pairs(&clis),
        "action_servers": pairs(&action_servers),
        "action_clients": pairs(&action_clients),
    });

    emit(g.format, &value, || {
        println!("{node_name}");
        let section = |title: &str, v: &[(String, String)]| {
            println!("  {title}:");
            for (n, t) in v {
                println!("    {n}: {t}");
            }
        };
        section("Publishers", &pubs);
        section("Subscribers", &subs);
        section("Service Servers", &srvs);
        section("Service Clients", &clis);
        section("Action Servers", &action_servers);
        section("Action Clients", &action_clients);
    })
}
