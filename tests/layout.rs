//! `layout()` through the public API, on small hand-written graphs.
//!
//! Every expected number is what elkjs 0.9.3 returns for the same graph.

use std::collections::BTreeMap;

use elk_layered::{ElkEdge, ElkLabel, ElkNode, ElkPoint, ElkPort, Error, layout};

fn node(id: &str, width: f64, height: f64) -> ElkNode {
    ElkNode {
        id: id.into(),
        width,
        height,
        ..Default::default()
    }
}

fn group(id: &str, children: Vec<ElkNode>) -> ElkNode {
    ElkNode {
        id: id.into(),
        children,
        ..Default::default()
    }
}

fn edge(id: &str, source: &str, target: &str) -> ElkEdge {
    ElkEdge {
        id: id.into(),
        sources: vec![source.into()],
        targets: vec![target.into()],
        ..Default::default()
    }
}

fn label(text: &str, width: f64, height: f64) -> ElkLabel {
    ElkLabel {
        text: text.into(),
        width,
        height,
        ..Default::default()
    }
}

fn options(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

fn root(children: Vec<ElkNode>, edges: Vec<ElkEdge>) -> ElkNode {
    ElkNode {
        id: "root".into(),
        children,
        edges,
        ..Default::default()
    }
}

fn include_children(mut graph: ElkNode) -> ElkNode {
    graph
        .layout_options
        .insert("elk.hierarchyHandling".into(), "INCLUDE_CHILDREN".into());
    graph
}

fn find<'a>(graph: &'a ElkNode, id: &str) -> &'a ElkNode {
    if graph.id == id {
        return graph;
    }
    graph
        .children
        .iter()
        .find_map(|child| {
            let found = find(child, id);
            (found.id == id).then_some(found)
        })
        .unwrap_or(graph)
}

fn find_edge<'a>(graph: &'a ElkNode, id: &str) -> &'a ElkEdge {
    fn search<'a>(graph: &'a ElkNode, id: &str) -> Option<&'a ElkEdge> {
        graph
            .edges
            .iter()
            .find(|edge| edge.id == id)
            .or_else(|| graph.children.iter().find_map(|child| search(child, id)))
    }
    search(graph, id).unwrap_or_else(|| panic!("no edge {id}"))
}

fn at(graph: &ElkNode, id: &str) -> (f64, f64) {
    let node = find(graph, id);
    assert_eq!(node.id, id, "no node {id}");
    (node.x, node.y)
}

fn size(graph: &ElkNode, id: &str) -> (f64, f64) {
    let node = find(graph, id);
    assert_eq!(node.id, id, "no node {id}");
    (node.width, node.height)
}

fn route(edge: &ElkEdge) -> Vec<(f64, f64)> {
    let section = &edge.sections[0];
    std::iter::once(&section.start_point)
        .chain(&section.bend_points)
        .chain(std::iter::once(&section.end_point))
        .map(|p| (p.x, p.y))
        .collect()
}

// Mutation: let `layout_recursively` lay out a node without children. The root then gets the
// padding as its size, 24 x 24.
#[test]
fn an_empty_graph_comes_back_unchanged() {
    let graph = root(vec![], vec![]);
    assert_eq!(layout(&graph).unwrap(), graph);
}

// Mutation: leave the padding out of the node offset in `apply_graph_layout`.
#[test]
fn one_node_sits_inside_the_padding() {
    let laid = layout(&root(vec![node("a", 60.0, 30.0)], vec![])).unwrap();
    assert_eq!(at(&laid, "a"), (12.0, 12.0));
    assert_eq!(size(&laid, "root"), (84.0, 54.0));
    assert_eq!((laid.x, laid.y), (0.0, 0.0));
}

// Mutation: swap start and end point when `finish` writes a section.
#[test]
fn a_chain_runs_left_to_right() {
    let graph = root(
        vec![
            node("a", 60.0, 30.0),
            node("b", 70.0, 30.0),
            node("c", 80.0, 30.0),
        ],
        vec![edge("e0", "a", "b"), edge("e1", "b", "c")],
    );
    let laid = layout(&graph).unwrap();
    assert_eq!(at(&laid, "a"), (12.0, 12.0));
    assert_eq!(at(&laid, "b"), (92.0, 12.0));
    assert_eq!(at(&laid, "c"), (182.0, 12.0));
    let e0 = find_edge(&laid, "e0");
    assert_eq!(route(e0), vec![(72.0, 27.0), (92.0, 27.0)]);
    assert_eq!(e0.sections[0].id, "e0_s0");
    assert_eq!(e0.sections[0].incoming_shape.as_deref(), Some("a"));
    assert_eq!(e0.sections[0].outgoing_shape.as_deref(), Some("b"));
    assert_eq!(e0.container.as_deref(), Some("root"));
    assert_eq!(size(&laid, "root"), (274.0, 54.0));
}

// Mutation: let `best_containment` return the source's parent in the general case. The edge
// between two groups then lands in "h", not in the root.
#[test]
fn an_edge_between_groups_belongs_to_their_lowest_common_ancestor() {
    let mut g = group(
        "g",
        vec![
            group("h", vec![node("x", 60.0, 30.0)]),
            node("y", 60.0, 30.0),
        ],
    );
    g.edges = vec![edge("eyx", "y", "x")];
    let graph = include_children(root(
        vec![g, group("k", vec![node("z", 60.0, 30.0)])],
        vec![edge("exz", "x", "z")],
    ));
    let laid = layout(&graph).unwrap();
    // Depth two: x inside h inside g.
    assert_eq!(at(&laid, "g"), (12.0, 12.0));
    assert_eq!(at(&laid, "h"), (97.0, 12.0));
    assert_eq!(at(&laid, "x"), (12.0, 12.0));
    assert_eq!(size(&laid, "g"), (198.0, 78.0));
    assert_eq!(size(&laid, "h"), (84.0, 54.0));
    let eyx = find_edge(&laid, "eyx");
    assert_eq!(eyx.container.as_deref(), Some("g"));
    assert_eq!(route(eyx), vec![(72.0, 39.0), (109.0, 39.0)]);
    let exz = find_edge(&laid, "exz");
    assert_eq!(exz.container.as_deref(), Some("root"));
    assert_eq!(route(exz), vec![(181.0, 51.0), (252.0, 51.0)]);
    assert_eq!(size(&laid, "root"), (336.0, 102.0));
}

// Mutation: in `edge_layout`, take the absolute anchor for an edge into a descendant too.
#[test]
fn an_edge_into_a_descendant_starts_on_its_node() {
    let mut g = group("g", vec![node("x", 60.0, 30.0)]);
    g.edges = vec![edge("egx", "g", "x")];
    let graph = include_children(root(
        vec![node("p", 60.0, 30.0), g],
        vec![edge("epg", "p", "g")],
    ));
    let laid = layout(&graph).unwrap();
    let egx = find_edge(&laid, "egx");
    assert_eq!(egx.container.as_deref(), Some("g"));
    assert_eq!(route(egx), vec![(0.0, 27.0), (12.0, 27.0)]);
    assert_eq!(
        route(find_edge(&laid, "epg")),
        vec![(72.0, 49.0), (92.0, 49.0)]
    );
    assert_eq!(at(&laid, "g"), (92.0, 12.0));
}

// Mutation: skip `apply_parent_node_layout` for a node laid out on its own. The group then keeps
// size zero in its parent's layout.
#[test]
fn separate_children_are_laid_out_before_their_parent() {
    let mut g = group("g", vec![node("x", 60.0, 30.0), node("y", 60.0, 30.0)]);
    g.edges = vec![edge("exy", "x", "y")];
    g.labels = vec![label("G", 30.0, 10.0)];
    g.layout_options = options(&[("elk.nodeLabels.placement", "[H_LEFT, V_TOP, INSIDE]")]);
    let graph = root(
        vec![node("p", 60.0, 30.0), g],
        vec![edge("epg", "p", "g"), edge("epx", "p", "x")],
    );
    let laid = layout(&graph).unwrap();
    assert_eq!(size(&laid, "g"), (199.0, 69.0));
    assert_eq!(at(&laid, "g"), (92.0, 12.0));
    assert_eq!(at(&laid, "x"), (47.0, 27.0));
    let g = find(&laid, "g");
    assert_eq!((g.labels[0].x, g.labels[0].y), (5.0, 5.0));
    assert_eq!(
        route(find_edge(&laid, "exy")),
        vec![(107.0, 42.0), (127.0, 42.0)]
    );
    // An edge into another level of a separate layout stays unrouted, as in elkjs.
    let epx = find_edge(&laid, "epx");
    assert!(epx.sections.is_empty());
    assert_eq!(epx.container.as_deref(), Some("root"));
    assert_eq!(size(&laid, "root"), (303.0, 93.0));
}

