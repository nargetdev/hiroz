//! `hros2 service` — service introspection (graph reads).

use anyhow::Result;
use hiroz::entity::EndpointKind;
use serde_json::json;

use crate::cli::{GlobalOpts, ServiceCmd};
use crate::conn::Conn;
use crate::output::emit;
use crate::util::{demangle_type, is_hidden};

pub async fn run(conn: &Conn, g: &GlobalOpts, cmd: ServiceCmd) -> Result<()> {
    match cmd {
        ServiceCmd::List {
            show_types,
            count_services,
        } => list(conn, g, show_types, count_services),
        ServiceCmd::Info { service_name } => info(conn, g, &service_name),
        ServiceCmd::Type { service_name } => type_of(conn, g, &service_name),
        ServiceCmd::Find { type_name } => find(conn, g, &type_name),
    }
}

fn list(conn: &Conn, g: &GlobalOpts, show_types: bool, count: bool) -> Result<()> {
    let mut svcs: Vec<(String, String)> = conn
        .graph
        .get_service_names_and_types()
        .into_iter()
        .map(|(n, t)| (n, demangle_type(&t)))
        .filter(|(n, _)| !is_hidden(n))
        .collect();
    svcs.sort();
    svcs.dedup();

    if count {
        return emit(g.format, &json!(svcs.len()), || println!("{}", svcs.len()));
    }
    let value = json!(
        svcs.iter()
            .map(|(n, t)| json!({"name": n, "type": t}))
            .collect::<Vec<_>>()
    );
    emit(g.format, &value, || {
        for (n, t) in &svcs {
            if show_types {
                println!("{n} [{t}]");
            } else {
                println!("{n}");
            }
        }
    })
}

fn info(conn: &Conn, g: &GlobalOpts, service_name: &str) -> Result<()> {
    let servers = conn
        .graph
        .count_by_service(EndpointKind::Service, service_name);
    let clients = conn
        .graph
        .count_by_service(EndpointKind::Client, service_name);
    let ty = conn
        .graph
        .get_service_names_and_types()
        .into_iter()
        .find(|(n, _)| n == service_name)
        .map(|(_, t)| demangle_type(&t));

    let value = json!({
        "name": service_name,
        "type": ty,
        "servers": servers,
        "clients": clients,
    });
    emit(g.format, &value, || {
        println!("{service_name}");
        if let Some(t) = &ty {
            println!("  Type: {t}");
        }
        println!("  Servers: {servers}");
        println!("  Clients: {clients}");
    })
}

fn type_of(conn: &Conn, g: &GlobalOpts, service_name: &str) -> Result<()> {
    let ty = conn
        .graph
        .get_service_names_and_types()
        .into_iter()
        .find(|(n, _)| n == service_name)
        .map(|(_, t)| demangle_type(&t));
    match ty {
        Some(t) => emit(g.format, &json!(t), || println!("{t}")),
        None => anyhow::bail!("service '{service_name}' not found"),
    }
}

fn find(conn: &Conn, g: &GlobalOpts, type_name: &str) -> Result<()> {
    let mut names: Vec<String> = conn
        .graph
        .get_service_names_and_types()
        .into_iter()
        .filter(|(_, t)| demangle_type(t) == type_name)
        .map(|(n, _)| n)
        .collect();
    names.sort();
    names.dedup();
    emit(g.format, &json!(names), || {
        for n in &names {
            println!("{n}");
        }
    })
}
