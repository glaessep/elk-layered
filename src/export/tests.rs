use super::*;
use crate::graph::LNode;
use crate::options::LayeredOptions;

/// A root with two nodes, each holding a graph. A point in one of them has no ancestor
/// link to a point in the other, so the offset between them needs the edge's container.
fn two_nested_graphs() -> GraphArena {
    let mut graphs = GraphArena::new(LGraph::new("root", LayeredOptions::default()));
    for (position, id) in ["a", "b"].into_iter().enumerate() {
        graphs
            .root_mut()
            .layerless_nodes
            .push(LNode::new(id, 10.0, 10.0, None));
        graphs.nest(
            GraphId::ROOT,
            NodeId::new(position),
            LGraph::new(id, LayeredOptions::default()),
        );
    }
    graphs
}

fn offset_error(containers: &HashMap<String, GraphId>) -> Option<Error> {
    let root = two_nested_graphs();
    let hierarchy = Hierarchy::new(&root);
    let parents = HashMap::new();
    let sized_nodes = HashSet::new();
    let input = TransferInput {
        parents: &parents,
        sized_nodes: &sized_nodes,
        edge_containers: containers,
    };
    hierarchical_offset(
        &hierarchy,
        (GraphId::new(1), NodeId::new(0)),
        (GraphId::new(2), NodeId::new(0)),
        "e",
        &input,
    )
    .err()
}

/// The edge `e` from `a`, inside `group`, to `b` outside it, laid out in two graphs.
fn laid_out_edge_across_levels() -> GraphArena {
    use crate::importer::{ElkInputGraph, import_graph};
    use crate::options::{ElkDirection, HierarchyHandling};
    use crate::snapshot_tests::{edge, node};

    let mut group = node("group", 40.0, 30.0, None);
    group.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let input = ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Right),
        nodes: vec![
            group,
            node("a", 40.0, 30.0, Some("group")),
            node("b", 40.0, 30.0, None),
        ],
        edges: vec![edge("e", "a", "b")],
    };
    let mut graph = import_graph(&input).expect("imports");
    crate::pipeline::execute_ported_compound_processors(&mut graph).expect("lays out");
    graph
}

fn apply_to_edge_e(graph: &mut GraphArena) -> Result<LayoutResult, Error> {
    let parents = HashMap::new();
    let sized_nodes = HashSet::new();
    let containers = HashMap::from([("e".to_string(), GraphId::ROOT)]);
    let input = TransferInput {
        parents: &parents,
        sized_nodes: &sized_nodes,
        edge_containers: &containers,
    };
    apply_layout(graph, &input)
}

// Mutation: leave out `copyJunctionPoints` in `join_cross_hierarchy_edges`. The joined edge
// then has no junction points.
#[test]
fn the_junction_points_of_an_edge_across_levels_are_joined_and_moved_to_one_graph() {
    let mut graph = laid_out_edge_across_levels();
    // Give every piece of the edge a junction point, as a router would.
    for piece in graph.iter_mut() {
        for edge in &mut piece.edges {
            edge.junction_points = vec![LPoint { x: 1.0, y: 2.0 }];
        }
    }

    let result = apply_to_edge_e(&mut graph).expect("transfers");
    let edge = result
        .edges
        .iter()
        .find(|edge| edge.id == "e")
        .expect("the edge");
    // One junction point per piece. The pieces are in two graphs, so they land apart.
    assert_eq!(edge.junction_points.len(), 2);
    assert_ne!(edge.junction_points[0], edge.junction_points[1]);
}

// Mutation: index the arena with the record's graph id (`hierarchy.graph(record.graph_id)`)
// instead of `checked_graph`. A record that names a missing graph then panics.
#[test]
fn a_cross_hierarchy_record_of_a_missing_graph_is_an_internal_error() {
    let mut graph = laid_out_edge_across_levels();
    graph.root_mut().cross_hierarchy_edges[0].graph_id = GraphId::new(99);

    let error = apply_to_edge_e(&mut graph).expect_err("an error");

    assert!(matches!(error, Error::Internal { ref message } if message.contains("99")));
}