// Mutation: write edge label positions without the edge offset.
#[test]
fn edge_labels_go_to_the_center_head_and_tail() {
    let mut e = edge("e", "a", "b");
    let mut head = label("h", 10.0, 10.0);
    head.layout_options = options(&[("elk.edgeLabels.placement", "HEAD")]);
    let mut tail = label("t", 10.0, 10.0);
    tail.layout_options = options(&[("elk.edgeLabels.placement", "TAIL")]);
    e.labels = vec![label("c", 20.0, 10.0), head, tail];
    let laid = layout(&root(
        vec![node("a", 60.0, 30.0), node("b", 60.0, 30.0)],
        vec![e],
    ))
    .unwrap();
    let e = find_edge(&laid, "e");
    let positions: Vec<(f64, f64)> = e.labels.iter().map(|l| (l.x, l.y)).collect();
    assert_eq!(positions, vec![(104.0, 30.0), (144.0, 30.0), (74.0, 30.0)]);
    assert_eq!(at(&laid, "b"), (156.0, 12.0));
}

// Mutation: swap `HAlign::Left` and `HAlign::Right` for the inside locations in
// `location_cell`.
#[test]
fn node_labels_go_where_their_placement_says() {
    let mut children = Vec::new();
    for inside in ["INSIDE", "OUTSIDE"] {
        for vertical in ["V_TOP", "V_CENTER", "V_BOTTOM"] {
            for horizontal in ["H_LEFT", "H_CENTER", "H_RIGHT"] {
                let mut n = node(&format!("{inside}-{vertical}-{horizontal}"), 80.0, 40.0);
                n.labels = vec![label("L", 20.0, 10.0)];
                n.layout_options = options(&[(
                    "elk.nodeLabels.placement",
                    &format!("[{horizontal}, {vertical}, {inside}]"),
                )]);
                children.push(n);
            }
        }
    }
    let mut priority = node("prio", 80.0, 40.0);
    priority.labels = vec![label("L", 20.0, 10.0)];
    priority.layout_options = options(&[(
        "elk.nodeLabels.placement",
        "[H_LEFT, V_TOP, OUTSIDE, H_PRIORITY]",
    )]);
    children.push(priority);
    let mut fixed = node("fixed", 80.0, 40.0);
    fixed.labels = vec![ElkLabel {
        x: 3.0,
        y: 4.0,
        ..label("L", 20.0, 10.0)
    }];
    children.push(fixed);
    let laid = layout(&root(children, vec![])).unwrap();
    let expected = [
        ("INSIDE-V_TOP-H_LEFT", (5.0, 5.0)),
        ("INSIDE-V_TOP-H_CENTER", (30.0, 5.0)),
        ("INSIDE-V_TOP-H_RIGHT", (55.0, 5.0)),
        ("INSIDE-V_CENTER-H_LEFT", (5.0, 15.0)),
        ("INSIDE-V_CENTER-H_CENTER", (30.0, 15.0)),
        ("INSIDE-V_CENTER-H_RIGHT", (55.0, 15.0)),
        ("INSIDE-V_BOTTOM-H_LEFT", (5.0, 25.0)),
        ("INSIDE-V_BOTTOM-H_CENTER", (30.0, 25.0)),
        ("INSIDE-V_BOTTOM-H_RIGHT", (55.0, 25.0)),
        ("OUTSIDE-V_TOP-H_LEFT", (0.0, -15.0)),
        ("OUTSIDE-V_TOP-H_CENTER", (30.0, -15.0)),
        ("OUTSIDE-V_TOP-H_RIGHT", (60.0, -15.0)),
        ("OUTSIDE-V_CENTER-H_LEFT", (-25.0, 15.0)),
        // No location has outside, center and center: ELK leaves the label where it was.
        ("OUTSIDE-V_CENTER-H_CENTER", (0.0, 0.0)),
        ("OUTSIDE-V_CENTER-H_RIGHT", (85.0, 15.0)),
        ("OUTSIDE-V_BOTTOM-H_LEFT", (0.0, 45.0)),
        ("OUTSIDE-V_BOTTOM-H_CENTER", (30.0, 45.0)),
        ("OUTSIDE-V_BOTTOM-H_RIGHT", (60.0, 45.0)),
        ("prio", (-25.0, 0.0)),
        ("fixed", (3.0, 4.0)),
    ];
    for (id, position) in expected {
        let label = &find(&laid, id).labels[0];
        assert_eq!((label.x, label.y), position, "{id}");
    }
}

// Mutation: drop the `NODE_LABELS` branch of `configure_size_contributions`.
#[test]
fn size_constraints_size_a_leaf_node() {
    let mut a = node("a", 10.0, 10.0);
    a.labels = vec![label("L", 50.0, 12.0), label("M", 30.0, 8.0)];
    a.layout_options = options(&[
        ("elk.nodeLabels.placement", "[H_CENTER, V_TOP, INSIDE]"),
        ("elk.nodeSize.constraints", "[NODE_LABELS]"),
    ]);
    let mut b = node("b", 10.0, 10.0);
    b.layout_options = options(&[
        ("elk.nodeSize.constraints", "[MINIMUM_SIZE]"),
        ("elk.nodeSize.minimum", "(70, 0)"),
    ]);
    let mut c = node("c", 10.0, 10.0);
    c.layout_options = options(&[
        ("elk.nodeSize.constraints", "MINIMUM_SIZE"),
        ("elk.nodeSize.minimum", "55;33"),
    ]);
    let laid = layout(&root(vec![a, b, c], vec![])).unwrap();
    assert_eq!(size(&laid, "a"), (60.0, 50.0));
    let a = find(&laid, "a");
    assert_eq!((a.labels[0].x, a.labels[0].y), (5.0, 5.0));
    assert_eq!((a.labels[1].x, a.labels[1].y), (15.0, 17.0));
    // The default minimum size option turns a zero height into 20.
    assert_eq!(size(&laid, "b"), (70.0, 20.0));
    assert_eq!(size(&laid, "c"), (55.0, 33.0));
}

// Mutation: let `NodeSizing::effective_minimum` return `None`. The group then shrinks to its
// content, 84 x 54.
#[test]
fn a_group_with_a_minimum_size_keeps_it() {
    let mut g = group("g", vec![node("x", 60.0, 30.0)]);
    g.layout_options = options(&[
        ("elk.nodeSize.constraints", "[MINIMUM_SIZE]"),
        ("elk.nodeSize.minimum", "(300, 150)"),
        ("elk.contentAlignment", "[H_CENTER, V_CENTER]"),
    ]);
    let graph = include_children(root(
        vec![node("p", 60.0, 30.0), g],
        vec![edge("e", "p", "x")],
    ));
    let laid = layout(&graph).unwrap();
    assert_eq!(size(&laid, "g"), (300.0, 150.0));
    assert_eq!(at(&laid, "x"), (120.0, 60.0));
    assert_eq!(
        route(find_edge(&laid, "e")),
        vec![(72.0, 87.0), (217.0, 87.0)]
    );
}

// Mutation: let `include_minimum_graph_size` ignore the computed minimum. The group then shrinks
// to its content and the title sticks out.
#[test]
fn a_group_keeps_the_width_its_title_asks_for() {
    let mut g = group("g", vec![node("x", 40.0, 24.0)]);
    g.labels = vec![label("Brake System Controller", 160.0, 16.0)];
    g.layout_options = options(&[
        ("elk.nodeLabels.placement", "[H_LEFT, V_TOP, INSIDE]"),
        ("elk.nodeSize.constraints", "[NODE_LABELS]"),
    ]);
    let graph = include_children(root(
        vec![node("p", 60.0, 30.0), g],
        vec![edge("e0", "p", "x")],
    ));
    let laid = layout(&graph).unwrap();
    assert_eq!(size(&laid, "g"), (330.0, 69.0));
    assert_eq!(at(&laid, "x"), (177.0, 33.0));
    let g = find(&laid, "g");
    assert_eq!((g.labels[0].x, g.labels[0].y), (5.0, 5.0));
}

// Mutation: let `node_settings` ignore `elk.direction`.
#[test]
fn options_are_read_with_every_prefix_form_and_value_syntax() {
    let mut graph = root(
        vec![
            node("a", 60.0, 30.0),
            node("b", 60.0, 30.0),
            node("c", 60.0, 30.0),
        ],
        vec![edge("e", "a", "b")],
    );
    graph.layout_options = options(&[
        // The full id, a suffix after a dot, and the `elk.` short form.
        ("org.eclipse.elk.direction", "DOWN"),
        ("spacing.nodeNode", "50"),
        ("elk.padding", "[top=1,left=2,bottom=3,right=4]"),
        ("elk.layered.spacing.nodeNodeBetweenLayers", "40"),
        // Booleans ignore case.
        ("elk.separateConnectedComponents", "FALSE"),
    ]);
    let laid = layout(&graph).unwrap();
    assert_eq!(at(&laid, "a"), (112.0, 1.0));
    assert_eq!(at(&laid, "b"), (112.0, 71.0));
    assert_eq!(at(&laid, "c"), (2.0, 1.0));
    assert_eq!(size(&laid, "root"), (176.0, 104.0));
}

// Mutation: drop the ordinal fallback in `parse_enum`.
#[test]
fn an_enum_may_be_given_by_its_ordinal() {
    let mut graph = root(
        vec![node("a", 60.0, 30.0), node("b", 60.0, 30.0)],
        vec![edge("e", "a", "b")],
    );
    graph.layout_options = options(&[("elk.direction", "3")]);
    let laid = layout(&graph).unwrap();
    assert_eq!(at(&laid, "b"), (12.0, 62.0));
}

