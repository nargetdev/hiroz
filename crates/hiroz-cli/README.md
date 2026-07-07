# hiroz-cli — the `hros2` command-line tool

A pure-Rust clone of the `ros2` CLI, built on the hiroz stack. It introspects
and interacts with a live ROS 2 graph over a Zenoh router — no ROS 2
installation, no Python, no DDS. Interoperates with `rmw_zenoh_cpp` and hiroz
nodes on the same router.

See the design spec and command reference at
[`docs/tools/hros2-cli-spec.md`](../../docs/tools/hros2-cli-spec.md).

## Build & run

```bash
cargo build -p hiroz-cli --release
./target/release/hros2 --help

# against a router on tcp/127.0.0.1:7447, domain 0 (defaults)
hros2 node list
hros2 topic list -t
hros2 topic echo /chatter
hros2 topic echo --format json /cmd_vel
hros2 topic pub /chatter std_msgs/msg/String '{data: hello}'
hros2 param get /talker use_sim_time
```

Global options (`-r/--router`, `-d/--domain`, `--backend`, `--format`,
`--timeout`, `--spin-time`) work on every subcommand and also read the
`HROS2_ROUTER`, `ROS_DOMAIN_ID`, and `HROS2_BACKEND` environment variables.

## Implemented commands

| Group | Verbs |
|-------|-------|
| `node` | `list`, `info` |
| `topic` | `list`, `info`, `type`, `find`, `echo`, `hz`, `bw`, `pub` |
| `service` | `list`, `info`, `type`, `find` |
| `action` | `list`, `info`, `type` |
| `param` | `list`, `get`, `set`, `describe`, `dump`, `delete` |

All commands support `--format {human,json,yaml}`. Streaming commands
(`echo`, `hz`, `bw`) emit one JSON object per line under `--format json`.

## Notes and current limitations

- **Dynamic types.** `topic echo`/`hz`/`bw` discover the message schema at
  runtime via the ROS 2 Type Description service (REP-2016), so they work with
  any type without local `.msg` files — provided a publisher on the topic
  offers that service (hiroz nodes built `.with_type_description_service()` and
  all `rmw_zenoh_cpp` nodes do).
- **`topic pub`** obtains its schema by discovering an existing publisher of the
  topic. Publishing to a topic that has no publisher yet needs the schema
  supplied another way; a schema-registry / bundled-asset path is planned
  library work (see the spec's "Required library work").
- **Not yet implemented** (need new hiroz library APIs, tracked in the spec):
  `service call`, `action send_goal`, `interface *`, `lifecycle *`, `doctor`.
- **Distro** is a compile-time feature inherited from hiroz (`jazzy` by
  default). Build per-distro binaries via the `humble` / `kilted` / `lyrical`
  features.
