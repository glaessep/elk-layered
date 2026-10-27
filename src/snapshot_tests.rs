//! Every coordinate of a set of layouts, held to a committed snapshot.
//!
//! This is the safety net for refactoring: a change that moves one box, one bend point or one
//! label by one bit fails here. Each case is a small graph that exercises one part of the
//! algorithm. The snapshot is written in absolute coordinates: a nested graph's content is
//! moved by its parent node's position, the nested graph's offset and its padding.
//!
//! After a change that is meant to move the layout, regenerate and read the diff:
//!
//! ```text
//! UPDATE_SNAPSHOTS=1 cargo test snapshot_tests
//! ```

use std::fmt::Write as _;
use std::path::PathBuf;

use crate::graph::{EdgeLabelPlacement, LGraph, LNode, LPoint};
use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputLabel, ElkInputNode, import_graph};
use crate::options::{
    CycleBreakingStrategy, ElkDirection, FixedAlignment, HierarchyHandling, LayeredOptions,
    NodeFlexibility, NodeLabelPlacement, OrderingStrategy,
};
use crate::pipeline::{execute_ported_compound_processors, execute_ported_processors};

pub(crate) fn node(id: &str, width: f64, height: f64, parent: Option<&str>) -> ElkInputNode {
    ElkInputNode {
        ports: Vec::new(),
        parent_settings: Default::default(),
        id: id.into(),
        width,
        height,
        parent: parent.map(Into::into),
        direction: None,
        hierarchy_handling: None,
        layer_constraint: None,
        port_constraints: None,
        port_alignment: None,
        node_label_placement: NodeLabelPlacement::default(),
        node_flexibility: NodeFlexibility::default(),
        ports_surrounding: None,
        nested_options: None,
        labels: Vec::new(),
        node_size: Default::default(),
        self_loop_distribution: Default::default(),
        self_loop_ordering: Default::default(),
    }
}

pub(crate) fn edge(id: &str, source: &str, target: &str) -> ElkInputEdge {
    ElkInputEdge {
        source_port: None,
        target_port: None,
        id: id.into(),
        source: source.into(),
        target: target.into(),
        labels: Vec::new(),
        minlen: 1,
        inside_self_loops_yo: false,
        model_order: None,
        priority_direction: 0,
        priority_shortness: 0,
        priority_straightness: 0,
        container: None,
    }
}

fn labelled(mut e: ElkInputEdge, text: &str, placement: EdgeLabelPlacement) -> ElkInputEdge {
    let mut label = ElkInputLabel::center(text, 6.5 * text.len() as f64, 14.0);
    label.placement = placement;
    e.labels.push(label);
    e
}

fn graph(
    options: LayeredOptions,
    nodes: Vec<ElkInputNode>,
    edges: Vec<ElkInputEdge>,
) -> ElkInputGraph {
    ElkInputGraph {
        id: "root".into(),
        options,
        nodes,
        edges,
    }
}

fn boxes(ids: &[&str]) -> Vec<ElkInputNode> {
    ids.iter()
        .enumerate()
        .map(|(i, id)| {
            node(
                id,
                60.0 + 10.0 * (i % 3) as f64,
                30.0 + 5.0 * (i % 2) as f64,
                None,
            )
        })
        .collect()
}