// Mutation: return the origin's graph id unchecked from `actual_end`. The next lookup then
// indexes the arena with a missing id and panics.
#[test]
fn an_external_port_dummy_that_comes_from_a_missing_graph_is_an_internal_error() {
    let mut graph = laid_out_edge_across_levels();
    let nested = graph.children(GraphId::ROOT)[0];
    let origin = graph[nested]
        .layerless_nodes
        .iter_mut()
        .find_map(|node| node.external_port_mut()?.origin_port.as_mut())
        .expect("the edge leaves group through an external port dummy");
    origin.graph_id = GraphId::new(99);

    let error = apply_to_edge_e(&mut graph).expect_err("an error");

    assert!(matches!(error, Error::Internal { ref message } if message.contains("99")));
}

// Mutation: index `edges[record.edge]` in `join_cross_hierarchy_edges` again, without `get`.
// A record that names an edge its graph lacks then panics.
#[test]
fn a_cross_hierarchy_record_of_a_missing_edge_is_an_internal_error() {
    let mut graph = laid_out_edge_across_levels();
    graph.root_mut().cross_hierarchy_edges[0].edge = crate::graph::EdgeId::new(99);

    let error = apply_to_edge_e(&mut graph).expect_err("an error");

    assert!(matches!(error, Error::Internal { ref message } if message.contains("99")));
}

// Mutation: index `layerless_nodes[port.node]` in `actual_end` again, without `get`. An edge
// that ends at a node its graph lacks then panics.
#[test]
fn an_edge_end_at_a_missing_node_is_an_internal_error() {
    let mut graph = laid_out_edge_across_levels();
    let records: Vec<_> = graph
        .iter()
        .flat_map(|holder| holder.cross_hierarchy_edges.iter())
        .map(|record| (record.graph_id, record.edge))
        .collect();
    for (graph_id, edge) in records {
        let edge = &mut graph[graph_id].edges[edge];
        edge.map_ports(|port| {
            port.node = crate::graph::NodeId::new(99);
            Ok::<(), std::convert::Infallible>(())
        })
        .unwrap();
    }

    let error = apply_to_edge_e(&mut graph).expect_err("an error");

    assert!(matches!(error, Error::Internal { ref message } if message.contains("99")));
}

// Mutation: return `Ok(LPoint::default())` when the edge has no container.
#[test]
fn an_edge_without_a_container_is_an_internal_error() {
    let error = offset_error(&HashMap::new()).expect("an error");
    assert!(matches!(error, Error::Internal { ref message } if message.contains("\"e\"")));
}

// Mutation: leave out `hierarchy.checked_graph(target_graph)?` in `hierarchical_offset`. The
// walk then ends at the root and returns an offset for a container that does not exist.
#[test]
fn a_container_that_names_a_missing_graph_is_an_internal_error() {
    let containers = HashMap::from([("e".to_string(), GraphId::new(99))]);
    let error = offset_error(&containers).expect("an error");
    assert!(matches!(error, Error::Internal { ref message } if message.contains("99")));
}

// Mutation: look the container up by the name of its graph again (`Hierarchy::by_name`, with
// the edge's container as a node id). The graph in node `a` is named like the root, so the
// root's edge then reads as an edge of that graph and gets no offset.
//
// `layout()` refuses a node with the root's id, so only a hand-built arena shows this.
#[test]
fn a_nested_graph_named_like_the_root_does_not_take_the_roots_edges() {
    let mut graphs = two_nested_graphs();
    graphs[GraphId::new(1)].name = "root".to_string();
    graphs.root_mut().layerless_nodes[NodeId::new(0)].position = LPoint { x: 5.0, y: 7.0 };
    graphs.root_mut().padding.left = 3.0;
    graphs.root_mut().padding.top = 4.0;
    let hierarchy = Hierarchy::new(&graphs);
    let parents = HashMap::new();
    let sized_nodes = HashSet::new();
    let containers = HashMap::from([("e".to_string(), GraphId::ROOT)]);
    let input = TransferInput {
        parents: &parents,
        sized_nodes: &sized_nodes,
        edge_containers: &containers,
    };

    let offset = hierarchical_offset(
        &hierarchy,
        (GraphId::new(1), crate::graph::NodeId::new(0)),
        (GraphId::new(2), crate::graph::NodeId::new(0)),
        "e",
        &input,
    )
    .expect("the container has a graph");

    // From the graph in `a` up to the root: the node's position plus the root's padding.
    assert_eq!((offset.x, offset.y), (8.0, 11.0));
}

// Mutation: return an error whenever the container is found.
#[test]
fn a_known_container_gives_no_error() {
    let containers = HashMap::from([("e".to_string(), GraphId::ROOT)]);
    assert!(offset_error(&containers).is_none());
}