// Mutation: let `with_base_value` scale explicit spacings too. The node-node spacing of 5 then
// grows to 40 and moves c down.
#[test]
fn the_spacing_base_value_leaves_explicit_spacings_alone() {
    let mut graph = root(
        vec![
            node("a", 60.0, 30.0),
            node("b", 60.0, 30.0),
            node("c", 60.0, 30.0),
        ],
        vec![edge("e", "a", "b"), edge("f", "a", "c")],
    );
    graph.layout_options = options(&[
        ("elk.layered.spacing.baseValue", "40"),
        ("elk.spacing.nodeNode", "5"),
    ]);
    let laid = layout(&graph).unwrap();
    assert_eq!(at(&laid, "a"), (12.0, 17.0));
    // The base value doubles the spacing between layers: b starts at 72 + 40.
    assert_eq!(at(&laid, "b"), (112.0, 12.0));
    assert_eq!(at(&laid, "c"), (112.0, 47.0));
}

// Mutation: make `check_number` refuse zero sizes.
#[test]
fn zero_sizes_are_allowed() {
    let laid = layout(&root(
        vec![node("a", 0.0, 0.0), node("b", 0.0, 0.0)],
        vec![edge("e", "a", "b")],
    ))
    .unwrap();
    assert_eq!(at(&laid, "b"), (32.0, 12.0));
    assert_eq!(size(&laid, "root"), (44.0, 24.0));
}

// Mutation: let `finish` always name the section `<id>_s0`, or clear the junction points of an
// edge that gets none. elkjs 0.9.3 leaves the ones the JSON came with: the layout sets the ELK
// property to null, and the JSON writer skips a null.
#[test]
fn an_edge_with_a_section_keeps_its_id_and_its_junction_points() {
    let mut e = edge("e", "a", "b");
    e.sections = vec![
        elk_layered::ElkEdgeSection {
            id: "mine".into(),
            ..Default::default()
        },
        elk_layered::ElkEdgeSection {
            id: "other".into(),
            ..Default::default()
        },
    ];
    e.junction_points = vec![ElkPoint { x: 1.0, y: 1.0 }];
    let laid = layout(&root(
        vec![node("a", 60.0, 30.0), node("b", 60.0, 30.0)],
        vec![e],
    ))
    .unwrap();
    let e = find_edge(&laid, "e");
    assert_eq!(e.sections.len(), 1);
    assert_eq!(e.sections[0].id, "mine");
    assert_eq!(e.junction_points, [ElkPoint { x: 1.0, y: 1.0 }]);
}

// --- Ports on nodes without children. ---

fn port(id: &str, width: f64, height: f64, side: &str) -> ElkPort {
    let mut port = ElkPort {
        id: id.into(),
        width,
        height,
        ..Default::default()
    };
    if !side.is_empty() {
        port.layout_options
            .insert("elk.port.side".into(), side.into());
    }
    port
}

fn with_ports(mut node: ElkNode, constraints: &str, ports: Vec<ElkPort>) -> ElkNode {
    if !constraints.is_empty() {
        node.layout_options
            .insert("elk.portConstraints".into(), constraints.into());
    }
    node.ports = ports;
    node
}

fn port_at(graph: &ElkNode, node: &str, port: &str) -> (f64, f64) {
    let port = find(graph, node)
        .ports
        .iter()
        .find(|candidate| candidate.id == port)
        .unwrap_or_else(|| panic!("no port {port}"));
    (port.x, port.y)
}

// Mutation: drop the port writes in `write_result`. Every port then stays at the given (0, 0).
#[test]
fn ports_come_back_with_their_positions_relative_to_their_node() {
    // elkjs 0.9.3 returns these numbers. `p2` is given below `p`, and FIXED_ORDER sorts by it.
    let mut p = port("p", 6.0, 4.0, "EAST");
    p.y = 30.0;
    let mut p2 = port("p2", 6.0, 4.0, "EAST");
    p2.y = 5.0;
    let graph = root(
        vec![
            with_ports(
                node("a", 40.0, 50.0),
                "FIXED_ORDER",
                vec![p, p2, port("p3", 6.0, 4.0, "WEST")],
            ),
            with_ports(
                node("b", 40.0, 30.0),
                "FIXED_ORDER",
                vec![port("q", 6.0, 4.0, "WEST"), port("q2", 6.0, 4.0, "WEST")],
            ),
        ],
        vec![
            edge("e", "p", "q"),
            edge("e2", "p2", "q2"),
            edge("e3", "b", "p3"),
        ],
    );
    let laid = layout(&graph).unwrap();
    assert_eq!(at(&laid, "a"), (28.0, 12.0));
    assert_eq!(port_at(&laid, "a", "p"), (40.0, 32.0));
    assert_eq!(port_at(&laid, "a", "p2"), (40.0, 14.0));
    assert_eq!(port_at(&laid, "a", "p3"), (-6.0, 23.0));
    assert_eq!(at(&laid, "b"), (100.0, 18.666_666_666_666_664));
    assert_eq!(port_at(&laid, "b", "q"), (-6.0, 18.666_666_666_666_668));
    assert_eq!(port_at(&laid, "b", "q2"), (-6.0, 7.333_333_333_333_334));
    // The edge starts at the anchor of `p2`, the east side of its box, and ends at `q2`.
    let e2 = route(find_edge(&laid, "e2"));
    assert_eq!((e2[0], e2[e2.len() - 1]), ((74.0, 28.0), (94.0, 28.0)));
    // Sizes stay as given.
    assert_eq!(size(&laid, "a"), (40.0, 50.0));
    assert_eq!(laid.children[0].ports[0].width, 6.0);
}

// Mutation: read the border offset of a port only when the node is not fixed.
#[test]
fn a_border_offset_moves_a_port_off_the_border() {
    let mut p = port("p", 6.0, 4.0, "EAST");
    p.x = 40.0;
    p.y = 5.0;
    p.layout_options
        .insert("elk.port.borderOffset".into(), "-3".into());
    let mut p3 = port("p3", 6.0, 4.0, "WEST");
    p3.layout_options
        .insert("elk.port.borderOffset".into(), "2".into());
    let graph = root(
        vec![
            with_ports(node("a", 40.0, 30.0), "FIXED_SIDE", vec![p, p3]),
            with_ports(
                node("b", 40.0, 30.0),
                "FIXED_SIDE",
                vec![port("q", 6.0, 4.0, "WEST")],
            ),
        ],
        vec![edge("e", "p", "q"), edge("e3", "b", "p3")],
    );
    let laid = layout(&graph).unwrap();
    assert_eq!(at(&laid, "a"), (96.0, 12.0));
    assert_eq!(port_at(&laid, "a", "p"), (37.0, 13.0));
    assert_eq!(port_at(&laid, "a", "p3"), (-8.0, 13.0));
}

// Mutation: ignore the position of a port that has no side, so it gets the default side.
#[test]
fn a_fixed_side_port_without_a_side_takes_it_from_where_it_sits() {
    let at_xy = |id: &str, x: f64, y: f64| {
        let mut p = port(id, 6.0, 4.0, "");
        p.x = x;
        p.y = y;
        p
    };
    let graph = root(
        vec![
            with_ports(
                node("a", 40.0, 50.0),
                "FIXED_SIDE",
                vec![at_xy("p", 40.0, 10.0), at_xy("p2", -6.0, 35.0)],
            ),
            with_ports(
                node("b", 40.0, 30.0),
                "FIXED_SIDE",
                vec![at_xy("q", -6.0, 5.0)],
            ),
        ],
        vec![edge("e", "p", "q"), edge("e2", "b", "p2")],
    );
    let laid = layout(&graph).unwrap();
    assert_eq!(at(&laid, "a"), (94.0, 12.0));
    assert_eq!(port_at(&laid, "a", "p"), (40.0, 23.0));
    assert_eq!(port_at(&laid, "a", "p2"), (-6.0, 23.0));
    assert_eq!(at(&laid, "b"), (28.0, 22.0));
}

// Mutation: refuse a port across the flow in a graph whose ports are free.
#[test]
fn a_port_on_a_north_side_is_laid_out_while_the_ports_are_free() {
    let graph = root(
        vec![
            with_ports(
                node("a", 40.0, 30.0),
                "",
                vec![port("p", 6.0, 4.0, "NORTH"), port("p2", 6.0, 4.0, "SOUTH")],
            ),
            with_ports(node("b", 40.0, 30.0), "", vec![port("q", 6.0, 4.0, "WEST")]),
        ],
        vec![edge("e", "p", "q"), edge("e2", "b", "p2")],
    );
    let laid = layout(&graph).unwrap();
    // elkjs 0.9.3 moves the free ports to the sides along the flow.
    assert_eq!(at(&laid, "a"), (84.0, 12.0));
    assert_eq!(port_at(&laid, "a", "p"), (-6.0, 18.666_666_666_666_668));
    assert_eq!(port_at(&laid, "a", "p2"), (-6.0, 7.333_333_333_333_334));
    assert_eq!(at(&laid, "b"), (12.0, 12.666_666_666_666_668));
    assert_eq!(port_at(&laid, "b", "q"), (40.0, 17.333_333_333_333_332));
}