fn cases() -> Vec<(&'static str, ElkInputGraph)> {
    let elk = LayeredOptions::default;
    let mut cases = Vec::new();

    cases.push((
        "chain",
        graph(
            elk(),
            boxes(&["a", "b", "c"]),
            vec![edge("e0", "a", "b"), edge("e1", "b", "c")],
        ),
    ));
    cases.push((
        "diamond",
        graph(
            elk(),
            boxes(&["a", "b", "c", "d"]),
            vec![
                edge("e0", "a", "b"),
                edge("e1", "a", "c"),
                edge("e2", "b", "d"),
                edge("e3", "c", "d"),
            ],
        ),
    ));
    cases.push((
        "cycle",
        graph(
            elk(),
            boxes(&["a", "b", "c", "d"]),
            vec![
                edge("e0", "a", "b"),
                edge("e1", "b", "c"),
                edge("e2", "c", "a"),
                edge("e3", "c", "d"),
            ],
        ),
    ));
    cases.push((
        "long-edges",
        graph(
            elk(),
            boxes(&["a", "b", "c", "d"]),
            vec![
                edge("e0", "a", "b"),
                edge("e1", "b", "c"),
                edge("e2", "c", "d"),
                edge("e3", "a", "d"),
            ],
        ),
    ));
    cases.push((
        "fan",
        graph(
            elk(),
            boxes(&["a", "b", "c", "d", "e", "f", "g"]),
            ["b", "c", "d", "e", "f", "g"]
                .iter()
                .enumerate()
                .map(|(i, t)| edge(&format!("e{i}"), "a", t))
                .collect(),
        ),
    ));
    cases.push((
        "labels",
        graph(
            elk(),
            boxes(&["a", "b", "c"]),
            vec![
                labelled(edge("e0", "a", "b"), "pedal %", EdgeLabelPlacement::Center),
                labelled(
                    labelled(edge("e1", "b", "c"), "1", EdgeLabelPlacement::Tail),
                    "0..*",
                    EdgeLabelPlacement::Head,
                ),
            ],
        ),
    ));
    cases.push((
        "self-loop",
        graph(
            elk(),
            boxes(&["a", "b"]),
            vec![edge("e0", "a", "a"), edge("e1", "a", "b")],
        ),
    ));
    cases.push((
        "parallel-edges",
        graph(
            elk(),
            boxes(&["a", "b"]),
            vec![
                edge("e0", "a", "b"),
                edge("e1", "a", "b"),
                edge("e2", "b", "a"),
            ],
        ),
    ));
    cases.push((
        "components",
        graph(
            elk(),
            boxes(&["a", "b", "c", "d", "e"]),
            vec![edge("e0", "a", "b"), edge("e1", "c", "d")],
        ),
    ));
    cases.push((
        "down-with-labels",
        graph(
            LayeredOptions {
                direction: ElkDirection::Down,
                ..elk()
            },
            boxes(&["a", "b", "c", "d"]),
            vec![
                labelled(edge("e0", "a", "b"), "go", EdgeLabelPlacement::Center),
                labelled(edge("e1", "a", "c"), "stop", EdgeLabelPlacement::Center),
                edge("e2", "b", "d"),
                edge("e3", "c", "d"),
            ],
        ),
    ));
    cases.push((
        "model-order",
        graph(
            LayeredOptions {
                consider_model_order_strategy: OrderingStrategy::NodesAndEdges,
                ..elk()
            },
            boxes(&["a", "z", "m", "b"]),
            vec![
                edge("e0", "a", "z"),
                edge("e1", "a", "m"),
                edge("e2", "a", "b"),
            ],
        ),
    ));
    cases.push((
        "depth-first-balanced",
        graph(
            LayeredOptions {
                cycle_breaking_strategy: CycleBreakingStrategy::DepthFirst,
                node_placement_bk_fixed_alignment: FixedAlignment::Balanced,
                ..elk()
            },
            boxes(&["a", "b", "c", "d", "e"]),
            vec![
                edge("e0", "a", "b"),
                edge("e1", "b", "c"),
                edge("e2", "c", "a"),
                edge("e3", "a", "d"),
                edge("e4", "d", "e"),
            ],
        ),
    ));

    let mut nested = vec![
        node("outer", 0.0, 0.0, None),
        node("x", 60.0, 30.0, Some("outer")),
    ];
    nested.extend([
        node("y", 70.0, 30.0, Some("outer")),
        node("p", 60.0, 30.0, None),
        node("q", 60.0, 30.0, None),
    ]);
    cases.push((
        "include-children",
        graph(
            LayeredOptions {
                hierarchy_handling: HierarchyHandling::IncludeChildren,
                ..elk()
            },
            nested,
            vec![
                edge("e0", "p", "x"),
                edge("e1", "x", "y"),
                edge("e2", "y", "q"),
            ],
        ),
    ));

    let deep = vec![
        node("g1", 0.0, 0.0, None),
        node("g2", 0.0, 0.0, Some("g1")),
        node("a", 60.0, 30.0, Some("g2")),
        node("b", 60.0, 30.0, Some("g1")),
        node("c", 60.0, 30.0, None),
    ];
    cases.push((
        "include-children-deep",
        graph(
            LayeredOptions {
                hierarchy_handling: HierarchyHandling::IncludeChildren,
                ..elk()
            },
            deep,
            vec![
                edge("e0", "a", "b"),
                edge("e1", "b", "c"),
                edge("e2", "c", "a"),
            ],
        ),
    ));

    cases
}

