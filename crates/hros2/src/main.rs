//! hros2 — a `ros2`-compatible command line clone built on the Hiroz stack.
//!
//! Speaks the `rmw_zenoh_cpp` wire format (Hiroz's default `KeyExprFormat::RmwZenoh`),
//! so it introspects a live ROS 2 graph by connecting to a Zenoh router and reading
//! the ROS 2 liveliness tokens — no ROS installation required.
//!
//! Supported today: `topic list`, `topic echo`, `node list`, `service list`.

mod format;

use std::num::NonZeroUsize;
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};
use hiroz::Builder;
use hiroz::context::ZContextBuilder;
use hiroz::entity::{Entity, EndpointKind};
use hiroz::qos::{QosDurability, QosHistory, QosProfile, QosReliability};
use hiroz_protocol::KeyExprFormat;
use hiroz_protocol::qos::{
    QosDurability as ProtoDurability, QosReliability as ProtoReliability,
};

/// hros2: ROS 2 CLI over Zenoh (Hiroz).
#[derive(Parser, Debug)]
#[command(name = "hros2", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Zenoh router endpoint to connect to.
    /// Defaults to $ROS2_ROUTER, else $ZENOH_CONNECT, else tcp/localhost:7447.
    #[arg(long, global = true)]
    router: Option<String>,

    /// ROS domain id. Defaults to $ROS_DOMAIN_ID, else 0.
    #[arg(long, global = true)]
    domain_id: Option<usize>,

    /// Discovery settle window in milliseconds: keep listening until the graph
    /// stops changing for this long (bounded by --timeout-ms).
    #[arg(long, global = true, default_value_t = 700)]
    settle_ms: u64,

    /// Hard upper bound on discovery time in milliseconds.
    #[arg(long, global = true, default_value_t = 5000)]
    timeout_ms: u64,

    /// Enable Hiroz/Zenoh internal logging (honors RUST_LOG).
    #[arg(long, global = true)]
    debug: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Introspect topics.
    Topic {
        #[command(subcommand)]
        action: TopicCmd,
    },
    /// Introspect nodes.
    Node {
        #[command(subcommand)]
        action: NodeCmd,
    },
    /// Introspect services.
    Service {
        #[command(subcommand)]
        action: ServiceCmd,
    },
}

#[derive(Subcommand, Debug)]
enum TopicCmd {
    /// Output a list of available topics.
    List(ListArgs),
    /// Output messages from a topic (decoded dynamically, no compiled type needed).
    Echo(EchoArgs),
}

#[derive(Args, Debug)]
struct EchoArgs {
    /// Topic to echo (e.g. /kuka/pose). Relative names are resolved against `/`.
    topic: String,

    /// Print one message and exit (alias for `--times 1`).
    #[arg(long, conflicts_with = "times")]
    once: bool,

    /// Print this many messages, then exit.
    #[arg(long, value_name = "N")]
    times: Option<u64>,

    /// Emit one compact JSON object per message (no `---` separator) instead of YAML.
    #[arg(long)]
    json: bool,

    /// Reliability QoS for the subscription (default: auto-detect from publishers).
    #[arg(long, value_enum, default_value_t = ReliabilityArg::SystemDefault)]
    qos_reliability: ReliabilityArg,

    /// Durability QoS for the subscription (default: auto-detect from publishers).
    #[arg(long, value_enum, default_value_t = DurabilityArg::SystemDefault)]
    qos_durability: DurabilityArg,

    /// History depth (KeepLast) for the subscription queue.
    #[arg(long, value_name = "N", default_value_t = 10)]
    qos_depth: usize,
}

/// `--qos-reliability` choices; `system_default` keeps the auto-detected value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
enum ReliabilityArg {
    #[default]
    SystemDefault,
    Reliable,
    BestEffort,
}

/// `--qos-durability` choices; `system_default` keeps the auto-detected value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
enum DurabilityArg {
    #[default]
    SystemDefault,
    Volatile,
    TransientLocal,
}

#[derive(Subcommand, Debug)]
enum NodeCmd {
    /// Output a list of available nodes.
    List(NodeListArgs),
}

#[derive(Subcommand, Debug)]
enum ServiceCmd {
    /// Output a list of available services.
    List(ListArgs),
}

#[derive(Args, Debug)]
struct ListArgs {
    /// Additionally show the message/service type of each entry.
    #[arg(short = 't', long)]
    show_types: bool,

    /// Include hidden entries (names with a leading-underscore token, e.g. action internals).
    #[arg(long)]
    include_hidden: bool,

    /// Number of matching entries.
    #[arg(short = 'c', long)]
    count: bool,
}

#[derive(Args, Debug)]
struct NodeListArgs {
    /// Include hidden nodes.
    #[arg(long)]
    include_hidden: bool,

    /// Number of matching nodes.
    #[arg(short = 'c', long)]
    count: bool,
}

