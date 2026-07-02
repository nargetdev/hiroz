//! `hros2 topic` — topic introspection and pub/sub.

use std::time::Duration;

use anyhow::{Context, Result};
use hiroz::Builder;
use hiroz::entity::EndpointKind;
use serde_json::json;
use tokio::time::Instant;

use crate::cli::{GlobalOpts, OutputFormat, TopicCmd};
use crate::conn::Conn;
use crate::dynmsg;
use crate::output::{emit, emit_stream};
use crate::util::{demangle_type, is_hidden, secs};

pub async fn run(conn: &Conn, g: &GlobalOpts, cmd: TopicCmd) -> Result<()> {
    match cmd {
        TopicCmd::List {
            show_types,
            count_topics,
            include_hidden,
        } => list(conn, g, show_types, count_topics, include_hidden),
        TopicCmd::Info {
            topic_name,
            verbose,
        } => info(conn, g, &topic_name, verbose),
        TopicCmd::Type { topic_name } => type_of(conn, g, &topic_name),
        TopicCmd::Find { type_name } => find(conn, g, &type_name),
        TopicCmd::Echo {
            topic_name,
            once,
            times,
            field,
        } => echo(conn, g, &topic_name, once, times, field.as_deref()).await,
        TopicCmd::Hz { topic_name, window } => hz(conn, g, &topic_name, window).await,
        TopicCmd::Bw { topic_name, window } => bw(conn, g, &topic_name, window).await,
        TopicCmd::Pub {
            topic_name,
            type_name,
            values,
            once,
            rate,
            times,
        } => publish(conn, g, &topic_name, &type_name, &values, once, rate, times).await,
    }
}

fn list(
    conn: &Conn,
    g: &GlobalOpts,
    show_types: bool,
    count: bool,
    include_hidden: bool,
) -> Result<()> {
    let mut topics: Vec<(String, String)> = conn
        .graph
        .get_topic_names_and_types()
        .into_iter()
        .map(|(n, t)| (n, demangle_type(&t)))
        .filter(|(n, _)| include_hidden || !is_hidden(n))
        .collect();
    topics.sort();
    topics.dedup();

    if count {
        return emit(g.format, &json!(topics.len()), || {
            println!("{}", topics.len())
        });
    }
    let value = json!(
        topics
            .iter()
            .map(|(n, t)| json!({"name": n, "type": t}))
            .collect::<Vec<_>>()
    );
    emit(g.format, &value, || {
        for (n, t) in &topics {
            if show_types {
                println!("{n} [{t}]");
            } else {
                println!("{n}");
            }
        }
    })
}

