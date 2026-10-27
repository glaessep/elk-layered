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

Version 0.1.0 is in development. The crate is being turned from a building block of another
project into a library of its own. The open issues of this repository list the steps:

- one entry point, `layout(graph)`, shaped like elkjs's JSON graph
- ports in the input, including ports across the flow (north and south)
- a model with typed ids and private fields
- one file per ELK class

Until 0.1.0 is released, the public API changes without notice.

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