/// A ROS 2 name is "hidden" when any of its `/`-separated tokens begins with `_`.
/// This is exactly how `ros2 topic list` / `ros2 service list` hide action-internal
/// (`/.../_action/...`) and service-event (`/.../_service_event`) endpoints.
fn is_hidden(name: &str) -> bool {
    name.split('/').any(|tok| tok.starts_with('_'))
}

fn resolve_router(cli: &Cli) -> String {
    cli.router
        .clone()
        .or_else(|| std::env::var("ROS2_ROUTER").ok())
        .or_else(|| std::env::var("ZENOH_CONNECT").ok())
        .unwrap_or_else(|| "tcp/localhost:7447".to_string())
}

fn resolve_domain(cli: &Cli) -> usize {
    cli.domain_id
        .or_else(|| std::env::var("ROS_DOMAIN_ID").ok().and_then(|v| v.parse().ok()))
        .unwrap_or(0)
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli).await {
        eprintln!("hros2: error: {e}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Route logs to stderr so command stdout (topic lists, echo YAML/JSON) stays pipe-clean.
    // Zenoh emits through the `tracing` facade, so our subscriber captures its logs too.
    if cli.debug {
        use tracing_subscriber::{EnvFilter, fmt};
        let _ = fmt()
            .with_writer(std::io::stderr)
            .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug")))
            .try_init();
    }

    let router = resolve_router(&cli);
    let domain = resolve_domain(&cli);

    // Like rmw_zenoh_cpp, Hiroz honors the ambient `ZENOH_CONFIG_OVERRIDE` env var, and it
    // is applied *after* the builder config — so it beats `.with_router_endpoint()`. When the
    // user is explicit about the endpoint (via --router / $ROS2_ROUTER / $ZENOH_CONNECT), make
    // that authoritative by rewriting the override to a plain peer pointing at our router.
    // Otherwise leave any ambient override untouched (respect the environment).
    let endpoint_is_explicit = cli.router.is_some()
        || std::env::var_os("ROS2_ROUTER").is_some()
        || std::env::var_os("ZENOH_CONNECT").is_some()
        || std::env::var_os("ZENOH_CONFIG_OVERRIDE").is_none();
    if endpoint_is_explicit {
        // SAFETY: single-threaded startup, before any Zenoh session/threads are spawned.
        unsafe {
            std::env::set_var(
                "ZENOH_CONFIG_OVERRIDE",
                format!("mode=\"peer\";connect/endpoints=[\"{router}\"]"),
            );
        }
    }

    let ctx = ZContextBuilder::default()
        .with_domain_id(domain)
        .keyexpr_format(KeyExprFormat::RmwZenoh)
        .with_router_endpoint(&router)?
        .build()?;

    let graph = ctx.graph().clone();

    // Let discovery converge: poll the graph until its entity count is stable for
    // `settle_ms`, or until `timeout_ms` elapses.
    settle(
        || {
            graph.get_topic_names_and_types().len()
                + graph.get_service_names_and_types().len()
                + graph.get_node_names().len()
        },
        Duration::from_millis(cli.settle_ms),
        Duration::from_millis(cli.timeout_ms),
    )
    .await;

    match cli.command {
        Command::Topic { action: TopicCmd::List(args) } => {
            let mut items = graph.get_topic_names_and_types();
            print_names_and_types(&mut items, &args);
        }
        Command::Topic { action: TopicCmd::Echo(args) } => {
            echo_topic(&ctx, &graph, &args, Duration::from_millis(cli.timeout_ms)).await?;
        }
        Command::Service { action: ServiceCmd::List(args) } => {
            let mut items = graph.get_service_names_and_types();
            print_names_and_types(&mut items, &args);
        }
        Command::Node { action: NodeCmd::List(args) } => {
            let mut names: Vec<String> = graph
                .get_node_names()
                .into_iter()
                .map(|(name, ns)| join_node_name(&ns, &name))
                .filter(|n| args.include_hidden || !is_hidden(n))
                .collect();
            names.sort();
            names.dedup();
            if args.count {
                println!("{}", names.len());
            } else {
                for n in names {
                    println!("{n}");
                }
            }
        }
    }

    let _ = ctx.shutdown();
    Ok(())
}

/// Format `(name, type)` pairs the way ros2cli does: sorted, de-duplicated,
/// hidden entries filtered unless requested, optionally annotated with `[type]`.
fn print_names_and_types(items: &mut Vec<(String, String)>, args: &ListArgs) {
    items.sort();
    items.dedup();

    let visible: Vec<&(String, String)> = items
        .iter()
        .filter(|(name, _)| args.include_hidden || !is_hidden(name))
        .collect();

    if args.count {
        println!("{}", visible.len());
        return;
    }

    for (name, type_name) in visible {
        if args.show_types {
            println!("{name} [{type_name}]");
        } else {
            println!("{name}");
        }
    }
}

/// Subscribe to `args.topic` and stream decoded messages (`ros2 topic echo` clone).
async fn echo_topic(
    ctx: &hiroz::context::ZContext,
    graph: &hiroz::graph::Graph,
    args: &EchoArgs,
    timeout: Duration,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // A node (with the type-description service) is needed for schema discovery; we don't
    // publish parameters, so skip those services.
    let node = ctx
        .create_node("hros2")
        .with_type_description_service()
        .without_parameters()
        .build()?;

    let qualified = qualify_topic(&args.topic);
    let qos = resolve_echo_qos(graph, &qualified, args);

    // create_dyn_sub_auto_qos preserves the publisher's remote type hash, so the sub's
    // key expression matches the publisher even though we override QoS.
    let sub = node
        .create_dyn_sub_auto_qos(&qualified, timeout, qos)
        .await
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            format!("could not subscribe to {qualified} (no publisher / schema discovery failed): {e}")
                .into()
        })?;

    // remaining: None = stream forever; Some(n) = stop after n messages.
    let mut remaining: Option<u64> = if args.once { Some(1) } else { args.times };

    loop {
        tokio::select! {
            biased;
            // Ctrl-C during streaming exits gracefully (0), matching ros2.
            _ = tokio::signal::ctrl_c() => break,
            msg = sub.async_recv() => match msg {
                Ok(m) => {
                    let value = format::dynamic_message_to_json(&m);
                    if args.json {
                        println!("{}", serde_json::to_string(&value)?);
                    } else {
                        print!("{}", serde_yaml::to_string(&value)?);
                        println!("---");
                    }
                    if let Some(r) = remaining.as_mut() {
                        *r -= 1;
                        if *r == 0 {
                            break;
                        }
                    }
                }
                // A decode error on one message shouldn't kill the stream (ros2 keeps going).
                Err(e) => eprintln!("hros2: echo: recv error: {e}"),
            }
        }
    }

    Ok(())
}

