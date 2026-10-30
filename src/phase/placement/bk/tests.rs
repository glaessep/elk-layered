//! Tests of the classes in this module.

use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputNode, import_graph};
use crate::options::EdgeStraighteningStrategy;
use crate::options::{ElkDirection, GreedySwitchType, LayeredOptions};
use crate::pipeline::{LayeredPhase, run_processors_until_phase};

fn node(id: &str) -> ElkInputNode {
    ElkInputNode {
        ports: Vec::new(),
        parent_settings: Default::default(),
        id: id.to_string(),
        width: 80.0,
        height: 40.0,
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
        inside_self_loop: false,
        model_order: None,
        priority_direction: 0,
        priority_shortness: 0,
        priority_straightness: 0,
        container: None,
    }
}

#[test]
fn bk_node_placer_assigns_order_preserving_y_coordinates() {
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions {
            direction: ElkDirection::Right,
            greedy_switch_type: GreedySwitchType::Off,
            node_placement_bk_edge_straightening: EdgeStraighteningStrategy::None,
            ..LayeredOptions::default()
        },
        nodes: vec![node("A"), node("B"), node("C")],
        edges: vec![edge("A-C", "A", "C"), edge("B-C", "B", "C")],
    })
    .unwrap();

    run_processors_until_phase(&mut graph, LayeredPhase::P4NodePlacement).unwrap();

    // Every layer must keep its nodes apart in y, margins included.
    assert!(!graph.layers().is_empty());
    for layer in graph.layers() {
        let mut bottom = f64::NEG_INFINITY;
        for node in layer.nodes() {
            let lnode = &graph.layerless_nodes[*node];
            assert!(lnode.position.y - lnode.margin.top > bottom);
            bottom = lnode.position.y + lnode.size.height + lnode.margin.bottom;
        }
    }
}