// Mutation: drop the `NorthSouthPort` mapping in `run_layered`, which then reports an internal
// error, or let a node with children through.
#[test]
fn a_port_across_the_flow_of_a_node_with_children_is_unsupported() {
    // Right: north and south are across. Down: east and west are across.
    for (direction, side) in [
        ("RIGHT", "NORTH"),
        ("LEFT", "SOUTH"),
        ("DOWN", "EAST"),
        ("UP", "WEST"),
    ] {
        let mut graph = include_children(root(
            vec![with_ports(
                group("a", vec![node("c", 40.0, 30.0)]),
                "FIXED_SIDE",
                vec![port("p", 6.0, 4.0, side)],
            )],
            vec![],
        ));
        graph
            .layout_options
            .insert("elk.direction".into(), direction.into());
        assert!(
            matches!(
                error_of(&graph),
                Error::Unsupported { element, feature }
                    if element == "port \"p\""
                        && feature == "north and south ports on a node with children"
            ),
            "{direction} {side}"
        );
    }
}

// Mutation: in `check_north_south_ports`, call the north and south sides across the flow in a
// vertical layout too. The refusal then names `n`.
#[test]
fn the_refusal_names_the_port_across_the_flow() {
    let mut graph = include_children(root(
        vec![with_ports(
            group("a", vec![node("c", 40.0, 30.0)]),
            "FIXED_SIDE",
            vec![port("n", 6.0, 4.0, "NORTH"), port("e", 6.0, 4.0, "EAST")],
        )],
        vec![],
    ));
    graph
        .layout_options
        .insert("elk.direction".into(), "DOWN".into());
    assert!(matches!(
        error_of(&graph),
        Error::Unsupported { element, .. } if element == "port \"e\""
    ));
}

// Mutation: leave the southern port at the top of the node, or put the ports of a vertical
// layout on the sides of the horizontal one.
#[test]
fn a_port_across_the_flow_is_laid_out_in_every_direction() {
    // (direction, side, where the port of a node of 40 by 30 ends up)
    for (direction, side, expected) in [
        ("RIGHT", "NORTH", (17.0, -4.0)),
        ("LEFT", "SOUTH", (17.0, 30.0)),
        ("DOWN", "EAST", (40.0, 13.0)),
        ("UP", "WEST", (-6.0, 13.0)),
    ] {
        let mut graph = root(
            vec![with_ports(
                node("a", 40.0, 30.0),
                "FIXED_SIDE",
                vec![port("p", 6.0, 4.0, side)],
            )],
            vec![],
        );
        graph
            .layout_options
            .insert("elk.direction".into(), direction.into());
        let laid = layout(&graph).unwrap();
        assert_eq!(port_at(&laid, "a", "p"), expected, "{direction} {side}");
    }
}

// Mutation: treat the sides along the flow as across it for the vertical directions.
#[test]
fn a_port_along_the_flow_is_laid_out_in_every_direction() {
    for (direction, side) in [
        ("RIGHT", "EAST"),
        ("LEFT", "WEST"),
        ("DOWN", "NORTH"),
        ("UP", "SOUTH"),
    ] {
        let mut graph = root(
            vec![with_ports(
                node("a", 40.0, 30.0),
                "FIXED_SIDE",
                vec![port("p", 6.0, 4.0, side)],
            )],
            vec![],
        );
        graph
            .layout_options
            .insert("elk.direction".into(), direction.into());
        assert!(layout(&graph).is_ok(), "{direction} {side}");
    }
}

// Mutation: drop the check for the root in `run_layered`. ELK fails on this graph, and elk-layered
// would give it a layout.
#[test]
fn an_edge_between_a_root_port_and_a_child_is_unsupported() {
    for edges in [vec![edge("e", "p", "a")], vec![edge("e", "a", "p")]] {
        let mut graph = root(vec![node("a", 40.0, 30.0)], edges);
        graph.ports = vec![port("p", 6.0, 4.0, "WEST")];
        graph
            .layout_options
            .insert("elk.portConstraints".into(), "FIXED_SIDE".into());
        assert!(matches!(
            error_of(&graph),
            Error::Unsupported { element, feature }
                if element == "port \"p\""
                    && feature == "an edge between a port of the root graph and a node inside it"
        ));
    }
}

// Mutation: count a grandchild as a child in `Tree::count_port_facts`. The edge at `p` then makes
// the port an external port of the root.
#[test]
fn a_root_port_with_an_edge_to_a_grandchild_only_is_laid_out() {
    let mut graph = root(
        vec![group("g", vec![node("a", 40.0, 30.0)])],
        vec![edge("e", "p", "a")],
    );
    graph.ports = vec![port("p", 6.0, 4.0, "WEST")];
    graph
        .layout_options
        .insert("elk.portConstraints".into(), "FIXED_SIDE".into());
    // The edge is not one of the root's children, so ELK leaves it out of this run.
    assert!(layout(&graph).is_ok());
}

// Mutation: let the root count as a graph with external ports whenever it has a port, in
// `Tree::count_port_facts`.
#[test]
fn a_root_port_without_edges_is_laid_out_and_placed_by_its_side() {
    let mut graph = root(vec![node("a", 40.0, 30.0)], vec![]);
    graph.ports = vec![port("w", 10.0, 10.0, "WEST"), port("e", 10.0, 10.0, "EAST")];
    graph
        .layout_options
        .insert("elk.portConstraints".into(), "FIXED_SIDE".into());
    let laid = layout(&graph).unwrap();
    // The root is 64 wide: 12 padding, 40 for the node, 12 padding. ELK moves the east port with
    // the right border, and leaves the west port where it is.
    assert_eq!(laid.width, 64.0);
    let (west_x, _) = port_at(&laid, "root", "w");
    let (east_x, _) = port_at(&laid, "root", "e");
    assert_eq!(west_x, 0.0);
    assert_eq!(east_x, 64.0);
}

// Mutation: in `transform_port` of src/importer.rs, keep a port label without text, which ELK
// drops.
#[test]
fn a_port_label_with_text_is_placed_but_one_without_is_dropped() {
    // elkjs 0.9.3 returns these numbers: the label goes outside, below the east port, 1 away
    // from it each way, and the label without text stays where it was given.
    let mut p = port("p", 6.0, 4.0, "EAST");
    p.labels = vec![label("", 10.0, 10.0), label("P", 10.0, 10.0)];
    let graph = root(
        vec![with_ports(node("a", 40.0, 30.0), "FIXED_SIDE", vec![p])],
        vec![],
    );
    let laid = layout(&graph).unwrap();
    let labels = &find(&laid, "a").ports[0].labels;
    assert_eq!((labels[0].x, labels[0].y), (0.0, 0.0));
    assert_eq!((labels[1].x, labels[1].y), (7.0, 5.0));
}

// Mutation: drop the check for labels on the ports of a node with children in `Tree::new`.
#[test]
fn a_port_label_with_text_on_a_group_is_unsupported_but_one_without_is_dropped() {
    let mut p = port("p", 6.0, 4.0, "EAST");
    p.labels = vec![label("", 10.0, 10.0)];
    let mut g = group("g", vec![node("c", 20.0, 20.0)]);
    g.ports = vec![p.clone()];
    assert!(layout(&root(vec![g.clone()], vec![])).is_ok());
    p.labels = vec![label("P", 10.0, 10.0)];
    g.ports = vec![p];
    assert!(matches!(
        error_of(&root(vec![g], vec![])),
        Error::Unsupported { element, feature }
            if element == "port \"p\"" && feature == "a label on a port of a node with children"
    ));
}

// Mutation: drop the check for the port size constraints of a node with children in
// `Tree::new`.
#[test]
fn a_group_that_sizes_itself_to_its_ports_is_unsupported() {
    for constraints in ["[PORTS]", "[PORT_LABELS]"] {
        let mut g = group("g", vec![node("c", 20.0, 20.0)]);
        g.layout_options = options(&[("elk.nodeSize.constraints", constraints)]);
        assert!(
            matches!(
                error_of(&root(vec![g], vec![])),
                Error::Unsupported { element, feature }
                    if element == "node \"g\""
                        && feature == "sizing a node with children to its ports"
            ),
            "{constraints}"
        );
    }
}

// Mutation: in `hierarchy_edge_end` of src/importer.rs, ignore the named port and make a port of
// the node's own, as for an edge that names no port.
#[test]
fn an_edge_at_a_port_across_a_level_of_an_included_graph_ends_at_the_port() {
    let inner = group(
        "g",
        vec![with_ports(
            node("x", 40.0, 30.0),
            "FIXED_SIDE",
            vec![port("p", 6.0, 4.0, "EAST")],
        )],
    );
    let graph = include_children(root(
        vec![
            inner,
            with_ports(
                node("y", 40.0, 30.0),
                "FIXED_SIDE",
                vec![port("q", 6.0, 4.0, "WEST")],
            ),
        ],
        vec![edge("e", "p", "q")],
    ));
    let laid = layout(&graph).unwrap();
    let e = find_edge(&laid, "e");
    assert_eq!(e.sections[0].incoming_shape.as_deref(), Some("p"));
    assert_eq!(e.sections[0].outgoing_shape.as_deref(), Some("q"));
    // The route starts at the anchor of `p`: the middle of its right border. `p` is 6 by 4.
    let (px, py) = port_at(&laid, "x", "p");
    let (x, y) = at(&laid, "x");
    let (gx, gy) = at(&laid, "g");
    let start = e.sections[0].start_point;
    assert_eq!((start.x, start.y), (gx + x + px + 6.0, gy + y + py + 2.0));
    assert_eq!((start.x, start.y), (70.0, 39.0));
}

