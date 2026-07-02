//! Shared connection: a Zenoh session, a graph observer, and a hidden node for
//! dynamic pub/sub and parameter clients.
//!
//! This mirrors the proven wiring in `hiroz-console`'s `CoreEngine`: a
//! client-mode session connected to the configured router, a `Graph` built with
//! the backend-appropriate key-expression format, and a node created with the
//! type-description service enabled so dynamic subscribers can discover schemas.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use hiroz::Builder;
use hiroz::context::{ZContext, ZContextBuilder};
use hiroz::graph::Graph;
use hiroz::node::ZNode;

use crate::cli::Backend;

/// A live connection to a ROS 2 graph over Zenoh.
pub struct Conn {
    /// The underlying Zenoh session (kept alive for the graph subscriber).
    #[allow(dead_code)]
    pub session: Arc<zenoh::Session>,
    /// Graph observer for node/topic/service/action introspection.
    pub graph: Graph,
    /// The context backing `node` (kept alive so the node stays valid).
    #[allow(dead_code)]
    pub context: ZContext,
    /// Hidden CLI node used for dynamic pub/sub and parameter clients.
    pub node: Arc<ZNode>,
    #[allow(dead_code)]
    pub domain_id: usize,
}

impl Conn {
    /// Open a connection to `router` on `domain`, using `backend`'s key-expression
    /// format, then settle for `spin_time` so the graph can populate.
    pub async fn connect(
        router: &str,
        domain: usize,
        backend: Backend,
        spin_time: Duration,
    ) -> Result<Self> {
        let format = backend.key_expr_format();

        // Client mode + explicit router is required: hiroz has no multicast
        // discovery, and client mode is what reliably sees rmw_zenoh_cpp
        // liveliness tokens on the shared router.
        let mut config = zenoh::Config::default();
        config
            .insert_json5("mode", "\"client\"")
            .map_err(|e| anyhow::anyhow!("zenoh config (mode): {e}"))?;
        config
            .insert_json5("connect/endpoints", &format!("[\"{router}\"]"))
            .map_err(|e| anyhow::anyhow!("zenoh config (endpoints): {e}"))?;
        config
            .insert_json5("scouting/multicast/enabled", "false")
            .map_err(|e| anyhow::anyhow!("zenoh config (scouting): {e}"))?;

        let session = zenoh::open(config.clone())
            .await
            .map_err(|e| anyhow::anyhow!("failed to open Zenoh session on {router}: {e}"))?;
        let session = Arc::new(session);

        let graph = Graph::new(&session, domain, format)
            .map_err(|e| anyhow::anyhow!("failed to build graph observer: {e}"))?;

        // A separate context/node for dynamic subscribers and parameter clients.
        let context = ZContextBuilder::default()
            .with_domain_id(domain)
            .with_zenoh_config(config)
            .build()
            .map_err(|e| anyhow::anyhow!("failed to build context: {e}"))?;

        let node = context
            .create_node(format!("_hros2_cli_{}", std::process::id()))
            .with_type_description_service()
            .build()
            .map_err(|e| anyhow::anyhow!("failed to create CLI node: {e}"))?;
        let node = Arc::new(node);

        // Let liveliness tokens propagate before the first graph read.
        tokio::time::sleep(spin_time).await;

        Ok(Self {
            session,
            graph,
            context,
            node,
            domain_id: domain,
        })
    }
}