/// Lays a case out with the pipeline its graph needs: the compound pipeline when a node holds
/// others or the graph lays out its children with it, the flat pipeline otherwise.
fn lay_out(input: &ElkInputGraph) -> LGraph {
    let mut graph = import_graph(input).expect("every case is a valid graph");
    let compound = graph.options.hierarchy_handling == HierarchyHandling::IncludeChildren
        || graph.graph_properties.external_ports
        || !graph.hierarchy_edges.is_empty()
        || !graph.cross_hierarchy_edges.is_empty()
        || graph
            .layerless_nodes
            .iter()
            .any(|n| n.nested_graph.is_some());
    if compound {
        execute_ported_compound_processors(&mut graph).expect("every case lays out");
    } else {
        execute_ported_processors(&mut graph).expect("every case lays out");
    }
    graph
}

fn point(p: LPoint) -> String {
    format!("({:?}, {:?})", p.x, p.y)
}

fn add(a: LPoint, b: LPoint) -> LPoint {
    LPoint {
        x: a.x + b.x,
        y: a.y + b.y,
    }
}

/// Writes one graph level, then recurses into the graphs its nodes hold.
fn dump(graph: &LGraph, origin: LPoint, depth: usize, out: &mut String) {
    let pad = "  ".repeat(depth);
    let _ = writeln!(
        out,
        "{pad}graph {} size {:?}x{:?} padding {:?} {:?} {:?} {:?}",
        graph.id,
        graph.size.width,
        graph.size.height,
        graph.padding.top,
        graph.padding.right,
        graph.padding.bottom,
        graph.padding.left
    );
    for node in &graph.layerless_nodes {
        let at = add(origin, node.position);
        let _ = writeln!(
            out,
            "{pad}  node {} {:?} at {} size {:?}x{:?}",
            node.id,
            node.kind,
            point(at),
            node.size.width,
            node.size.height
        );
        for port in &node.ports {
            let _ = writeln!(
                out,
                "{pad}    port {} {:?} at {} anchor {}",
                port.id,
                port.side,
                point(add(at, port.position)),
                point(port.anchor)
            );
        }
        for label in &node.labels {
            let _ = writeln!(
                out,
                "{pad}    label {:?} at {}",
                label.text,
                point(add(at, label.position))
            );
        }
    }
    for edge in &graph.edges {
        let source = &graph.layerless_nodes[edge.source.node];
        let target = &graph.layerless_nodes[edge.target.node];
        let end = |n: &LNode, p: usize| {
            let port = &n.ports[p];
            add(add(add(origin, n.position), port.position), port.anchor)
        };
        let bends: Vec<String> = edge
            .bend_points
            .iter()
            .map(|p| point(add(origin, *p)))
            .collect();
        let _ = writeln!(
            out,
            "{pad}  edge {} from {} via [{}] to {}",
            edge.id,
            point(end(source, edge.source.port)),
            bends.join(", "),
            point(end(target, edge.target.port))
        );
        for label in &edge.labels {
            let _ = writeln!(
                out,
                "{pad}    label {:?} {:?} at {}",
                label.text,
                label.placement,
                point(add(origin, label.position))
            );
        }
    }
    for node in &graph.layerless_nodes {
        if let Some(nested) = &node.nested_graph {
            let inner = add(
                add(add(origin, node.position), nested.offset),
                LPoint {
                    x: nested.padding.left,
                    y: nested.padding.top,
                },
            );
            dump(nested, inner, depth + 1, out);
        }
    }
}

#[test]
fn every_layout_matches_its_committed_snapshot() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let update = std::env::var_os("UPDATE_SNAPSHOTS").is_some();
    let mut failures = Vec::new();
    for (name, input) in cases() {
        let mut got = String::new();
        dump(&lay_out(&input), LPoint { x: 0.0, y: 0.0 }, 0, &mut got);
        let path = dir.join(format!("{name}.txt"));
        if update {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, &got).unwrap();
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(want) if want == got => {}
            Ok(_) => failures.push(format!("{name}: the layout changed")),
            Err(_) => failures.push(format!("{name}: no snapshot at {}", path.display())),
        }
    }
    assert!(
        failures.is_empty(),
        "{}\nRun UPDATE_SNAPSHOTS=1 cargo test snapshot_tests and read the diff.",
        failures.join("\n")
    );
}

#[test]
fn the_same_graph_gives_the_same_layout_twice() {
    for (name, input) in cases() {
        let (mut first, mut second) = (String::new(), String::new());
        dump(&lay_out(&input), LPoint { x: 0.0, y: 0.0 }, 0, &mut first);
        dump(&lay_out(&input), LPoint { x: 0.0, y: 0.0 }, 0, &mut second);
        assert_eq!(first, second, "{name} laid out differently the second time");
    }
}
