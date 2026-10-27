//! The graph types read and write elkjs's JSON shape.

use elk_layered::{ElkNode, layout};
use serde_json::{Value, json};

// Mutation: drop `default` from `ElkNode::x`.
#[test]
fn missing_numbers_and_lists_read_as_zero_and_empty() {
    let graph: ElkNode = serde_json::from_value(json!({"id": "root"})).unwrap();
    assert_eq!(
        graph,
        ElkNode {
            id: "root".into(),
            ..Default::default()
        }
    );
}

// Mutation: drop `visit_i64` from the option value visitor.
#[test]
fn option_values_may_be_strings_numbers_or_booleans() {
    let graph: ElkNode = serde_json::from_value(json!({
        "id": "root",
        "layoutOptions": {"a": "x", "b": 40, "c": 2.5, "d": false, "e": -3}
    }))
    .unwrap();
    let values: Vec<&str> = graph.layout_options.values().map(String::as_str).collect();
    assert_eq!(values, ["x", "40", "2.5", "false", "-3"]);
    assert!(
        serde_json::from_value::<ElkNode>(json!({"id": "r", "layoutOptions": {"a": [1]}})).is_err()
    );
}

// Mutation: drop `skip_serializing_if` from `ElkEdgeSection::bend_points`.
#[test]
fn the_result_has_the_fields_elkjs_writes() {
    let graph: ElkNode = serde_json::from_value(json!({
        "id": "root",
        "children": [
            {"id": "a", "width": 60, "height": 30, "labels": [{"text": "A"}]},
            {"id": "b", "width": 60, "height": 30}
        ],
        "edges": [{"id": "e", "sources": ["a"], "targets": ["b"]}]
    }))
    .unwrap();
    let got = serde_json::to_value(layout(&graph).unwrap()).unwrap();
    let want = json!({
        "id": "root", "x": 0.0, "y": 0.0, "width": 164.0, "height": 54.0,
        "children": [
            {"id": "a", "x": 12.0, "y": 12.0, "width": 60.0, "height": 30.0,
             "labels": [{"text": "A", "x": 0.0, "y": 0.0, "width": 0.0, "height": 0.0}]},
            {"id": "b", "x": 92.0, "y": 12.0, "width": 60.0, "height": 30.0}
        ],
        "edges": [{
            "id": "e", "sources": ["a"], "targets": ["b"], "container": "root",
            "sections": [{
                "id": "e_s0",
                "startPoint": {"x": 72.0, "y": 27.0},
                "endPoint": {"x": 92.0, "y": 27.0},
                "incomingShape": "a",
                "outgoingShape": "b"
            }]
        }]
    });
    assert_eq!(got, want);
}

// Mutation: rename `junction_points` to `junction_points` in JSON.
#[test]
fn every_field_reads_and_writes_in_camel_case() {
    let text = json!({
        "id": "root", "x": 1.5, "y": 2.0, "width": 3.0, "height": 4.0,
        "layoutOptions": {"elk.direction": "DOWN"},
        "ports": [{"id": "p", "x": 0.0, "y": 0.0, "width": 1.0, "height": 1.0,
                   "labels": [{"id": "pl", "text": "P", "x": 0.0, "y": 0.0, "width": 1.0, "height": 1.0}]}],
        "edges": [{
            "id": "e", "sources": ["p"], "targets": ["p"], "container": "root",
            "junctionPoints": [{"x": 5.0, "y": 6.0}],
            "sections": [{"id": "s", "startPoint": {"x": 0.0, "y": 0.0},
                          "endPoint": {"x": 1.0, "y": 1.0}, "bendPoints": [{"x": 0.5, "y": 0.0}],
                          "incomingShape": "p", "outgoingShape": "p"}]
        }]
    });
    let graph: ElkNode = serde_json::from_value(text.clone()).unwrap();
    assert_eq!(graph.edges[0].junction_points[0].y, 6.0);
    assert_eq!(graph.ports[0].labels[0].id.as_deref(), Some("pl"));
    let back: Value = serde_json::to_value(&graph).unwrap();
    assert_eq!(back, text);
}

