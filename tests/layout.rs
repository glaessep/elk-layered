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

// Mutation: let `finish` always name the section `<id>_s0`.
#[test]
fn an_edge_with_a_section_keeps_its_id() {
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
    assert!(e.junction_points.is_empty());
}

// --- Errors: one test per variant and cause. ---

fn error_of(graph: &ElkNode) -> Error {
    layout(graph).expect_err("the graph should be refused")
}

// Mutation: drop the ports check in `Tree::new`.
#[test]
fn ports_are_unsupported() {
    let mut a = node("a", 60.0, 30.0);
    a.ports = vec![ElkPort {
        id: "p".into(),
        ..Default::default()
    }];
    assert!(matches!(
        error_of(&root(vec![a], vec![])),
        Error::Unsupported { feature, .. } if feature == "ports"
    ));
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
    let cases: [(&str, &str); 6] = [
        ("elk.noLayout", "true"),
        ("elk.layered.mergeEdges", "true"),
        ("elk.layered.cycleBreaking.strategy", "INTERACTIVE"),
        ("elk.nodeSize.constraints", "[PORTS]"),
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
        "edge \"e\" names \"x\", which is no node of the graph"
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
