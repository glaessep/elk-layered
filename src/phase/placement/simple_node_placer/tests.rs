use super::*;
use crate::graph::{LMargin, NodeId};
use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputNode, import_graph};
use crate::options::{ElkDirection, LayeredOptions};

fn node(id: &str, width: f64, height: f64) -> ElkInputNode {
    ElkInputNode {
        ports: Vec::new(),
        parent_settings: Default::default(),
        id: id.to_string(),
        width,
        height,
        parent: None,
        direction: None,
        hierarchy_handling: None,
        layer_constraint: None,
        port_alignment: None,
        port_constraints: None,
        node_label_placement: crate::options::NodeLabelPlacement::FIXED,
        node_flexibility: crate::options::NodeFlexibility::None,
        ports_surrounding: None,
        nested_options: None,
        compound_node: false,
        labels: Vec::new(),
        node_size: Default::default(),
        self_loop_distribution: Default::default(),
        self_loop_ordering: Default::default(),
        port_label_placement: Default::default(),
        port_labels_treat_as_group: true,
    }
}

fn edge(id: &str, source: &str, target: &str) -> ElkInputEdge {
    ElkInputEdge {
        source_port: None,
        target_port: None,
        id: id.to_string(),
        source: source.to_string(),
        target: target.to_string(),
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

fn graph(nodes: Vec<ElkInputNode>, edges: Vec<ElkInputEdge>) -> crate::graph::GraphArena {
    import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes,
        edges,
    })
    .unwrap()
}

#[test]
fn simple_placer_centers_shorter_layers() {
    let mut graph = graph(
        vec![
            node("A", 80.0, 20.0),
            node("B", 80.0, 20.0),
            node("C", 80.0, 100.0),
        ],
        vec![edge("A-C", "A", "C"), edge("B-C", "B", "C")],
    );
    graph.set_node_layer(NodeId::new(0), 0);
    graph.set_node_layer(NodeId::new(1), 0);
    graph.set_node_layer(NodeId::new(2), 1);

    place_nodes_simple(&mut graph);

    assert_eq!(graph.layers()[0].size.height, 80.0);
    assert_eq!(graph.layers()[1].size.height, 100.0);
    assert_eq!(graph.layerless_nodes[NodeId::new(0)].position.y, 10.0);
    assert_eq!(graph.layerless_nodes[NodeId::new(1)].position.y, 70.0);
    assert_eq!(graph.layerless_nodes[NodeId::new(2)].position.y, 0.0);
}

#[test]
fn simple_placer_respects_node_margins() {
    let mut graph = graph(vec![node("A", 80.0, 20.0), node("B", 80.0, 20.0)], vec![]);
    graph.set_node_layer(NodeId::new(0), 0);
    graph.set_node_layer(NodeId::new(1), 0);
    graph.layerless_nodes[NodeId::new(0)].margin = LMargin {
        top: 3.0,
        bottom: 7.0,
        ..LMargin::default()
    };
    graph.layerless_nodes[NodeId::new(1)].margin = LMargin {
        top: 5.0,
        bottom: 11.0,
        ..LMargin::default()
    };

    place_nodes_simple(&mut graph);

    assert_eq!(graph.layers()[0].size.height, 106.0);
    assert_eq!(graph.layerless_nodes[NodeId::new(0)].position.y, 3.0);
    assert_eq!(graph.layerless_nodes[NodeId::new(1)].position.y, 75.0);
}