fn info(conn: &Conn, g: &GlobalOpts, topic: &str, verbose: bool) -> Result<()> {
    let ty = conn
        .graph
        .get_topic_names_and_types()
        .into_iter()
        .find(|(n, _)| n == topic)
        .map(|(_, t)| demangle_type(&t));
    let publishers = conn
        .graph
        .get_entities_by_topic(EndpointKind::Publisher, topic);
    let subscribers = conn
        .graph
        .get_entities_by_topic(EndpointKind::Subscription, topic);

    let endpoint_json = |entities: &[std::sync::Arc<hiroz::entity::Entity>]| -> serde_json::Value {
        json!(
            entities
                .iter()
                .filter_map(|e| match &**e {
                    hiroz::entity::Entity::Endpoint(ep) => {
                        let node = ep.node.as_ref();
                        Some(json!({
                            "node": node.map(|n| crate::util::fqn(&n.namespace, &n.name)),
                            "type": ep.type_info.as_ref().map(|t| demangle_type(&t.name)),
                        }))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        )
    };

    let value = json!({
        "name": topic,
        "type": ty,
        "publisher_count": publishers.len(),
        "subscriber_count": subscribers.len(),
        "publishers": endpoint_json(&publishers),
        "subscribers": endpoint_json(&subscribers),
    });

    emit(g.format, &value, || {
        println!("Type: {}", ty.as_deref().unwrap_or("<unknown>"));
        println!("Publisher count: {}", publishers.len());
        println!("Subscription count: {}", subscribers.len());
        if verbose {
            let dump = |label: &str, entities: &[std::sync::Arc<hiroz::entity::Entity>]| {
                for e in entities {
                    if let hiroz::entity::Entity::Endpoint(ep) = &**e {
                        let node = ep
                            .node
                            .as_ref()
                            .map(|n| crate::util::fqn(&n.namespace, &n.name))
                            .unwrap_or_else(|| "<unknown>".to_string());
                        let t = ep
                            .type_info
                            .as_ref()
                            .map(|t| demangle_type(&t.name))
                            .unwrap_or_default();
                        println!("  [{label}] {node}: {t}");
                    }
                }
            };
            dump("pub", &publishers);
            dump("sub", &subscribers);
        }
    })
}

fn type_of(conn: &Conn, g: &GlobalOpts, topic: &str) -> Result<()> {
    match conn
        .graph
        .get_topic_names_and_types()
        .into_iter()
        .find(|(n, _)| n == topic)
        .map(|(_, t)| demangle_type(&t))
    {
        Some(t) => emit(g.format, &json!(t), || println!("{t}")),
        None => anyhow::bail!("topic '{topic}' not found"),
    }
}

fn find(conn: &Conn, g: &GlobalOpts, type_name: &str) -> Result<()> {
    let mut names: Vec<String> = conn
        .graph
        .get_topic_names_and_types()
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

async fn echo(
    conn: &Conn,
    g: &GlobalOpts,
    topic: &str,
    once: bool,
    times: usize,
    field: Option<&str>,
) -> Result<()> {
    let sub = conn
        .node
        .create_dyn_sub_auto(topic, secs(g.timeout))
        .await
        .map_err(|e| anyhow::anyhow!("failed to subscribe to {topic}: {e}"))?;

    if !g.quiet
        && g.format == OutputFormat::Human
        && let Some(schema) = sub.schema()
    {
        eprintln!("Type: {}", schema.type_name);
    }

    let limit = if once { 1 } else { times };
    let mut count = 0usize;
    loop {
        let msg = tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            m = sub.async_recv() => m.map_err(|e| anyhow::anyhow!("recv error: {e}"))?,
        };

        let full = dynmsg::message_to_json(&msg);
        let value = match field {
            Some(path) => select_field(&full, path)
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            None => full,
        };

        emit_stream(g.format, &value, || {
            if field.is_some() {
                println!("{}", render_scalar(&value));
            } else {
                print!("{}", dynmsg::format_pretty(&msg));
                println!("---");
            }
        })?;

        count += 1;
        if limit != 0 && count >= limit {
            break;
        }
    }
    Ok(())
}

/// Render a JSON scalar/compound for `--field` human output.
fn render_scalar(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn select_field<'a>(v: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    if path.is_empty() {
        return Some(v);
    }
    let mut cur = v;
    for seg in path.split('.') {
        cur = cur.get(seg)?;
    }
    Some(cur)
}

async fn hz(conn: &Conn, g: &GlobalOpts, topic: &str, window: usize) -> Result<()> {
    let sub = conn
        .node
        .create_dyn_sub_auto(topic, secs(g.timeout))
        .await
        .map_err(|e| anyhow::anyhow!("failed to subscribe to {topic}: {e}"))?;

    let window = window.max(2);
    let mut stamps: Vec<Instant> = Vec::with_capacity(window);
    loop {
        let recv = tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            r = sub.async_recv_serialized() => r,
        };
        if recv.is_err() {
            continue;
        }
        stamps.push(Instant::now());
        if stamps.len() < window {
            continue;
        }

        let deltas: Vec<f64> = stamps
            .windows(2)
            .map(|w| w[1].duration_since(w[0]).as_secs_f64())
            .collect();
        let n = deltas.len() as f64;
        let mean = deltas.iter().sum::<f64>() / n;
        let rate = if mean > 0.0 { 1.0 / mean } else { 0.0 };
        let min = deltas.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = deltas.iter().cloned().fold(0.0_f64, f64::max);
        let var = deltas.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / n;
        let std = var.sqrt();

        let value = json!({
            "topic": topic, "rate": rate, "min": min, "max": max,
            "std_dev": std, "window": deltas.len(),
        });
        emit_stream(g.format, &value, || {
            println!(
                "average rate: {rate:.3}\n\tmin: {min:.4}s max: {max:.4}s std dev: {std:.5}s window: {}",
                deltas.len()
            );
        })?;
        stamps.clear();
    }
    Ok(())
}

async fn bw(conn: &Conn, g: &GlobalOpts, topic: &str, window: f64) -> Result<()> {
    let sub = conn
        .node
        .create_dyn_sub_auto(topic, secs(g.timeout))
        .await
        .map_err(|e| anyhow::anyhow!("failed to subscribe to {topic}: {e}"))?;

    let window = Duration::from_secs_f64(window.max(0.1));
    loop {
        let mut bytes = 0usize;
        let mut msgs = 0usize;
        let start = Instant::now();
        let mut stop = false;
        while start.elapsed() < window {
            let remaining = window - start.elapsed();
            tokio::select! {
                _ = tokio::signal::ctrl_c() => { stop = true; break; }
                r = sub.async_recv_serialized() => {
                    if let Ok(sample) = r {
                        bytes += sample.payload().len();
                        msgs += 1;
                    }
                }
                _ = tokio::time::sleep(remaining) => {}
            }
        }
        let secs = start.elapsed().as_secs_f64();
        let bw = bytes as f64 / secs;
        let value = json!({
            "topic": topic, "bytes_per_sec": bw, "messages": msgs, "window_s": secs,
        });
        emit_stream(g.format, &value, || {
            println!("{:.2} B/s from {} messages over {:.1}s", bw, msgs, secs);
        })?;
        if stop {
            break;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn publish(
    conn: &Conn,
    g: &GlobalOpts,
    topic: &str,
    type_name: &str,
    values: &str,
    once: bool,
    rate: f64,
    times: usize,
) -> Result<()> {
    // Obtain the schema by discovering an existing publisher of the topic.
    // (Publishing to a topic that has no publisher yet requires the schema to be
    // supplied another way — see the crate README; that is planned library work.)
    let discovered = conn
        .node
        .discover_topic_schema(topic, secs(g.timeout))
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "could not discover schema for {topic} ({type_name}): {e}\n\
                 hint: `topic pub` currently needs an existing publisher of the topic \
                 so its type description can be fetched."
            )
        })?;
    let schema = discovered.schema;
    if schema.type_name != type_name {
        anyhow::bail!(
            "type mismatch: topic {topic} carries {}, but {type_name} was requested",
            schema.type_name
        );
    }

    let yaml: serde_yaml::Value =
        serde_yaml::from_str(values).context("failed to parse message YAML")?;
    let msg = dynmsg::build_message(&schema, &yaml)?;

    let publisher = conn
        .node
        .create_dyn_pub(topic, schema.clone())
        .build()
        .map_err(|e| anyhow::anyhow!("failed to create publisher: {e}"))?;

    if !g.quiet && g.format == OutputFormat::Human {
        eprintln!("publishing {} to {topic}", schema.type_name);
    }

    let limit = if once { 1 } else { times };
    let period = Duration::from_secs_f64(if rate > 0.0 { 1.0 / rate } else { 1.0 });
    let mut count = 0usize;
    loop {
        publisher
            .publish(&msg)
            .map_err(|e| anyhow::anyhow!("publish failed: {e}"))?;
        count += 1;
        if limit != 0 && count >= limit {
            break;
        }
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = tokio::time::sleep(period) => {}
        }
    }
    Ok(())
}