// Mutation: leave the port ids out of the id check in `Tree::new`, or check them against the
// node ids too.
#[test]
fn a_port_id_may_repeat_a_node_id_but_not_a_port_id() {
    let two = |first: &str, second: &str, node_id: &str| {
        root(
            vec![
                with_ports(node("a", 40.0, 30.0), "", vec![port(first, 6.0, 4.0, "")]),
                with_ports(
                    node(node_id, 40.0, 30.0),
                    "",
                    vec![port(second, 6.0, 4.0, "")],
                ),
            ],
            vec![],
        )
    };
    assert!(matches!(
        error_of(&two("p", "p", "b")),
        Error::DuplicateId { id } if id == "p"
    ));
    // elkjs keeps `nodeIdMap` and `portIdMap` apart: these lay out.
    assert!(layout(&two("a", "q", "b")).is_ok());
    assert!(layout(&two("b", "q", "b")).is_ok());
    // A port may share an id with an edge, too.
    let mut graph = two("p", "q", "b");
    graph.edges = vec![edge("p", "a", "b")];
    assert!(layout(&graph).is_ok());
}

// Mutation: look up an edge end among the ports before the nodes. The edge then ends at the
// port `b` of `a`, not at the node `b`.
#[test]
fn an_edge_end_that_names_a_node_and_a_port_means_the_node() {
    let graph = root(
        vec![
            with_ports(
                node("a", 40.0, 30.0),
                "FIXED_SIDE",
                vec![port("b", 6.0, 4.0, "WEST")],
            ),
            node("b", 40.0, 30.0),
        ],
        vec![edge("e", "a", "b")],
    );
    let laid = layout(&graph).unwrap();
    // `a` is left of `b`, as for any edge from `a` to the node `b`.
    assert!(at(&laid, "a").0 < at(&laid, "b").0);
    assert_eq!(
        route(find_edge(&laid, "e")).last().unwrap().0,
        at(&laid, "b").0
    );
}

// Mutation: look up an edge end among the nodes only.
#[test]
fn an_edge_end_may_name_a_port_and_an_unknown_id_is_still_unknown() {
    let graph = root(
        vec![
            with_ports(
                node("a", 40.0, 30.0),
                "FIXED_SIDE",
                vec![port("p", 6.0, 4.0, "EAST")],
            ),
            node("b", 40.0, 30.0),
        ],
        vec![edge("e", "p", "b")],
    );
    let laid = layout(&graph).unwrap();
    // elkjs 0.9.3: the route starts at the anchor of `p`, the east middle of its 6 by 4 box.
    assert_eq!(port_at(&laid, "a", "p"), (40.0, 13.0));
    assert_eq!(route(find_edge(&laid, "e")), [(58.0, 27.0), (78.0, 27.0)]);
    assert_eq!(
        find_edge(&laid, "e").sections[0].incoming_shape.as_deref(),
        Some("p")
    );
    let mut graph = graph;
    graph.edges[0].targets = vec!["nowhere".into()];
    assert!(matches!(
        error_of(&graph),
        Error::UnknownId { edge, id } if edge == "e" && id == "nowhere"
    ));
}

// Mutation: skip the options of a port in `Tree::new`.
#[test]
fn the_options_of_a_port_are_checked() {
    let with_option = |key: &str, value: &str| {
        let mut p = port("p", 6.0, 4.0, "");
        p.layout_options.insert(key.into(), value.into());
        root(vec![with_ports(node("a", 40.0, 30.0), "", vec![p])], vec![])
    };
    assert!(matches!(
        error_of(&with_option("elk.port.side", "UPWARDS")),
        Error::InvalidOptionValue { element, .. } if element == "port \"p\""
    ));
    assert!(matches!(
        error_of(&with_option("elk.port.anchor", "(1, 2)")),
        Error::UnsupportedOption { element, key, .. }
            if element == "port \"p\"" && key == "elk.port.anchor"
    ));
    assert!(matches!(
        error_of(&with_option("elk.nothing", "1")),
        Error::UnknownOption { .. }
    ));
    // A node option does not apply to a port.
    assert!(matches!(
        error_of(&with_option("elk.portConstraints", "FREE")),
        Error::UnsupportedOption { .. }
    ));
    assert!(layout(&with_option("elk.port.index", "2")).is_ok());
    assert!(layout(&with_option("elk.port.borderOffset", "1.5")).is_ok());
}

// Mutation: skip the number checks for ports.
#[test]
fn the_numbers_of_a_port_are_checked() {
    for (field, graph) in [
        ("x", f64::NAN),
        ("y", f64::INFINITY),
        ("width", -1.0),
        ("height", f64::NAN),
    ]
    .map(|(field, value)| {
        let mut p = port("p", 6.0, 4.0, "");
        match field {
            "x" => p.x = value,
            "y" => p.y = value,
            "width" => p.width = value,
            _ => p.height = value,
        }
        (
            field,
            root(vec![with_ports(node("a", 40.0, 30.0), "", vec![p])], vec![]),
        )
    }) {
        assert!(matches!(
            error_of(&graph),
            Error::InvalidSize { element, field: f, .. } if element == "port \"p\"" && f == field
        ));
    }
}

// --- Junction points. ---

fn junctions(graph: &ElkNode, edge: &str) -> Vec<(f64, f64)> {
    find_edge(graph, edge)
        .junction_points
        .iter()
        .map(|p| (p.x, p.y))
        .collect()
}

/// `a` leaves through one port `p` to `b`, `c` (ports) and `d` (no port).
fn shared_port_graph(direction: &str, out_side: &str, in_side: &str) -> ElkNode {
    let mut graph = root(
        vec![
            with_ports(
                node("a", 40.0, 30.0),
                "FIXED_SIDE",
                vec![port("p", 6.0, 4.0, out_side)],
            ),
            with_ports(
                node("b", 40.0, 30.0),
                "FIXED_SIDE",
                vec![port("q", 6.0, 4.0, in_side)],
            ),
            with_ports(
                node("c", 40.0, 30.0),
                "FIXED_SIDE",
                vec![port("r", 6.0, 4.0, in_side)],
            ),
            node("d", 50.0, 20.0),
        ],
        vec![
            edge("e", "p", "q"),
            edge("e2", "p", "r"),
            edge("e3", "p", "d"),
        ],
    );
    graph
        .layout_options
        .insert("elk.direction".into(), direction.into());
    graph
}

// Mutation: skip `add_junction_point_if_necessary` in `calculate_bend_points`. Layered routes every
// direction left to right, so the vertical directions are covered by the mirror and transpose.
#[test]
fn a_port_with_several_edges_gets_junction_points_in_every_direction() {
    // elkjs 0.9.3 returns these. Only the edge that bends where the others join has them.
    for (direction, out_side, in_side, want) in [
        ("RIGHT", "EAST", "WEST", [(68.0, 117.0), (68.0, 67.0)]),
        ("LEFT", "WEST", "EAST", [(72.0, 117.0), (72.0, 67.0)]),
        ("DOWN", "SOUTH", "NORTH", [(37.0, 56.0), (102.0, 56.0)]),
        ("UP", "NORTH", "SOUTH", [(37.0, 56.0), (102.0, 56.0)]),
    ] {
        let laid = layout(&shared_port_graph(direction, out_side, in_side)).unwrap();
        assert_eq!(junctions(&laid, "e"), want, "{direction}");
        assert!(junctions(&laid, "e2").is_empty(), "{direction}");
        assert!(junctions(&laid, "e3").is_empty(), "{direction}");
    }
}

// Mutation: leave the junction points out of `offset_graph` in components.rs.
#[test]
fn junction_points_move_with_their_component() {
    let component = |a: &str, p: &str, q: &str, r: &str, d: &str| {
        let ids = |s: &str| format!("{a}{s}");
        (
            vec![
                with_ports(
                    node(a, 40.0, 30.0),
                    "FIXED_SIDE",
                    vec![port(p, 6.0, 4.0, "EAST")],
                ),
                with_ports(
                    node(&ids("b"), 40.0, 30.0),
                    "FIXED_SIDE",
                    vec![port(q, 6.0, 4.0, "WEST")],
                ),
                with_ports(
                    node(&ids("c"), 40.0, 30.0),
                    "FIXED_SIDE",
                    vec![port(r, 6.0, 4.0, "WEST")],
                ),
                node(d, 50.0, 20.0),
            ],
            vec![
                edge(&ids("1"), p, q),
                edge(&ids("2"), p, r),
                edge(&ids("3"), p, d),
            ],
        )
    };
    let (mut nodes, mut edges) = component("a", "p", "q", "r", "d");
    let (more_nodes, more_edges) = component("f", "p5", "q5", "r5", "i");
    nodes.extend(more_nodes);
    edges.extend(more_edges);
    let laid = layout(&root(nodes, edges)).unwrap();
    // The second component sits right of the first; elkjs 0.9.3 gives these.
    assert_eq!(junctions(&laid, "a1"), [(68.0, 117.0), (68.0, 67.0)]);
    assert_eq!(junctions(&laid, "f2"), [(204.0, 117.0), (204.0, 67.0)]);
    assert!(junctions(&laid, "f1").is_empty());
}