// Mutation: drop `alias = "properties"` from `ElkNode::layout_options`.
#[test]
fn the_legacy_properties_key_reads_like_layout_options() {
    let graph = |key: &str| -> ElkNode {
        serde_json::from_value(json!({
            "id": "root",
            key: {"elk.direction": "DOWN"},
            "children": [
                {"id": "a", "width": 60, "height": 30},
                {"id": "b", "width": 60, "height": 30}
            ],
            "edges": [{"id": "e", "sources": ["a"], "targets": ["b"]}]
        }))
        .unwrap()
    };
    let legacy = graph("properties");
    assert_eq!(legacy.layout_options["elk.direction"], "DOWN");
    let got = layout(&legacy).unwrap();
    assert_eq!(got, layout(&graph("layoutOptions")).unwrap());
    // DOWN stacks the nodes: the second sits below the first, not to its right.
    assert!(got.children[1].y > got.children[0].y);
    assert_eq!(got.children[1].x, got.children[0].x);
}

// Mutation: drop `alias = "properties"` from `ElkPort::layout_options`.
#[test]
fn the_legacy_properties_key_reads_on_ports_edges_and_labels() {
    let graph: ElkNode = serde_json::from_value(json!({
        "id": "root",
        "ports": [{"id": "p", "properties": {"a": "1"}, "labels": [{"text": "t", "properties": {"d": "4"}}]}],
        "edges": [{"id": "e", "properties": {"b": "2"}}]
    }))
    .unwrap();
    assert_eq!(graph.ports[0].layout_options["a"], "1");
    assert_eq!(graph.ports[0].labels[0].layout_options["d"], "4");
    assert_eq!(graph.edges[0].layout_options["b"], "2");
}

#[test]
fn a_graph_with_both_option_keys_is_an_error() {
    let both = json!({"id": "r", "layoutOptions": {}, "properties": {}});
    assert!(serde_json::from_value::<ElkNode>(both).is_err());
}

// Mutation: drop `alias = "source"` from `ElkEdge::sources`.
#[test]
fn the_old_edge_form_reads_like_sources_and_targets() {
    let graph = |edge: Value| -> ElkNode {
        serde_json::from_value(json!({
            "id": "root",
            "children": [
                {"id": "a", "width": 60, "height": 30},
                {"id": "b", "width": 60, "height": 30}
            ],
            "edges": [edge]
        }))
        .unwrap()
    };
    let old = graph(json!({"id": "e", "source": "a", "target": "b"}));
    assert_eq!(old.edges[0].sources, ["a"]);
    assert_eq!(old.edges[0].targets, ["b"]);
    let new = graph(json!({"id": "e", "sources": ["a"], "targets": ["b"]}));
    assert_eq!(layout(&old).unwrap(), layout(&new).unwrap());
    // The result is written in the new form.
    let back = serde_json::to_value(layout(&old).unwrap()).unwrap();
    assert_eq!(back["edges"][0]["sources"], json!(["a"]));
    assert!(back["edges"][0].get("source").is_none());
}

// Mutation: let the `ends` deserializer accept only a list.
#[test]
fn an_edge_end_is_a_string_or_a_list_of_strings() {
    let edge = |ends: Value| -> Result<ElkNode, _> {
        serde_json::from_value(json!({"id": "r", "edges": [{"id": "e", "sources": ends}]}))
    };
    assert!(edge(json!("a")).is_ok());
    assert!(edge(json!(["a", "b"])).is_ok());
    assert!(edge(json!(5)).is_err());
    assert!(edge(json!([5])).is_err());
    assert!(edge(json!({"id": "a"})).is_err());
    let both = json!({"id": "r", "edges": [{"id": "e", "source": "a", "sources": ["a"]}]});
    assert!(serde_json::from_value::<ElkNode>(both).is_err());
}

