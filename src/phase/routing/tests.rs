//! Tests of the classes in this module.

use crate::graph::LGraph;
use crate::graph::{EdgeId, LNode, LPoint, LayeredEdge, PortSide, PortType};
use crate::options::LayeredOptions;
use crate::phase::routing::orthogonal_edge_router::route_edges_orthogonal;

#[test]
fn orthogonal_router_places_layers_and_routes_between_them() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    let source = graph.next_node_id();
    graph
        .layerless_nodes
        .push(LNode::new("A", 40.0, 20.0, Some(0)));
    let target = graph.next_node_id();
    let mut target_node = LNode::new("B", 40.0, 20.0, Some(1));
    target_node.position.y = 40.0;
    graph.layerless_nodes.push(target_node);
    graph.set_node_layer(source, 0);
    graph.set_node_layer(target, 1);
    graph.layer_size_mut(0).width = 40.0;
    graph.layer_size_mut(1).width = 40.0;

    let source_port = graph
        .add_port(
            source,
            PortType::Output,
            PortSide::East,
            LPoint { x: 40.0, y: 10.0 },
        )
        .unwrap();
    let target_port = graph
        .add_port(
            target,
            PortType::Output,
            PortSide::West,
            LPoint { x: 0.0, y: 10.0 },
        )
        .unwrap();
    graph.add_edge({
        let mut built_edge = LayeredEdge::new("A-B".to_string(), source_port, target_port);
        built_edge.source_node_id = "A".to_string();
        built_edge.target_node_id = "B".to_string();
        built_edge.model_order = Some(0);
        built_edge
    });

    route_edges_orthogonal(&mut graph);

    assert_eq!(graph.layerless_nodes[source].position.x, 0.0);
    assert_eq!(graph.layerless_nodes[target].position.x, 60.0);
    assert_eq!(graph.size.width, 100.0);
    assert_eq!(
        graph.edges[EdgeId::new(0)].bend_points,
        vec![LPoint { x: 50.0, y: 10.0 }, LPoint { x: 50.0, y: 50.0 }]
    );
}

#[test]
fn orthogonal_router_centers_unconnected_nodes_in_wide_layers() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    let wide = graph.next_node_id();
    graph
        .layerless_nodes
        .push(LNode::new("wide", 80.0, 20.0, Some(0)));
    let narrow = graph.next_node_id();
    graph
        .layerless_nodes
        .push(LNode::new("narrow", 20.0, 20.0, Some(1)));
    graph.set_node_layer(wide, 0);
    graph.set_node_layer(narrow, 1);
    graph.layer_size_mut(0).width = 80.0;
    graph.layer_size_mut(1).width = 80.0;

    route_edges_orthogonal(&mut graph);

    let expected_narrow_x =
        80.0 + graph.options.spacing.node_node_between_layers + (80.0 - 20.0) / 2.0;
    assert_eq!(graph.layerless_nodes[wide].position.x, 0.0);
    assert_eq!(graph.layerless_nodes[narrow].position.x, expected_narrow_x);
}
