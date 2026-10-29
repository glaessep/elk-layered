//! Connected-component layout, SimpleRow packing and component-group packing from Eclipse ELK
//! (EPL-2.0).
//!
//! Source references (62d5909f96fad541bc101ad52dabaece6b7eab7e):
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/components/ComponentsProcessor.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/components/SimpleRowGraphPlacer.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/components/AbstractGraphPlacer.java
//!
//! `group.rs` holds the placer that ELK uses when the graph has external ports.

mod group;

use crate::graph::{
    EdgeId, GraphArena, GraphId, IdVec, LGraph, LLabel, LNodeKind, LPoint, LSize, NodeId, PortRef,
    VecId,
};
use crate::options::{HierarchyHandling, PortConstraints};
use crate::pipeline::PipelineResult;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ComponentError {
    #[error("flat ELK component processing cannot own cross-hierarchy edges")]
    HierarchyEdges,
    #[error("ELK component {kind} reference {index} is outside its component")]
    InvalidReference { kind: &'static str, index: usize },
    #[error("ELK component packing requires finite nonnegative geometry and spacing")]
    InvalidGeometry,
}

pub(crate) struct Component {
    pub(crate) graph: LGraph,
    original_nodes: Vec<NodeId>,
    original_edges: Vec<EdgeId>,
    priority: i32,
    /// The sides of the external ports the component joins (ELK's `EXT_PORT_CONNECTIONS`), as
    /// `group::side_bit` makes them.
    ext_port_sides: u8,
}

/// The supplied graph has already passed GraphConfigurator. Component construction copies
/// properties, not a new configuration or random seed.
///
/// What `split` changes, and what the caller owes:
/// - The root graph in `graphs` keeps its node and edge lists. The components hold copies of them
///   under new, component-local indices.
/// - The graphs inside the root's nodes are shared, so they cannot be copied per component.
///   `split` renumbers their references to the root in place (the origin of an external port
///   dummy, the port dummies, the cross-hierarchy edges), so that they match the component that
///   holds the node. [`combine`] renumbers them back.
/// - `GraphParent.node` of those graphs is not renumbered. It keeps naming the node's index in
///   the root, which the root still holds, and nothing reads it while components run: only
///   compound preprocessing reads it, and that never runs after `split`. `combine` puts every
///   original node back at its original index, so the value is right again afterwards.
/// - After an error between `split` and the end of `combine`, the references of the nested graphs
///   are half renumbered, and `graphs` is not valid. The caller drops it. The one caller,
///   `execute_ported_processors`, hands the error up, and `layout` drops the graphs.
pub(crate) fn split(graphs: &mut GraphArena) -> PipelineResult<Option<Vec<Component>>> {
    let graph = graphs.root();
    if !graph.options.separate_connected_components
        || graph.options.hierarchy_handling == HierarchyHandling::IncludeChildren
        || (graph.graph_properties.external_ports
            && matches!(
                graph.options.port_constraints,
                PortConstraints::FixedOrder
                    | PortConstraints::FixedRatio
                    | PortConstraints::FixedPos
            ))
    {
        return Ok(None);
    }
    if !graph.hierarchy_edges.is_empty() || !graph.cross_hierarchy_edges.is_empty() {
        return Err(ComponentError::HierarchyEdges.into());
    }
    let node_count = graph.layerless_nodes.len();
    let mut owners = IdVec::filled(node_count, usize::MAX);
    let mut groups = Vec::<Vec<NodeId>>::new();
    let mut stack = Vec::new();
    for start in graph.node_ids() {
        if owners[start] != usize::MAX {
            continue;
        }
        let owner = groups.len();
        let mut nodes = Vec::new();
        stack.push(start);
        while let Some(index) = stack.pop() {
            let node =
                graph
                    .layerless_nodes
                    .get(index)
                    .ok_or(ComponentError::InvalidReference {
                        kind: "node",
                        index: index.index(),
                    })?;
            if owners[index] != usize::MAX {
                continue;
            }
            owners[index] = owner;
            nodes.push(index);
            // LPort.getConnectedPorts visits incoming then outgoing edges; reverse the
            // pushes so this iterative traversal retains the source recursive DFS order.
            for port in node.ports.iter().rev() {
                for &edge in port.outgoing_edges.iter().rev() {
                    let edge = graph
                        .edges
                        .get(edge)
                        .ok_or(ComponentError::InvalidReference {
                            kind: "edge",
                            index: edge.index(),
                        })?;
                    stack.push(edge.target.node);
                }
                for &edge in port.incoming_edges.iter().rev() {
                    let edge = graph
                        .edges
                        .get(edge)
                        .ok_or(ComponentError::InvalidReference {
                            kind: "edge",
                            index: edge.index(),
                        })?;
                    stack.push(edge.source.node);
                }
            }
        }
        groups.push(nodes);
    }
    let mut components = groups
        .into_iter()
        .map(|original_nodes| {
            let mut component = graph.component_shell();
            component.layerless_nodes = original_nodes
                .iter()
                .map(|&node| graph.layerless_nodes[node].clone())
                .collect();
            let priority = component
                .layerless_nodes
                .iter()
                .fold(0i32, |sum, node| sum.wrapping_add(node.component_priority));
            // ComponentsProcessor.dfs: the side of every external port dummy of the component.
            let ext_port_sides = component
                .layerless_nodes
                .iter()
                .filter(|node| node.kind == LNodeKind::ExternalPort)
                .fold(group::SIDES_NONE, |sides, node| {
                    sides | group::side_bit(node.external_port_side)
                });
            Component {
                graph: component,
                original_nodes,
                original_edges: Vec::new(),
                priority,
                ext_port_sides,
            }
        })
        .collect::<Vec<_>>();
    for (index, edge) in graph.edges_with_ids() {
        let owner = *owners
            .get(edge.source.node)
            .ok_or(ComponentError::InvalidReference {
                kind: "source node",
                index: edge.source.node.index(),
            })?;
        if owners.get(edge.target.node) != Some(&owner) {
            return Err(ComponentError::InvalidReference {
                kind: "target node",
                index: edge.target.node.index(),
            }
            .into());
        }
        components[owner].original_edges.push(index);
        components[owner].graph.edges.push(edge.clone());
    }
    for &node in &graph.hidden_nodes {
        let owner = component_owner(&owners, node)?;
        components[owner].graph.hidden_nodes.push(node);
    }
    for &node in &graph.replaced_external_port_dummies {
        let owner = component_owner(&owners, node)?;
        components[owner]
            .graph
            .replaced_external_port_dummies
            .push(node);
    }
    for holder in &graph.self_loop_holders {
        let owner = component_owner(&owners, holder.node)?;
        components[owner]
            .graph
            .self_loop_holders
            .push(holder.clone());
    }
    // Reuse the global maps across components: clearing only each component's entries
    // keeps this O(V + E), including a graph made entirely of isolated nodes.
    let mut node_map = IdVec::filled(node_count, None);
    let mut edge_map = IdVec::filled(graph.edges.len(), None);
    for component in &mut components {
        for (local, &original) in component.original_nodes.iter().enumerate() {
            node_map[original] = Some(NodeId::new(local));
        }
        for (local, &original) in component.original_edges.iter().enumerate() {
            edge_map[original] = Some(EdgeId::new(local));
        }
        remap_references(&mut component.graph, &node_map, &edge_map)?;
        remap_nested_references(graphs, &component.graph, &node_map, &edge_map)?;
        component.graph.clear_layers();
        for &original in &component.original_nodes {
            node_map[original] = None;
        }
        for &original in &component.original_edges {
            edge_map[original] = None;
        }
    }
    Ok(Some(components))
}

fn component_owner(owners: &IdVec<NodeId, usize>, node: NodeId) -> Result<usize, ComponentError> {
    owners
        .get(node)
        .copied()
        .filter(|&owner| owner != usize::MAX)
        .ok_or(ComponentError::InvalidReference {
            kind: "node",
            index: node.index(),
        })
}

/// One entry of a map from old ids to new ones. `split` maps through `Option<I>`, because a node
/// can stay behind. `combine` maps through plain `I`, because every node of a component has a new id.
trait Slot<I> {
    fn mapped(&self) -> Option<I>;
}

impl<I: VecId> Slot<I> for I {
    fn mapped(&self) -> Option<I> {
        Some(*self)
    }
}

impl<I: VecId> Slot<I> for Option<I> {
    fn mapped(&self) -> Option<I> {
        *self
    }
}

fn map_id<I: VecId, S: Slot<I>>(
    map: &IdVec<I, S>,
    id: I,
    kind: &'static str,
) -> Result<I, ComponentError> {
    map.get(id)
        .and_then(Slot::mapped)
        .ok_or(ComponentError::InvalidReference {
            kind,
            index: id.to_index(),
        })
}

fn map_optional<I: VecId, S: Slot<I>>(
    value: &mut Option<I>,
    map: &IdVec<I, S>,
    kind: &'static str,
) -> Result<(), ComponentError> {
    if let Some(id) = value {
        *id = map_id(map, *id, kind)?;
    }
    Ok(())
}

fn map_list<I: VecId, S: Slot<I>>(
    values: &mut [I],
    map: &IdVec<I, S>,
    kind: &'static str,
) -> Result<(), ComponentError> {
    for id in values {
        *id = map_id(map, *id, kind)?;
    }
    Ok(())
}

fn map_port<N: Slot<NodeId>>(
    port: &mut PortRef,
    nodes: &IdVec<NodeId, N>,
) -> Result<(), ComponentError> {
    port.node = map_id(nodes, port.node, "port node")?;
    Ok(())
}

fn map_labels<E: Slot<EdgeId>>(
    labels: &mut [LLabel],
    edges: &IdVec<EdgeId, E>,
) -> Result<(), ComponentError> {
    for label in labels {
        map_optional(&mut label.end_label_edge, edges, "label edge")?;
    }
    Ok(())
}

/// Maps every arena reference, including processor-created dummies and detached edges.
/// Port ids are node-local and are deliberately unchanged.
fn remap_references<N: Slot<NodeId>, E: Slot<EdgeId>>(
    graph: &mut LGraph,
    nodes: &IdVec<NodeId, N>,
    edges: &IdVec<EdgeId, E>,
) -> Result<(), ComponentError> {
    for node in &mut graph.layerless_nodes {
        map_optional(
            &mut node.replaced_external_port_dummy,
            nodes,
            "replaced dummy",
        )?;
        map_list(&mut node.in_layer_successor_constraints, nodes, "successor")?;
        map_optional(&mut node.in_layer_layout_unit, nodes, "layout unit")?;
        map_list(
            &mut node.barycenter_associates,
            nodes,
            "barycenter associate",
        )?;
        map_optional(&mut node.origin_edge, edges, "origin edge")?;
        for port in [&mut node.long_edge_source, &mut node.long_edge_target]
            .into_iter()
            .flatten()
        {
            map_port(port, nodes)?;
        }
        map_labels(&mut node.labels, edges)?;
        map_labels(&mut node.represented_labels, edges)?;
        for port in node.ports.slots_mut() {
            port.node = map_id(nodes, port.node, "port owner")?;
            map_optional(&mut port.long_edge_target_node, nodes, "long edge target")?;
            if let Some(origin) = &mut port.origin_port {
                map_port(origin, nodes)?;
            }
            map_list(&mut port.incoming_edges, edges, "incoming edge")?;
            map_list(&mut port.outgoing_edges, edges, "outgoing edge")?;
            map_labels(&mut port.labels, edges)?;
        }
    }
    for edge in &mut graph.edges {
        map_port(&mut edge.source, nodes)?;
        map_port(&mut edge.target, nodes)?;
        if let Some(port) = &mut edge.original_opposite_port {
            map_port(port, nodes)?;
        }
        map_labels(&mut edge.labels, edges)?;
    }
    for layer in &mut graph.layers {
        map_list(&mut layer.nodes, nodes, "layer node")?;
    }
    map_list(&mut graph.hidden_nodes, nodes, "hidden node")?;
    map_list(
        &mut graph.replaced_external_port_dummies,
        nodes,
        "external dummy",
    )?;
    for holder in &mut graph.self_loop_holders {
        holder.node = map_id(nodes, holder.node, "self loop owner")?;
        for hyper_loop in &mut holder.hyper_loops {
            for edge in &mut hyper_loop.edges {
                edge.edge = map_id(edges, edge.edge, "self loop edge")?;
            }
            if let Some(labels) = &mut hyper_loop.labels {
                for label in &mut labels.label_refs {
                    label.edge = map_id(edges, label.edge, "self loop label")?;
                }
            }
        }
    }
    remap_graph_references(graph, graph.id, nodes, edges)
}

/// Maps the references that the graphs inside the component's nodes hold to the component's
/// graph: the origin of an external port dummy, and the cross-hierarchy edges that name an edge
/// of it. `graph` is the component, and the graphs it holds are in `graphs`.
fn remap_nested_references<N: Slot<NodeId>, E: Slot<EdgeId>>(
    graphs: &mut GraphArena,
    graph: &LGraph,
    nodes: &IdVec<NodeId, N>,
    edges: &IdVec<EdgeId, E>,
) -> Result<(), ComponentError> {
    let nested = graph
        .layerless_nodes
        .iter()
        .filter_map(|node| node.nested_graph)
        .flat_map(|nested| graphs.preorder(nested))
        .collect::<Vec<_>>();
    for id in nested {
        remap_graph_references(&mut graphs[id], graph.id, nodes, edges)?;
    }
    Ok(())
}

/// Maps the references in `current` that name the graph `graph_id`.
fn remap_graph_references<N: Slot<NodeId>, E: Slot<EdgeId>>(
    current: &mut LGraph,
    graph_id: GraphId,
    nodes: &IdVec<NodeId, N>,
    edges: &IdVec<EdgeId, E>,
) -> Result<(), ComponentError> {
    for node in &mut current.layerless_nodes {
        if let Some(origin) = &mut node.origin_port
            && origin.graph_id == graph_id
        {
            map_port(&mut origin.port, nodes)?;
        }
        for port in node.ports.slots_mut() {
            if let Some(dummy) = &mut port.port_dummy
                && dummy.graph_id == graph_id
            {
                dummy.node = map_id(nodes, dummy.node, "graph port dummy")?;
            }
        }
    }
    for edge in &mut current.cross_hierarchy_edges {
        if edge.graph_id == graph_id {
            edge.edge = map_id(edges, edge.edge, "hierarchy edge")?;
        }
    }
    Ok(())
}

fn offset_graph(graph: &mut LGraph, delta: LPoint) {
    for node in &mut graph.layerless_nodes {
        node.position.x += delta.x;
        node.position.y += delta.y;
        for port in &node.ports {
            for &edge in &port.outgoing_edges {
                let edge = &mut graph.edges[edge];
                for point in &mut edge.bend_points {
                    point.x += delta.x;
                    point.y += delta.y;
                }
                for point in &mut edge.junction_points {
                    point.x += delta.x;
                    point.y += delta.y;
                }
                for label in &mut edge.labels {
                    label.position.x += delta.x;
                    label.position.y += delta.y;
                }
            }
        }
    }
}

/// Refuses a spacing, an aspect ratio or a component size that no placement can use.
fn check_geometry(
    components: &[Component],
    spacing: f64,
    aspect_ratio: f64,
) -> Result<(), ComponentError> {
    let bad_number = |value: f64| !value.is_finite() || value < 0.0;
    if bad_number(spacing)
        || bad_number(aspect_ratio)
        || components.iter().any(|component| {
            bad_number(component.graph.size.width) || bad_number(component.graph.size.height)
        })
    {
        return Err(ComponentError::InvalidGeometry);
    }
    Ok(())
}

/// Places completed components, then restores the caller's original dense indices. Java
/// retains object identity while moving nodes; stable original indices are its Rust equivalent.
pub(crate) fn combine(
    graphs: &mut GraphArena,
    mut components: Vec<Component>,
) -> PipelineResult<()> {
    let target = graphs.root();
    let original_nodes = target.layerless_nodes.len();
    let original_edges = target.edges.len();
    let external_ports = target.graph_properties.external_ports;
    let spacing = target.options.spacing.component_component;
    let aspect_ratio = target.options.aspect_ratio;
    let size = match components.len() {
        // ComponentsProcessor.split picks the placer: with external ports, the component-group
        // placer, even for one component. That placer sizes the graph from the spacing.
        _ if external_ports => {
            check_geometry(&components, spacing, aspect_ratio)?;
            let size = group::place(&mut components, spacing, aspect_ratio);
            // AbstractGraphPlacer.moveGraph adds the graph's own offset last.
            for component in &mut components {
                let offset = component.graph.offset;
                offset_graph(&mut component.graph, offset);
                component.graph.offset = LPoint::default();
            }
            if !size.width.is_finite() || !size.height.is_finite() {
                return Err(ComponentError::InvalidGeometry.into());
            }
            size
        }
        0 => LSize::default(),
        1 => {
            let component = &mut components[0].graph;
            let offset = component.offset;
            offset_graph(component, offset);
            component.offset = LPoint::default();
            component.size
        }
        _ => {
            components.sort_by(|left, right| {
                right.priority.cmp(&left.priority).then_with(|| {
                    let area = |g: &LGraph| g.size.width * g.size.height;
                    area(&left.graph).total_cmp(&area(&right.graph))
                })
            });
            check_geometry(&components, spacing, aspect_ratio)?;
            let mut area = 0.0;
            let mut row_width = 0.0f64;
            for component in &components {
                let size = component.graph.size;
                area += size.width * size.height;
                row_width = row_width.max(size.width);
            }
            row_width = row_width.max(f64::from(area.sqrt() as f32) * aspect_ratio);
            if !area.is_finite() || !row_width.is_finite() {
                return Err(ComponentError::InvalidGeometry.into());
            }
            let (mut x, mut y, mut highest, mut broadest): (f64, f64, f64, f64) =
                (0.0, 0.0, 0.0, spacing);
            for component in &mut components {
                let graph = &mut component.graph;
                if x + graph.size.width > row_width {
                    x = 0.0;
                    y += highest + spacing;
                    highest = 0.0;
                }
                offset_graph(
                    graph,
                    LPoint {
                        x: x + graph.offset.x,
                        y: y + graph.offset.y,
                    },
                );
                graph.offset = LPoint::default();
                broadest = broadest.max(x + graph.size.width);
                highest = highest.max(graph.size.height);
                x += graph.size.width + spacing;
            }
            LSize {
                width: broadest,
                height: y + highest,
            }
        }
    };
    let node_count = components
        .iter()
        .map(|component| component.graph.layerless_nodes.len())
        .sum::<usize>();
    let edge_count = components
        .iter()
        .map(|component| component.graph.edges.len())
        .sum::<usize>();
    let mut nodes = IdVec::<NodeId, _>::from_iter(std::iter::repeat_with(|| None).take(node_count));
    let mut edges = IdVec::<EdgeId, _>::from_iter(std::iter::repeat_with(|| None).take(edge_count));
    let mut next_node = original_nodes;
    let mut next_edge = original_edges;
    let mut hidden = Vec::new();
    let mut replaced = Vec::new();
    let mut self_loops = Vec::new();
    let mut cyclic = false;
    for mut component in components {
        let mut node_map = IdVec::<NodeId, _>::from(component.original_nodes);
        let added_nodes = component.graph.layerless_nodes.len() - node_map.len();
        node_map.extend((next_node..next_node + added_nodes).map(NodeId::new));
        next_node += added_nodes;
        let mut edge_map = IdVec::<EdgeId, _>::from(component.original_edges);
        let added_edges = component.graph.edges.len() - edge_map.len();
        edge_map.extend((next_edge..next_edge + added_edges).map(EdgeId::new));
        next_edge += added_edges;
        component.graph.clear_layers();
        remap_references(&mut component.graph, &node_map, &edge_map)?;
        remap_nested_references(graphs, &component.graph, &node_map, &edge_map)?;
        for (index, node) in node_map.into_iter().zip(component.graph.layerless_nodes) {
            nodes[index] = Some(node);
        }
        for (index, edge) in edge_map.into_iter().zip(component.graph.edges) {
            edges[index] = Some(edge);
        }
        hidden.extend(component.graph.hidden_nodes);
        replaced.extend(component.graph.replaced_external_port_dummies);
        self_loops.extend(component.graph.self_loop_holders);
        cyclic |= component.graph.cyclic;
    }
    let layerless_nodes = nodes
        .into_iter()
        .enumerate()
        .map(|(index, node)| {
            node.ok_or(ComponentError::InvalidReference {
                kind: "combined node",
                index,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let edges = edges
        .into_iter()
        .enumerate()
        .map(|(index, edge)| {
            edge.ok_or(ComponentError::InvalidReference {
                kind: "combined edge",
                index,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let target = graphs.root_mut();
    target.layerless_nodes = layerless_nodes.into();
    target.edges = edges.into();
    target.layers.clear();
    target.hidden_nodes = hidden;
    target.replaced_external_port_dummies = replaced;
    target.self_loop_holders = self_loops;
    target.cyclic = cyclic;
    target.size = size;
    // Keep the target's minimum constraint. Source copyProperties merges its property map;
    // the missing minimum on a component does not remove the target's original property.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::PortId;
    use crate::graph::{GraphId, GraphNodeRef, LNode, LPoint, LayeredEdge, PortSide, PortType};
    use crate::options::LayeredOptions;

    // Mutation: leave the `origin_port` of a dummy's port out of `remap_references`. The second
    // component keeps the node index it had in the whole graph, and that index is past its end.
    #[test]
    fn the_port_a_north_dummy_was_made_for_moves_with_its_component() {
        let mut graph = LGraph::new("g", LayeredOptions::default());
        // Node 0 is a component of its own. Nodes 1 and 2 are the second component: node 1 has a
        // north port, and node 2 is the dummy made for it.
        graph
            .layerless_nodes
            .push(LNode::new("alone", 10.0, 10.0, None));
        graph
            .layerless_nodes
            .push(LNode::new("a", 10.0, 10.0, None));
        let mut dummy = LNode::new("dummy", 0.0, 0.0, None);
        dummy.kind = LNodeKind::NorthSouthPort;
        graph.layerless_nodes.push(dummy);
        let north = graph
            .add_port(
                NodeId::new(1),
                PortType::Input,
                PortSide::North,
                LPoint::default(),
            )
            .unwrap();
        let west = graph
            .add_port(
                NodeId::new(1),
                PortType::Input,
                PortSide::West,
                LPoint::default(),
            )
            .unwrap();
        let out = graph
            .add_port(
                NodeId::new(2),
                PortType::Output,
                PortSide::East,
                LPoint::default(),
            )
            .unwrap();
        graph.layerless_nodes[NodeId::new(2)].ports[out.port].origin_port = Some(north);
        graph.layerless_nodes[NodeId::new(1)].ports[north.port].port_dummy = Some(GraphNodeRef {
            graph_id: GraphId::ROOT,
            node: NodeId::new(2),
        });
        graph
            .add_edge(LayeredEdge {
                id: "e".to_string(),
                source: out,
                target: west,
                source_node_id: String::new(),
                target_node_id: String::new(),
                labels: Vec::new(),
                minlen: 1,
                reversed: false,
                bend_points: Vec::new(),
                junction_points: Vec::new(),
                model_order: None,
                priority_direction: 0,
                priority_shortness: 0,
                priority_straightness: 0,
                thickness: 1.0,
                original_opposite_port: None,
                compound_segment: None,
            })
            .unwrap();

        let components = split(&mut GraphArena::new(graph)).unwrap().unwrap();

        assert_eq!(components.len(), 2);
        let second = &components[1].graph;
        assert_eq!(second.layerless_nodes.len(), 2);
        // The dummy is node 1 here, and the node it was made for is node 0.
        assert_eq!(
            second.layerless_nodes[NodeId::new(1)].ports[PortId::new(0)].origin_port,
            Some(PortRef {
                node: NodeId::new(0),
                port: PortId::new(0)
            })
        );
        assert_eq!(
            second.layerless_nodes[NodeId::new(0)].ports[PortId::new(0)]
                .port_dummy
                .as_ref()
                .map(|dummy| dummy.node),
            Some(NodeId::new(1))
        );
    }

    fn edge_between(id: &str, source: PortRef, target: PortRef) -> LayeredEdge {
        LayeredEdge {
            id: id.to_string(),
            source,
            target,
            source_node_id: String::new(),
            target_node_id: String::new(),
            labels: Vec::new(),
            minlen: 1,
            reversed: false,
            bend_points: Vec::new(),
            junction_points: Vec::new(),
            model_order: None,
            priority_direction: 0,
            priority_shortness: 0,
            priority_straightness: 0,
            thickness: 1.0,
            original_opposite_port: None,
            compound_segment: None,
        }
    }

    // Mutation: leave out the `remap_nested_references` call in `split`. The nested graph then
    // keeps the indices of the whole graph while its component has smaller ones. Leave out the
    // call in `combine`: the nested graph keeps the component's indices after the nodes are back
    // where they were.
    #[test]
    fn a_graph_inside_a_node_follows_the_node_through_split_and_combine() {
        use crate::graph::{CompoundEdgeSegment, CrossHierarchyEdge, GraphPortRef};

        let mut graph = LGraph::new("g", LayeredOptions::default());
        for id in ["a", "b", "holder", "c"] {
            graph.layerless_nodes.push(LNode::new(id, 10.0, 10.0, None));
        }
        // The first component is nodes 0 and 1 with edge 0. The second is nodes 2 and 3 with
        // edge 1, so everything in it moves down: node 2 to 0, node 3 to 1, edge 1 to 0.
        let a_out = graph
            .add_port(
                crate::graph::NodeId::new(0),
                PortType::Output,
                PortSide::East,
                LPoint::default(),
            )
            .unwrap();
        let b_in = graph
            .add_port(
                crate::graph::NodeId::new(1),
                PortType::Input,
                PortSide::West,
                LPoint::default(),
            )
            .unwrap();
        graph.add_edge(edge_between("e0", a_out, b_in)).unwrap();
        let holder_out = graph
            .add_port(
                crate::graph::NodeId::new(2),
                PortType::Output,
                PortSide::East,
                LPoint::default(),
            )
            .unwrap();
        let c_in = graph
            .add_port(
                crate::graph::NodeId::new(3),
                PortType::Input,
                PortSide::West,
                LPoint::default(),
            )
            .unwrap();
        graph
            .add_edge(edge_between("e1", holder_out, c_in))
            .unwrap();

        let mut inside = LGraph::new("inside", LayeredOptions::default());
        inside
            .layerless_nodes
            .push(LNode::new("dummy", 1.0, 1.0, None));
        let port = inside
            .add_port(
                crate::graph::NodeId::new(0),
                PortType::Output,
                PortSide::East,
                LPoint::default(),
            )
            .unwrap();
        // The dummy stands for the port of `holder`, and holds the dummy made for `c`'s port.
        inside.layerless_nodes[NodeId::new(0)].origin_port = Some(GraphPortRef {
            graph_id: GraphId::ROOT,
            port: holder_out,
        });
        inside.layerless_nodes[NodeId::new(0)].ports[port.port].port_dummy = Some(GraphNodeRef {
            graph_id: GraphId::ROOT,
            node: crate::graph::NodeId::new(3),
        });
        let mut graphs = GraphArena::new(graph);
        let inside_id = graphs.nest(GraphId::ROOT, crate::graph::NodeId::new(2), inside);
        let record = |graph_id, edge| CrossHierarchyEdge {
            original_edge_id: "e1".to_string(),
            original_model_order: None,
            graph_id,
            edge,
            segment: CompoundEdgeSegment::Output { depth: 0 },
        };
        graphs[inside_id].cross_hierarchy_edges = vec![
            // A record of the root's edge 1, and one of the nested graph's own edge 5, which
            // belongs to no component.
            record(GraphId::ROOT, crate::graph::EdgeId::new(1)),
            record(inside_id, crate::graph::EdgeId::new(5)),
        ];
        let before = graphs[inside_id].clone();

        let components = split(&mut graphs).unwrap().unwrap();

        assert_eq!(components.len(), 2);
        let between = &graphs[inside_id];
        assert_eq!(
            between.layerless_nodes[NodeId::new(0)]
                .origin_port
                .map(|o| o.port.node),
            Some(crate::graph::NodeId::new(0)),
            "the origin is node 0 of its component"
        );
        assert_eq!(
            between.layerless_nodes[NodeId::new(0)].ports[port.port]
                .port_dummy
                .map(|dummy| dummy.node),
            Some(crate::graph::NodeId::new(1))
        );
        assert_eq!(
            between
                .cross_hierarchy_edges
                .iter()
                .map(|record| (record.graph_id, record.edge))
                .collect::<Vec<_>>(),
            [
                (GraphId::ROOT, crate::graph::EdgeId::new(0)),
                (inside_id, crate::graph::EdgeId::new(5))
            ]
        );
        // The root still holds the node, so the parent link keeps its index.
        assert_eq!(between.parent, before.parent);

        combine(&mut graphs, components).unwrap();

        assert_eq!(graphs[inside_id], before);
    }
}