// Mutation: drop the `source_port` and `target_port` fields of `EdgeWire`, so the old port keys
// read as nothing and the edge is laid out at the node.
#[test]
fn the_old_edge_form_attaches_to_a_port_when_it_gives_one() {
    let edge = |edge: Value| -> ElkNode {
        serde_json::from_value(json!({
            "id": "root",
            "children": [
                {"id": "a", "width": 60, "height": 30, "layoutOptions": {"elk.portConstraints": "FIXED_SIDE"},
                 "ports": [{"id": "p", "width": 6, "height": 4, "layoutOptions": {"elk.port.side": "EAST"}}]},
                {"id": "b", "width": 60, "height": 30, "layoutOptions": {"elk.portConstraints": "FIXED_SIDE"},
                 "ports": [{"id": "q", "width": 6, "height": 4, "layoutOptions": {"elk.port.side": "WEST"}}]}
            ],
            "edges": [edge]
        }))
        .unwrap()
    };
    let ends = |graph: &ElkNode| {
        (
            graph.edges[0].sources.clone(),
            graph.edges[0].targets.clone(),
        )
    };
    // The port wins over the node, on either end, and a missing key keeps the node.
    assert_eq!(
        ends(&edge(
            json!({"id": "e", "source": "a", "target": "b", "sourcePort": "p"})
        )),
        (vec!["p".to_string()], vec!["b".to_string()])
    );
    assert_eq!(
        ends(&edge(
            json!({"id": "e", "source": "a", "target": "b", "targetPort": "q"})
        )),
        (vec!["a".to_string()], vec!["q".to_string()])
    );
    assert_eq!(
        ends(&edge(
            json!({"id": "e", "source": "a", "target": "b", "sourcePort": "p", "targetPort": "q"})
        )),
        (vec!["p".to_string()], vec!["q".to_string()])
    );
    // The result is the same layout as the new form with port ids.
    let new = edge(json!({"id": "e", "sources": ["p"], "targets": ["q"]}));
    let old = edge(
        json!({"id": "e", "source": "a", "target": "b", "sourcePort": "p", "targetPort": "q"}),
    );
    assert_eq!(
        layout(&old).unwrap().edges[0].sections,
        layout(&new).unwrap().edges[0].sections
    );
}

// Mutation: read the port keys also in the new form. elkjs ignores them there.
#[test]
fn the_new_edge_form_ignores_the_old_port_keys() {
    let graph: ElkNode = serde_json::from_value(json!({
        "id": "root",
        "edges": [{"id": "e", "sources": ["a"], "targets": ["b"], "sourcePort": "p", "targetPort": "q"}]
    }))
    .unwrap();
    assert_eq!(graph.edges[0].sources, ["a"]);
    assert_eq!(graph.edges[0].targets, ["b"]);
    // `source` next to `targets` is not the old form either.
    let mixed: ElkNode = serde_json::from_value(json!({
        "id": "root",
        "edges": [{"id": "e", "source": "a", "targets": ["b"], "sourcePort": "p"}]
    }))
    .unwrap();
    assert_eq!(mixed.edges[0].sources, ["a"]);
}

// Mutation: let a port key that names no port keep the edge at the node. Here the edge names the
// id, which is no node, and layout reports it.
#[test]
fn an_old_port_key_that_names_no_port_is_an_unknown_id() {
    let graph: ElkNode = serde_json::from_value(json!({
        "id": "root",
        "children": [{"id": "a", "width": 60, "height": 30}, {"id": "b", "width": 60, "height": 30}],
        "edges": [{"id": "e", "source": "a", "target": "b", "sourcePort": "nowhere"}]
    }))
    .unwrap();
    assert!(matches!(
        layout(&graph),
        Err(elk_layered::Error::UnknownId { id, .. }) if id == "nowhere"
    ));
}

// Mutation: read a `null` port key as an id.
#[test]
fn a_null_old_port_key_is_no_port() {
    let graph: ElkNode = serde_json::from_value(json!({
        "id": "root",
        "edges": [{"id": "e", "source": "a", "target": "b", "sourcePort": null, "targetPort": null}]
    }))
    .unwrap();
    assert_eq!(graph.edges[0].sources, ["a"]);
    assert_eq!(graph.edges[0].targets, ["b"]);
}
