//! Command-line surface for `hros2`, defined with clap derive.

use clap::{Args, Parser, Subcommand, ValueEnum};

/// hros2 — a pure-Rust clone of the ros2 CLI, built on hiroz.
///
/// Introspect and interact with a live ROS 2 graph over a Zenoh router, with no
/// ROS 2 installation required. Interoperates with rmw_zenoh_cpp and hiroz nodes.
#[derive(Parser, Debug)]
#[command(name = "hros2", version, about, long_about = None)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalOpts,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Args, Debug, Clone)]
pub struct GlobalOpts {
    /// Zenoh router endpoint [env: HROS2_ROUTER]
    #[arg(
        short,
        long,
        global = true,
        env = "HROS2_ROUTER",
        default_value = "tcp/127.0.0.1:7447"
    )]
    pub router: String,

    /// ROS domain ID [env: ROS_DOMAIN_ID]
    #[arg(short, long, global = true, env = "ROS_DOMAIN_ID", default_value_t = 0)]
    pub domain: usize,

    /// Key-expression backend [env: HROS2_BACKEND]
    #[arg(long, global = true, env = "HROS2_BACKEND", value_enum, default_value_t = Backend::RmwZenoh)]
    pub backend: Backend,

    /// Output format
    #[arg(long, global = true, value_enum, default_value_t = OutputFormat::Human)]
    pub format: OutputFormat,

    /// Discovery / service timeout, in seconds
    #[arg(long, global = true, default_value_t = 5.0)]
    pub timeout: f64,

    /// Graph settle time before reads, in seconds
    #[arg(long, global = true, default_value_t = 1.0)]
    pub spin_time: f64,

    /// Reduce log output (errors only)
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Increase log output (repeatable)
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum Backend {
    /// rmw_zenoh_cpp / hiroz nodes (default)
    #[default]
    #[value(name = "rmw-zenoh")]
    RmwZenoh,
    /// Nodes bridged via zenoh-bridge-ros2dds
    #[value(name = "ros2dds")]
    Ros2Dds,
}

impl Backend {
    pub fn key_expr_format(self) -> hiroz_protocol::KeyExprFormat {
        match self {
            Backend::RmwZenoh => hiroz_protocol::KeyExprFormat::RmwZenoh,
            Backend::Ros2Dds => hiroz_protocol::KeyExprFormat::Ros2Dds,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum OutputFormat {
    #[default]
    Human,
    Json,
    Yaml,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Inspect nodes on the graph
    Node {
        #[command(subcommand)]
        cmd: NodeCmd,
    },
    /// Inspect and interact with topics
    Topic {
        #[command(subcommand)]
        cmd: TopicCmd,
    },
    /// Inspect services
    Service {
        #[command(subcommand)]
        cmd: ServiceCmd,
    },
    /// Inspect actions
    Action {
        #[command(subcommand)]
        cmd: ActionCmd,
    },
    /// Get and set node parameters
    Param {
        #[command(subcommand)]
        cmd: ParamCmd,
    },
}

#[derive(Subcommand, Debug)]
pub enum NodeCmd {
    /// List node names
    List {
        /// Include hidden nodes (leading underscore)
        #[arg(short = 'a', long)]
        all: bool,
        /// Print only the number of nodes
        #[arg(long)]
        count_nodes: bool,
    },
    /// Show a node's publishers, subscribers, services and actions
    Info {
        /// Fully-qualified node name, e.g. /talker
        node_name: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum TopicCmd {
    /// List topic names (and optionally types)
    List {
        /// Also print the topic type
        #[arg(short = 't', long)]
        show_types: bool,
        /// Print only the number of topics
        #[arg(short = 'c', long)]
        count_topics: bool,
        /// Include hidden topics (leading underscore segment)
        #[arg(long)]
        include_hidden: bool,
    },
    /// Show endpoint counts for a topic
    Info {
        topic_name: String,
        /// Show per-endpoint node names and QoS
        #[arg(long)]
        verbose: bool,
    },
    /// Print a topic's type
    Type { topic_name: String },
    /// List topics of a given type
    Find { type_name: String },
    /// Subscribe to a topic and print messages
    Echo {
        topic_name: String,
        /// Print one message and exit
        #[arg(long)]
        once: bool,
        /// Print N messages and exit (0 = unbounded)
        #[arg(long, default_value_t = 0)]
        times: usize,
        /// Print only this dotted field path (e.g. linear.x)
        #[arg(long)]
        field: Option<String>,
    },
    /// Measure a topic's publishing rate
    Hz {
        topic_name: String,
        /// Number of samples per reported window
        #[arg(short = 'w', long, default_value_t = 50)]
        window: usize,
    },
    /// Measure a topic's bandwidth
    Bw {
        topic_name: String,
        /// Reporting window in seconds
        #[arg(short = 'w', long, default_value_t = 1.0)]
        window: f64,
    },
    /// Publish a message to a topic (YAML values)
    Pub {
        topic_name: String,
        /// Message type, e.g. std_msgs/msg/String
        type_name: String,
        /// Message contents as YAML (defaults to an all-zero message)
        #[arg(default_value = "{}")]
        values: String,
        /// Publish once and exit
        #[arg(short = '1', long)]
        once: bool,
        /// Publish rate in Hz
        #[arg(long, default_value_t = 1.0)]
        rate: f64,
        /// Publish N times and exit (0 = unbounded)
        #[arg(long, default_value_t = 0)]
        times: usize,
    },
}

#[derive(Subcommand, Debug)]
pub enum ServiceCmd {
    /// List service names (and optionally types)
    List {
        #[arg(short = 't', long)]
        show_types: bool,
        #[arg(short = 'c', long)]
        count_services: bool,
    },
    /// Show server/client counts for a service
    Info { service_name: String },
    /// Print a service's type
    Type { service_name: String },
    /// List services of a given type
    Find { type_name: String },
}

#[derive(Subcommand, Debug)]
pub enum ActionCmd {
    /// List action names (and optionally types)
    List {
        #[arg(short = 't', long)]
        show_types: bool,
    },
    /// Show client/server counts for an action
    Info { action_name: String },
    /// Print an action's type
    Type { action_name: String },
}

#[derive(Subcommand, Debug)]
pub enum ParamCmd {
    /// List a node's parameters
    List {
        /// Fully-qualified node name, e.g. /talker
        node_name: String,
    },
    /// Get a parameter value
    Get {
        node_name: String,
        param_name: String,
    },
    /// Set a parameter value (YAML scalar)
    Set {
        node_name: String,
        param_name: String,
        value: String,
    },
    /// Describe a parameter
    Describe {
        node_name: String,
        param_name: String,
    },
    /// Dump all of a node's parameters as YAML
    Dump { node_name: String },
    /// Delete (unset) a parameter
    Delete {
        node_name: String,
        param_name: String,
    },
}
