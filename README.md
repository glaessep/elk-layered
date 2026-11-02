# elk-layered

Eclipse ELK's layered layout algorithm, in Rust. It takes a graph of boxes, ports and edges and
gives every box a position and every edge a route at right angles, the way
[Eclipse ELK](https://eclipse.dev/elk/) and [elkjs](https://github.com/kieler/elkjs) do.

**The goal is the same geometry as elkjs 0.9.3 for the same input.** elkjs is ELK's Java compiled
to JavaScript. Where it behaves differently from Java on a JVM, this crate follows elkjs.

**Read the JSON with an exact number parser.** JavaScript reads every number to the nearest
double. Plain `serde_json` may read a number such as `127.58333333333333` one bit off, and one bit
in the input can change the layout. If you parse the graph with `serde_json`, turn on its
`float_roundtrip` feature. With another parser, use one that reads to the nearest double. This
crate sets `float_roundtrip` only for its own tests.

## Status

Version 0.1.1 is the current release. Version 0.1.0 was the first release of the crate as a
library of its own. It started as a building block of another project. Until 1.0, a change to the
public API raises the minor version.

### Changes in 0.1.1

The public API is the same as in 0.1.0. What changed is which graphs `layout()` accepts and the
numbers it returns.

New in what `layout()` accepts:

- Ports with labels, placed by `elk.portLabels.placement` and `elk.portLabels.treatAsGroup`. This
  holds for nodes with children too.
- `PORTS` and `PORT_LABELS` in `elk.nodeSize.constraints`, so a node, or a node with children,
  sizes itself to its ports and their labels.
- Ports on the north and south side of a node with children.
- Self loops routed inside their node, with `elk.insideSelfLoops.activate` and
  `elk.insideSelfLoops.yo`.

New refusals, where elkjs fails or the crate cannot match it yet:

- An inside self loop at a port.
- An inside self loop in a node that is laid out on its own.
- A labelled port of the root graph under inside port labels.

Layouts that now match elkjs 0.9.3, where 0.1.0 gave other numbers:

- The one-sided greedy crossing switch counts crossings as elkjs does, so it switches the same nodes.
- The orthogonal router rates a free area as elkjs does, so a connecting segment can sit at another
  height.
- Packed components: the row width keeps `sqrt(area)` a double, as elkjs does, not an f32.
- Ports are sorted with GWT's merge sort, and barycenters compare as doubles, as in elkjs.
- With direction `UP` or `DOWN`, an external port on the north or south side keeps its first or
  last layer.
- Two end labels at one port, from edges that come from different groups, come in elkjs's order.
- A label moved to a segment of an edge across the hierarchy turns on both label steps of its
  graph, as elkjs does.
- North and south port dummies name the first port of the node's list, as Java does.
- A breaking point node gets its own spacing, not the spacing of a normal node.

Errors and panics fixed:

- A graph whose nested graphs reuse a node id or a name no longer mixes up their ports, their depth
  or their offset.
- An edge in a container without a graph is an error, not an edge of the root.
- Some graphs with ports on the north or south side, which failed or panicked in 0.1.0, now lay out.
- A long chain of nodes, such as 10,000 in a row, no longer overflows the stack.

Inside the crate: typed ids and private fields, one file per ELK class, every elkjs quirk in
`gwt.rs`, and the unit tests in `tests.rs` files.

### Changes in 0.1.0

- One entry point, `layout(graph)`, shaped like elkjs's JSON graph.
- Ports in the input, including ports across the flow (north and south).
- A parity suite holds `layout()` to what elkjs 0.9.3 returns, number for number.

## Where the code comes from

| | |
| --- | --- |
| Algorithm | [Eclipse ELK](https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e), commit `62d5909`, the ELK revision elkjs 0.9.3 is built from |
| Reference behaviour | [elkjs](https://github.com/kieler/elkjs) 0.9.3 |
| Rust port this repository starts from | `merman-elk-layered` 0.9.2 by Mingzhen Zhuang, from [Latias94/merman](https://github.com/Latias94/merman) at commit `8e303a4`, path `crates/merman-elk-layered` |

The first commit of this repository imports that crate unchanged. Every change since then is its
own commit, so `git log` shows each change made to the EPL-2.0 files.

## Licence

[Eclipse Public License 2.0](LICENSE), like Eclipse ELK, elkjs and the port this repository
starts from. Every file in this repository is under EPL-2.0.

## Development

```bash
cargo build
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

The Rust version is pinned in `rust-toolchain.toml`. [CLAUDE.md](CLAUDE.md) holds the rules for
changes, for agents and humans alike.