// Mutation: drop the offset of the group in `edge_layout`, so that the junction point stays in
// the coordinates of the graph inside the group.
#[test]
fn junction_points_are_relative_to_the_container_of_their_edge() {
    for handling in ["SEPARATE_CHILDREN", "INCLUDE_CHILDREN"] {
        let inner = group(
            "g",
            vec![
                with_ports(
                    node("x", 40.0, 30.0),
                    "FIXED_SIDE",
                    vec![port("p", 6.0, 4.0, "EAST")],
                ),
                with_ports(
                    node("y", 40.0, 30.0),
                    "FIXED_SIDE",
                    vec![port("q", 6.0, 4.0, "WEST")],
                ),
                node("z", 40.0, 30.0),
            ],
        );
        let mut inner = inner;
        inner.edges = vec![
            edge("e", "p", "q"),
            edge("e2", "p", "z"),
            edge("e3", "p", "y"),
        ];
        let mut graph = root(vec![inner, node("o", 40.0, 30.0)], vec![]);
        graph
            .layout_options
            .insert("elk.hierarchyHandling".into(), handling.into());
        let laid = layout(&graph).unwrap();
        assert_eq!(find_edge(&laid, "e2").container.as_deref(), Some("g"));
        assert_eq!(
            junctions(&laid, "e2"),
            [(68.0, 31.333_333_333_333_332)],
            "{handling}"
        );
        assert!(junctions(&laid, "e").is_empty());
    }
}

// Mutation: keep the junction points the JSON came with when the layout sets some.
#[test]
fn junction_points_from_the_layout_replace_those_of_the_input() {
    let mut graph = shared_port_graph("RIGHT", "EAST", "WEST");
    graph.edges[0].junction_points = vec![ElkPoint { x: 9.0, y: 9.0 }];
    graph.edges[1].junction_points = vec![ElkPoint { x: 8.0, y: 8.0 }];
    let laid = layout(&graph).unwrap();
    assert_eq!(junctions(&laid, "e"), [(68.0, 117.0), (68.0, 67.0)]);
    // elkjs 0.9.3 keeps the input's junction points where the layout has none.
    assert_eq!(junctions(&laid, "e2"), [(8.0, 8.0)]);
}

// --- Errors: one test per variant and cause. ---

fn error_of(graph: &ElkNode) -> Error {
    layout(graph).expect_err("the graph should be refused")
}

// Mutation: in `apply_parent_node_layout`, leave the ports where they are when the group grows.
#[test]
fn a_port_on_a_group_moves_with_the_border_the_group_grows_to() {
    // The group has no edge to its children, so it has no external ports, and ELK moves the
    // ports with the new size. 24 is the padding twice, and 50 the width of the child.
    let mut a = group("a", vec![node("c", 50.0, 20.0)]);
    a.layout_options
        .insert("elk.portConstraints".into(), "FIXED_SIDE".into());
    a.ports = vec![port("e", 10.0, 10.0, "EAST"), port("w", 10.0, 10.0, "WEST")];
    let laid = layout(&root(vec![a], vec![])).unwrap();
    let (width, _) = size(&laid, "a");
    assert_eq!(width, 74.0);
    let (east_x, _) = port_at(&laid, "a", "e");
    let (west_x, _) = port_at(&laid, "a", "w");
    assert_eq!(east_x, 74.0);
    assert_eq!(west_x, -10.0);
}

// Mutation: drop the fixed-ratio check in `run_layered`. ELK fails on the first graph, and
// elk-layered would give it a layout.
#[test]
fn an_edge_across_the_border_of_a_group_with_fixed_ratio_ports_is_unsupported() {
    let build = |edges: Vec<ElkEdge>| {
        let mut a = group("a", vec![node("c", 40.0, 30.0)]);
        a.layout_options
            .insert("elk.portConstraints".into(), "FIXED_RATIO".into());
        include_children(root(vec![a, node("y", 40.0, 30.0)], edges))
    };
    assert!(matches!(
        error_of(&build(vec![edge("e", "c", "y")])),
        Error::Unsupported { element, .. } if element == "edge \"e\""
    ));
    // Nothing crosses the border of the group, so there is no external port to place.
    assert!(layout(&build(vec![edge("e", "y", "y")])).is_ok());
}

// Mutation: drop the size check in `import_external_ports`. ELK divides by the size and the
// layout fills up with NaN; elk-layered would give some other number.
#[test]
fn a_fixed_ratio_port_of_a_group_is_unsupported_only_without_a_size() {
    let build = |width: f64, height: f64| {
        let mut a = group("a", vec![node("c", 40.0, 30.0)]);
        a.width = width;
        a.height = height;
        a.layout_options
            .insert("elk.portConstraints".into(), "FIXED_RATIO".into());
        a.ports = vec![port("p", 6.0, 4.0, "WEST")];
        a.edges = vec![edge("e", "p", "c")];
        root(vec![a], vec![])
    };
    // A port on the west side divides by the height. A side along the width is not checked.
    assert!(matches!(
        error_of(&build(100.0, 0.0)),
        Error::Unsupported { element, .. } if element == "port \"p\""
    ));
    assert!(layout(&build(0.0, 100.0)).is_ok());
    assert!(layout(&build(100.0, 100.0)).is_ok());
}

// Mutation: drop `check_external_port_sides` in src/pipeline.rs. The group would get a layout
// that is not elkjs's. Or cut "which wait for issue #24" from its reason: the text must say what the refusal
// waits for.
#[test]
fn a_dummy_for_a_group_port_on_a_side_across_the_flow_is_unsupported() {
    let across = |graph: &ElkNode| {
        matches!(
            error_of(graph),
            Error::Unsupported { feature, .. }
                if feature == "ports of a node with children on a side across the layout direction, \
                    which wait for issue #24"
        )
    };
    // The group lays out its own children, and a port of it becomes an external port of that
    // layout. The port sides along the flow are fine, and the ones across it are not.
    for (direction, side) in [
        ("RIGHT", "NORTH"),
        ("LEFT", "SOUTH"),
        ("DOWN", "EAST"),
        ("UP", "WEST"),
    ] {
        let mut a = group("a", vec![node("c", 40.0, 30.0)]);
        a.layout_options
            .insert("elk.portConstraints".into(), "FIXED_SIDE".into());
        a.layout_options
            .insert("elk.direction".into(), direction.into());
        a.ports = vec![port("p", 6.0, 4.0, side)];
        a.edges = vec![edge("e", "p", "c")];
        assert!(across(&root(vec![a], vec![])), "{direction} {side}");
    }
    // With INCLUDE_CHILDREN, a group that fixes the sides of its ports gets the dummy of an edge
    // that leaves it on the east side, whatever the direction is.
    let mut a = group("a", vec![node("c", 40.0, 30.0)]);
    a.layout_options
        .insert("elk.portConstraints".into(), "FIXED_SIDE".into());
    let mut graph = include_children(root(
        vec![a, node("y", 40.0, 30.0)],
        vec![edge("e", "c", "y")],
    ));
    assert!(layout(&graph).is_ok());
    graph
        .layout_options
        .insert("elk.direction".into(), "DOWN".into());
    assert!(across(&graph));
}

// Mutation: drop the `Undefined` arm of `check_external_port_sides`. ELK fails on the graph, and
// elk-layered would give it a layout.
#[test]
fn a_group_port_without_a_side_on_a_group_without_size_is_unsupported_when_an_edge_ends_there() {
    let build = |size: f64| {
        let mut a = group("a", vec![node("c", 40.0, 30.0)]);
        a.width = size;
        a.height = size;
        a.layout_options
            .insert("elk.portConstraints".into(), "FIXED_SIDE".into());
        // The port sits outside of the left border, so a node with a size can tell its side.
        let mut p = port("p", 6.0, 4.0, "");
        p.x = -6.0;
        a.ports = vec![p];
        a.edges = vec![edge("e", "p", "c")];
        include_children(root(vec![group("outer", vec![a])], vec![]))
    };
    // Without a size, the side of the port cannot come from where the port sits.
    assert!(matches!(
        error_of(&build(0.0)),
        Error::Unsupported { feature, .. }
            if feature == "a port of a node with children that has no side, on a node with fixed port sides"
    ));
    assert!(layout(&build(100.0)).is_ok());
}

// Mutation: drop the hyperedge check in `Tree::new`.
#[test]
fn a_hyperedge_is_unsupported() {
    let mut e = edge("e", "a", "b");
    e.targets.push("c".into());
    let graph = root(
        vec![
            node("a", 1.0, 1.0),
            node("b", 1.0, 1.0),
            node("c", 1.0, 1.0),
        ],
        vec![e],
    );
    assert!(
        matches!(error_of(&graph), Error::Unsupported { element, .. } if element == "edge \"e\"")
    );
}

// Mutation: let `check_algorithm` accept every value.
#[test]
fn another_algorithm_is_unsupported() {
    let mut graph = root(vec![node("a", 1.0, 1.0)], vec![]);
    graph.layout_options = options(&[("elk.algorithm", "stress")]);
    assert!(matches!(error_of(&graph), Error::Unsupported { .. }));
    graph.layout_options = options(&[("elk.algorithm", "org.eclipse.elk.layered")]);
    assert!(layout(&graph).is_ok());
    graph.layout_options = options(&[("elk.algorithm", "layered")]);
    assert!(layout(&graph).is_ok());
}

