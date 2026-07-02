//! hros2 — a pure-Rust clone of the ros2 CLI, built on hiroz.
//!
//! A thin clap frontend over the hiroz library: graph introspection, dynamic
//! pub/sub, and parameter clients, all over a Zenoh router with no ROS 2
//! installation required.

mod cli;
mod commands;
mod conn;
mod dynmsg;
mod output;
mod util;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Command};
use conn::Conn;
use util::secs;

#[tokio::main]
async fn main() -> Result<()> {
    // Restore default SIGPIPE handling so `hros2 ... | head` terminates
    // silently like a standard Unix tool instead of panicking on write.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let args = Cli::parse();
    init_logging(&args.global);

    let g = args.global.clone();
    let conn = Conn::connect(&g.router, g.domain, g.backend, secs(g.spin_time)).await?;

    let result = match args.command {
        Command::Node { cmd } => commands::node::run(&conn, &g, cmd).await,
        Command::Topic { cmd } => commands::topic::run(&conn, &g, cmd).await,
        Command::Service { cmd } => commands::service::run(&conn, &g, cmd).await,
        Command::Action { cmd } => commands::action::run(&conn, &g, cmd).await,
        Command::Param { cmd } => commands::param::run(&conn, &g, cmd).await,
    };

    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
    Ok(())
}

fn init_logging(g: &cli::GlobalOpts) {
    use tracing_subscriber::{EnvFilter, fmt};

    let default = if g.quiet {
        "error"
    } else {
        match g.verbose {
            0 => "warn",
            1 => "info",
            2 => "debug",
            _ => "trace",
        }
    };
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("hros2={default},hiroz={default}")));
    let _ = fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}