/// Qualify a topic name against the root namespace: absolute names pass through,
/// relative names get a leading `/` (hros2's node runs in namespace `/`).
fn qualify_topic(topic: &str) -> String {
    if topic.starts_with('/') {
        topic.to_string()
    } else {
        format!("/{topic}")
    }
}

/// Choose the subscription QoS the way `ros2 topic echo` does: auto-detect a profile
/// compatible with the current publishers, then apply any explicit `--qos-*` overrides.
fn resolve_echo_qos(graph: &hiroz::graph::Graph, qualified_topic: &str, args: &EchoArgs) -> QosProfile {
    let pub_qos: Vec<hiroz_protocol::qos::QosProfile> = graph
        .get_entities_by_topic(EndpointKind::Publisher, qualified_topic)
        .iter()
        .filter_map(|e| match e.as_ref() {
            Entity::Endpoint(ep) => Some(ep.qos),
            _ => None,
        })
        .collect();

    // Best-effort/transient-local only when EVERY publisher offers it (otherwise a
    // stricter reader would be incompatible); this mirrors ros2cli auto-detection.
    let auto_reliability = if !pub_qos.is_empty()
        && pub_qos.iter().all(|q| q.reliability == ProtoReliability::BestEffort)
    {
        QosReliability::BestEffort
    } else {
        QosReliability::Reliable
    };
    let auto_durability = if !pub_qos.is_empty()
        && pub_qos.iter().all(|q| q.durability == ProtoDurability::TransientLocal)
    {
        QosDurability::TransientLocal
    } else {
        QosDurability::Volatile
    };

    let reliability = match args.qos_reliability {
        ReliabilityArg::SystemDefault => auto_reliability,
        ReliabilityArg::Reliable => QosReliability::Reliable,
        ReliabilityArg::BestEffort => QosReliability::BestEffort,
    };
    let durability = match args.qos_durability {
        DurabilityArg::SystemDefault => auto_durability,
        DurabilityArg::Volatile => QosDurability::Volatile,
        DurabilityArg::TransientLocal => QosDurability::TransientLocal,
    };
    let depth = NonZeroUsize::new(args.qos_depth).unwrap_or(NonZeroUsize::new(10).unwrap());

    QosProfile {
        reliability,
        durability,
        history: QosHistory::KeepLast(depth),
        ..QosProfile::default()
    }
}

/// Join a node namespace and name into a fully-qualified ROS name (`/ns/name`).
fn join_node_name(namespace: &str, name: &str) -> String {
    if namespace.is_empty() || namespace == "/" {
        format!("/{name}")
    } else {
        let ns = namespace.trim_end_matches('/');
        format!("{ns}/{name}")
    }
}

/// Poll `count` until it holds steady for `settle`, or `timeout` elapses.
async fn settle<F: Fn() -> usize>(count: F, settle: Duration, timeout: Duration) {
    let poll = Duration::from_millis(100);
    let mut waited = Duration::ZERO;
    let mut last = count();
    let mut stable = Duration::ZERO;

    // Always give discovery at least one poll interval to receive liveliness replies.
    while waited < timeout {
        tokio::time::sleep(poll).await;
        waited += poll;

        let now = count();
        if now == last && now > 0 {
            stable += poll;
            if stable >= settle {
                return;
            }
        } else {
            stable = Duration::ZERO;
            last = now;
        }
    }
}