// Mutation: refuse an edge into a separately laid out part only when it starts there.
#[test]
fn an_edge_into_a_separate_part_of_an_included_graph_is_unsupported() {
    let mut g = group("g", vec![node("x", 1.0, 1.0)]);
    g.layout_options = options(&[("elk.hierarchyHandling", "SEPARATE_CHILDREN")]);
    let graph = include_children(root(
        vec![node("p", 1.0, 1.0), g],
        vec![edge("e", "p", "x")],
    ));
    assert!(
        matches!(error_of(&graph), Error::Unsupported { element, .. } if element == "edge \"e\"")
    );
}

// Mutation: drop the duplicate check for node ids.
#[test]
fn a_duplicate_node_id_is_an_error() {
    let graph = root(vec![node("a", 1.0, 1.0), node("a", 1.0, 1.0)], vec![]);
    assert_eq!(error_of(&graph), Error::DuplicateId { id: "a".into() });
    // The root counts too.
    let graph = root(vec![node("root", 1.0, 1.0)], vec![]);
    assert_eq!(error_of(&graph), Error::DuplicateId { id: "root".into() });
}

// Mutation: drop the duplicate check for edge ids.
#[test]
fn a_duplicate_edge_id_is_an_error() {
    let graph = root(
        vec![node("a", 1.0, 1.0), node("b", 1.0, 1.0)],
        vec![edge("e", "a", "b"), edge("e", "b", "a")],
    );
    assert_eq!(error_of(&graph), Error::DuplicateId { id: "e".into() });
}

// Mutation: let `find` in `Tree::new` fall back to the root for an unknown id.
#[test]
fn a_dangling_edge_end_is_an_error() {
    let graph = root(vec![node("a", 1.0, 1.0)], vec![edge("e", "a", "nowhere")]);
    assert_eq!(
        error_of(&graph),
        Error::UnknownId {
            edge: "e".into(),
            id: "nowhere".into()
        }
    );
}

// Mutation: drop the empty-sources check in `Tree::new`.
#[test]
fn an_edge_without_a_source_is_an_error() {
    let mut e = edge("e", "a", "a");
    e.sources.clear();
    let graph = root(vec![node("a", 1.0, 1.0)], vec![e]);
    assert_eq!(error_of(&graph), Error::MissingEdgeEnd { edge: "e".into() });
}

// Mutation: let `check_number` accept negative sizes.
#[test]
fn nan_negative_and_infinite_numbers_are_errors() {
    for (width, height, x) in [
        (f64::NAN, 1.0, 0.0),
        (1.0, -1.0, 0.0),
        (1.0, 1.0, f64::INFINITY),
    ] {
        let mut a = node("a", width, height);
        a.x = x;
        assert!(matches!(
            error_of(&root(vec![a], vec![])),
            Error::InvalidSize { element, .. } if element == "node \"a\""
        ));
    }
    let mut a = node("a", 1.0, 1.0);
    a.labels = vec![label("L", -2.0, 1.0)];
    assert!(matches!(
        error_of(&root(vec![a], vec![])),
        Error::InvalidSize { element, field: "width", .. } if element == "label 0 of node \"a\""
    ));
}

// Mutation: map the pipeline's zero-seed error to `Error::Internal`.
#[test]
fn a_zero_random_seed_is_an_error() {
    let mut graph = root(vec![node("a", 1.0, 1.0)], vec![]);
    graph.layout_options = options(&[("elk.randomSeed", "0")]);
    assert_eq!(
        error_of(&graph),
        Error::ZeroRandomSeed {
            graph: "root".into()
        }
    );
}

// Mutation: map the pipeline's `InvalidGeometry` error to `Error::Internal`.
#[test]
fn a_negative_component_spacing_is_unsolvable() {
    let mut graph = root(vec![node("a", 10.0, 10.0), node("b", 10.0, 10.0)], vec![]);
    graph.layout_options = options(&[("elk.spacing.componentComponent", "-5")]);
    assert!(matches!(
        error_of(&graph),
        Error::Unsolvable { element, .. } if element == "node \"root\""
    ));
}

// Mutation: map the pipeline's `CoordinateOutOfRange` error to `Error::Internal`.
#[test]
fn a_node_too_large_for_ints_is_unsolvable() {
    let mut graph = root(
        vec![
            node("a", 10.0, 1e12),
            node("b", 10.0, 1e12),
            node("c", 10.0, 10.0),
        ],
        vec![edge("e", "a", "c"), edge("f", "b", "c")],
    );
    graph.layout_options = options(&[("elk.layered.nodePlacement.strategy", "NETWORK_SIMPLEX")]);
    assert!(matches!(error_of(&graph), Error::Unsolvable { .. }));
}

// Mutation: let `resolve` skip the dot check, so `lk.direction` names the direction.
#[test]
fn an_unknown_option_is_an_error() {
    for key in ["elk.nothing", "lk.direction", "direction", ""] {
        let mut graph = root(vec![node("a", 1.0, 1.0)], vec![]);
        graph.layout_options = options(&[(key, "DOWN")]);
        assert_eq!(
            error_of(&graph),
            Error::UnknownOption {
                element: "node \"root\"".into(),
                key: key.into()
            },
            "{key}"
        );
    }
}

// Mutation: let `apply_node_option` accept unknown ids silently.
#[test]
fn an_unsupported_option_is_an_error() {
    let cases: [(&str, &str); 5] = [
        ("elk.noLayout", "true"),
        ("elk.layered.mergeEdges", "true"),
        ("elk.layered.cycleBreaking.strategy", "INTERACTIVE"),
        ("elk.edgeRouting", "UNDEFINED"),
        // A label option on a node.
        ("elk.edgeLabels.placement", "HEAD"),
    ];
    for (key, value) in cases {
        let mut graph = root(vec![node("a", 1.0, 1.0)], vec![]);
        graph.layout_options = options(&[(key, value)]);
        assert!(
            matches!(error_of(&graph), Error::UnsupportedOption { key: k, .. } if k == key),
            "{key}"
        );
    }
    let mut e = edge("e", "a", "b");
    e.layout_options = options(&[("elk.direction", "DOWN")]);
    let graph = root(vec![node("a", 1.0, 1.0), node("b", 1.0, 1.0)], vec![e]);
    assert!(
        matches!(error_of(&graph), Error::UnsupportedOption { element, .. } if element == "edge \"e\"")
    );
}

// Mutation: make `parse_enum` compare names without case.
#[test]
fn an_unreadable_option_value_is_an_error() {
    let cases: [(&str, &str); 8] = [
        ("elk.direction", "down"),
        ("elk.direction", " DOWN"),
        ("elk.direction", "9"),
        ("elk.padding", "[top]"),
        ("elk.nodeSize.minimum", "(1)"),
        ("elk.nodeSize.constraints", "[NODE_LABELS, SOMETHING]"),
        ("elk.separateConnectedComponents", "yes"),
        ("elk.spacing.nodeNode", "NaN"),
    ];
    for (key, value) in cases {
        let mut graph = root(vec![node("a", 1.0, 1.0)], vec![]);
        graph.layout_options = options(&[(key, value)]);
        assert_eq!(
            error_of(&graph),
            Error::InvalidOptionValue {
                element: "node \"root\"".into(),
                key: key.into(),
                value: value.into()
            },
            "{key} = {value}"
        );
    }
}

// Mutation: let `resolve_all` keep the second of two keys for the same option.
#[test]
fn two_keys_for_one_option_are_an_error() {
    let mut graph = root(vec![node("a", 1.0, 1.0)], vec![]);
    graph.layout_options = options(&[
        ("elk.direction", "DOWN"),
        ("org.eclipse.elk.direction", "DOWN"),
    ]);
    assert!(matches!(error_of(&graph), Error::DuplicateOption { .. }));
}

#[test]
fn errors_read_as_plain_sentences() {
    let error = Error::UnknownId {
        edge: "e".into(),
        id: "x".into(),
    };
    assert_eq!(
        error.to_string(),
        "edge \"e\" names \"x\", which is no node or port of the graph"
    );
}

fn cross_hierarchy(unnecessary_bendpoints: &str) -> ElkNode {
    let mut g = group("g", vec![node("x", 60.0, 30.0), node("y", 40.0, 70.0)]);
    g.edges = vec![edge("exy", "x", "y")];
    let mut graph = include_children(root(
        vec![node("p", 60.0, 30.0), node("q", 60.0, 90.0), g],
        vec![
            edge("e1", "p", "y"),
            edge("e2", "q", "x"),
            edge("e3", "y", "p"),
        ],
    ));
    graph.layout_options.insert(
        "elk.layered.unnecessaryBendpoints".into(),
        unnecessary_bendpoints.into(),
    );
    graph
}

// Mutation: in `join_cross_hierarchy_edges`, never add the bend point where an edge crosses
// into another level.
#[test]
fn an_edge_across_levels_is_joined_into_one_route() {
    let third = 78.666_666_666_666_66;
    let laid = layout(&cross_hierarchy("false")).unwrap();
    assert_eq!(
        route(find_edge(&laid, "e1")),
        vec![
            (266.0, third),
            (256.0, third),
            (256.0, 125.666_666_666_666_66),
            (82.0, 125.666_666_666_666_66),
            (82.0, 82.0),
            (179.0, 82.0),
            (179.0, 80.333_333_333_333_34),
            (189.0, 80.333_333_333_333_34),
        ]
    );
    assert_eq!(
        route(find_edge(&laid, "e2")),
        vec![(72.0, 57.0), (109.0, 57.0)]
    );
    let laid = layout(&cross_hierarchy("true")).unwrap();
    assert_eq!(
        route(find_edge(&laid, "e2")),
        vec![(72.0, 57.0), (92.0, 57.0), (109.0, 57.0)]
    );
    assert_eq!(
        route(find_edge(&laid, "e3")),
        vec![
            (229.0, 68.666_666_666_666_66),
            (246.0, 68.666_666_666_666_66),
            (266.0, 68.666_666_666_666_66)
        ]
    );
    assert_eq!(route(find_edge(&laid, "e1")).len(), 10);
}

// Mutation: copy the labels of a joined edge without `original_label_edge` filtering, or give
// the pieces of a split edge no thickness: the label then sits one unit too high.
#[test]
fn a_label_on_an_edge_across_levels_comes_back_to_its_edge() {
    let mut e = edge("e1", "p", "x");
    e.labels = vec![label("c", 20.0, 10.0)];
    let graph = include_children(root(
        vec![
            node("p", 60.0, 30.0),
            group("g", vec![node("x", 60.0, 30.0)]),
        ],
        vec![e],
    ));
    let laid = layout(&graph).unwrap();
    let e = find_edge(&laid, "e1");
    assert_eq!((e.labels[0].x, e.labels[0].y), (92.0, 42.0));
    assert_eq!(route(e), vec![(72.0, 39.0), (149.0, 39.0)]);
}

// Mutation: leave out the transposition of the minimum size or of the label placement in
// `transpose_node`.
#[test]
fn a_vertical_layout_sizes_and_places_node_labels_across_it() {
    let mut a = node("a", 10.0, 10.0);
    a.labels = vec![label("a", 30.0, 10.0), label("b", 20.0, 12.0)];
    a.layout_options = options(&[
        ("elk.nodeSize.constraints", "[NODE_LABELS]"),
        ("elk.nodeLabels.placement", "[H_LEFT, V_TOP, INSIDE]"),
    ]);
    let mut graph = root(vec![a], vec![]);
    graph.layout_options = options(&[("elk.direction", "DOWN"), ("elk.spacing.labelLabel", "3")]);
    let laid = layout(&graph).unwrap();
    assert_eq!(size(&laid, "a"), (76.0, 66.0));
    let a = find(&laid, "a");
    assert_eq!((a.labels[1].x, a.labels[1].y), (5.0, 18.0));

    let mut b = node("b", 10.0, 10.0);
    b.layout_options = options(&[
        ("elk.nodeSize.constraints", "[MINIMUM_SIZE]"),
        ("elk.nodeSize.minimum", "(0, 40)"),
        ("elk.nodeSize.options", "[ASYMMETRICAL]"),
    ]);
    let mut graph = root(vec![b], vec![]);
    graph.layout_options = options(&[("elk.direction", "UP")]);
    let laid = layout(&graph).unwrap();
    assert_eq!(size(&laid, "b"), (0.0, 40.0));
}

// Mutation: let the importer ignore `ElkInputEdge::container` and fall back to its own rule,
// which puts the edge n7 -> n4 in the root. Depth-first cycle breaking then walks the edges in
// another order and reverses another edge.
#[test]
fn edges_are_imported_in_the_order_of_their_containers() {
    let n6 = group("n6", vec![node("n7", 60.0, 30.0)]);
    let n3 = group(
        "n3",
        vec![node("n4", 60.0, 44.0), node("n5", 60.0, 30.0), n6],
    );
    let mut graph = include_children(root(
        vec![node("n1", 75.5, 20.0), node("n2", 60.0, 20.0), n3],
        vec![
            edge("e0", "n7", "n4"),
            edge("e1", "n7", "n1"),
            edge("e2", "n2", "n4"),
            edge("e3", "n2", "n1"),
            edge("e4", "n4", "n7"),
        ],
    ));
    graph.layout_options.insert(
        "elk.layered.cycleBreaking.strategy".into(),
        "DEPTH_FIRST".into(),
    );
    let laid = layout(&graph).unwrap();
    assert_eq!(find_edge(&laid, "e0").container.as_deref(), Some("n3"));
    assert_eq!(find_edge(&laid, "e1").container.as_deref(), Some("root"));
    assert_eq!(at(&laid, "n4"), (146.0, 104.0));
    assert_eq!(size(&laid, "n3"), (218.0, 160.0));
    assert_eq!(at(&laid, "n2"), (12.0, 146.666_666_666_666_69));
}

/// A root with `levels` groups nested in it, and a node at the bottom.
fn nested_chain(levels: usize) -> ElkNode {
    let mut inner = node("leaf", 30.0, 20.0);
    for level in (0..levels).rev() {
        inner = group(&format!("g{level}"), vec![inner]);
    }
    root(vec![inner], vec![])
}

/// Runs in a thread with Rust's default 2 MB test stack, as an overflow would abort the run.
fn in_small_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(work)
        .expect("a thread starts")
        .join()
        .expect("the thread did not overflow")
}

// Mutation: lower `MAX_NESTING_DEPTH` to 255. The deepest allowed graph is then refused.
#[test]
fn the_deepest_allowed_nesting_lays_out() {
    // The root plus 255 groups plus the leaf: the leaf sits 256 levels below the root.
    for include in [false, true] {
        let laid_out = in_small_thread(move || {
            let mut graph = nested_chain(255);
            if include {
                graph = include_children(graph);
            }
            let result = layout(&graph);
            std::mem::forget(graph);
            result.map(|graph| graph.width)
        });
        assert!(laid_out.expect("it lays out") > 30.0);
    }
}

// Mutation: raise `MAX_NESTING_DEPTH` to 257.
#[test]
fn a_graph_nested_one_level_deeper_is_refused() {
    let error = in_small_thread(|| {
        let graph = nested_chain(256);
        let error = layout(&graph).expect_err("it is refused");
        std::mem::forget(graph);
        error
    });
    assert_eq!(
        error,
        Error::Unsupported {
            element: "node \"leaf\"".into(),
            feature: "a graph nested deeper than 256 levels".into(),
        }
    );
}

// Mutation: round the square root of the area to `f32` in `components::combine`, as Java on a
// JVM does.
#[test]
fn the_row_of_components_is_as_wide_as_the_double_square_root_allows() {
    // The two components cover 700. sqrt(700) * 3.4016802 is just below 90 as a double, which is
    // where `b` would end in the first row; as a `float` it is just above. elkjs puts `b` in a
    // second row. With 3.401681 both are above 90, and `b` stays in the first row.
    for (ratio, b) in [("3.4016802", (12.0, 42.0)), ("3.401681", (62.0, 12.0))] {
        let mut graph = root(vec![node("a", 30.0, 10.0), node("b", 40.0, 10.0)], vec![]);
        graph
            .layout_options
            .insert("elk.aspectRatio".into(), ratio.into());
        let out = layout(&graph).unwrap();
        assert_eq!(at(&out, "a"), (12.0, 12.0), "{ratio}");
        assert_eq!(at(&out, "b"), b, "{ratio}");
    }
}

// Mutation: sort the ports in `PortListSorter` with Rust's `sort_by` instead of `gwt::sort`.
#[test]
fn ports_with_and_without_an_index_sort_as_elkjs_sorts_them() {
    // ELK compares port indices only when both ports have one, so its comparator is no total
    // order. elkjs sorts with GWT's merge sort; the expected positions are what it returns.
    let data: [(Option<u32>, f64); 8] = [
        (None, 3.0),
        (Some(7), 13.0),
        (None, 23.0),
        (Some(1), 36.0),
        (Some(2), 41.0),
        (Some(6), 50.0),
        (Some(1), 60.0),
        (Some(0), 71.0),
    ];
    let ports = data
        .iter()
        .enumerate()
        .map(|(i, (index, x))| {
            let mut layout_options = options(&[("elk.port.side", "NORTH")]);
            if let Some(index) = index {
                layout_options.insert("elk.port.index".into(), index.to_string());
            }
            ElkPort {
                id: format!("p{i}"),
                x: *x,
                width: 2.0,
                height: 2.0,
                layout_options,
                ..Default::default()
            }
        })
        .collect();
    let mut n = node("n", 100.0, 40.0);
    n.ports = ports;
    n.layout_options = options(&[("elk.portConstraints", "FIXED_ORDER")]);
    let out = layout(&root(vec![n], vec![])).unwrap();
    let xs: Vec<f64> = out.children[0].ports.iter().map(|p| p.x).collect();
    assert_eq!(
        xs,
        vec![
            9.333333333333334,
            66.0,
            77.33333333333333,
            88.66666666666666,
            43.333333333333336,
            54.66666666666667,
            32.0,
            20.666666666666668
        ]
    );
}
