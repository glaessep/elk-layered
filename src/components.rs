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
    EdgeId, GraphArena, GraphId, GraphParent, IdVec, LGraph, LLabel, LPoint, LSize, NodeId,
    NodeTables, PortRef, VecId,
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
    /// `drop_nodes` met a reference it cannot renumber. This is a bug in the processors, never
    /// caller input.
    #[error(
        "dropping the temporary dummies after the hierarchical port routing left a {kind} \
         reference {index} that names no node or edge"
    )]
    DroppedReference { kind: &'static str, index: usize },
}

pub(crate) struct Component {
    pub(crate) graph: LGraph,
    /// For each node of the component graph, the id it has in the whole graph.
    original_nodes: IdVec<NodeId, NodeId>,
    /// For each edge of the component graph, the id it has in the whole graph.
    original_edges: IdVec<EdgeId, EdgeId>,
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
                for &edge in port.outgoing_edges().iter().rev() {
                    let edge = graph
                        .edges
                        .get(edge)
                        .ok_or(ComponentError::InvalidReference {
                            kind: "edge",
                            index: edge.index(),
                        })?;
                    stack.push(edge.target().node);
                }
                for &edge in port.incoming_edges().iter().rev() {
                    let edge = graph
                        .edges
                        .get(edge)
                        .ok_or(ComponentError::InvalidReference {
                            kind: "edge",
                            index: edge.index(),
                        })?;
                    stack.push(edge.source().node);
                }
            }
        }
        groups.push(nodes);
    }
    let mut components = groups
        .into_iter()
        .map(|original_nodes| {
            let original_nodes = IdVec::<NodeId, NodeId>::from(original_nodes);
            let mut component = graph.component_shell();
            component.layerless_nodes = original_nodes
                .iter()
                .map(|&node| graph.layerless_nodes[node].clone())
                .collect();
            component.tables = graph.tables.subset(&original_nodes);
            // The importer reads no node priority yet, so every component has priority 0. A
            // port of ELK's node priority adds the sum over the component's nodes here.
            let priority = 0i32;
            // ComponentsProcessor.dfs: the side of every external port dummy of the component.
            let ext_port_sides = component
                .layerless_nodes
                .iter()
                .filter_map(|node| node.external_port())
                .fold(group::SIDES_NONE, |sides, external| {
                    sides | group::side_bit(external.side)
                });
            Component {
                graph: component,
                original_nodes,
                original_edges: IdVec::new(),
                priority,
                ext_port_sides,
            }
        })
        .collect::<Vec<_>>();
    for (index, edge) in graph.edges_with_ids() {
        let owner = *owners
            .get(edge.source().node)
            .ok_or(ComponentError::InvalidReference {
                kind: "source node",
                index: edge.source().node.index(),
            })?;
        if owners.get(edge.target().node) != Some(&owner) {
            return Err(ComponentError::InvalidReference {
                kind: "target node",
                index: edge.target().node.index(),
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
        for (local, &original) in component.original_nodes.iter_with_ids() {
            node_map[original] = Some(local);
        }
        for (local, &original) in component.original_edges.iter_with_ids() {
            edge_map[original] = Some(local);
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
    graph
        .tables
        .map_node_ids(|node, kind| map_id(nodes, node, kind))?;
    for node in &mut graph.layerless_nodes {
        if let Some(external) = node.external_port_mut() {
            map_optional(&mut external.replaced_dummy, nodes, "replaced dummy")?;
        }
        if let Some(ends) = node.long_edge_ends_mut() {
            for port in [&mut ends.source, &mut ends.target].into_iter().flatten() {
                map_port(port, nodes)?;
            }
        }
        if let Some(long_edge) = node.long_edge_mut() {
            map_optional(&mut long_edge.origin_edge, edges, "origin edge")?;
        }
        if let Some(label) = node.label_dummy_mut() {
            map_optional(&mut label.origin_edge, edges, "origin edge")?;
            map_labels(&mut label.represented_labels, edges)?;
        }
        map_labels(&mut node.labels, edges)?;
        for port in node.ports.slots_mut() {
            port.map_ids(
                |node| map_id(nodes, node, "port owner"),
                |edge| map_id(edges, edge, "incoming edge"),
                |edge| map_id(edges, edge, "outgoing edge"),
            )?;
            map_optional(&mut port.long_edge_target_node, nodes, "long edge target")?;
            if let Some(origin) = &mut port.origin_port {
                map_port(origin, nodes)?;
            }
            map_labels(&mut port.labels, edges)?;
        }
    }
    for edge in &mut graph.edges {
        edge.map_ports(|port| map_port(port, nodes))?;
        if let Some(port) = &mut edge.original_opposite_port {
            map_port(port, nodes)?;
        }
        map_labels(&mut edge.labels, edges)?;
    }
    graph.map_layer_nodes(|node| map_id(nodes, node, "layer node"))?;
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

/// Whether the root graph of the run stands in for a component of a split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComponentSplit {
    /// Between `split` and the end of `combine`.
    Active,
    /// Every other run: a flat run without components, and a compound run.
    Inactive,
}

/// Takes the nodes `dropped` out of the graph `id`, together with every edge that ends at one of
/// them. The other nodes and edges keep their order and close the gaps, and every reference to
/// them moves along: in the graph, in the graphs inside its nodes, and in the graphs above it.
///
/// Used for the temporary replacements of north and south external port dummies. ELK makes them
/// with `new LNode(graph)`, which never adds a node to `getLayerlessNodes()`, so they are gone
/// once they leave their layer. A dropped node must be in no layer, and no node that stays may
/// name it: such a reference is an error.
///
/// A dropped node must hold no graph.
///
/// The graph inside a kept node names it in `GraphParent.node`. Outside a component split, this
/// function renumbers that field with the node. During a split (`split` is `Active`), the field
/// keeps the node's index in the whole graph on purpose, so this function cannot map it. A node
/// that holds a graph must then come before every dropped node, so that it keeps its id.
pub(crate) fn drop_nodes(
    graphs: &mut GraphArena,
    id: GraphId,
    dropped: &[NodeId],
    split: ComponentSplit,
) -> Result<(), ComponentError> {
    // The renumbering shares its helpers with the component split, which report
    // `InvalidReference`. Here no component is involved, so the error says what really failed.
    drop_nodes_renumbered(graphs, id, dropped, split).map_err(|error| match error {
        ComponentError::InvalidReference { kind, index } => {
            ComponentError::DroppedReference { kind, index }
        }
        other => other,
    })
}

fn drop_nodes_renumbered(
    graphs: &mut GraphArena,
    id: GraphId,
    dropped: &[NodeId],
    split: ComponentSplit,
) -> Result<(), ComponentError> {
    let Some(&first) = dropped.iter().min() else {
        return Ok(());
    };
    let graph = &graphs[id];
    let mut keep_node = IdVec::<NodeId, bool>::filled(graph.layerless_nodes.len(), true);
    for &node in dropped {
        *keep_node
            .get_mut(node)
            .ok_or(ComponentError::InvalidReference {
                kind: "dropped node",
                index: node.to_index(),
            })? = false;
        // A dropped node that holds a graph would leave that graph without a node.
        if graph.layerless_nodes[node].nested_graph.is_some() {
            return Err(ComponentError::InvalidReference {
                kind: "dropped graph owner",
                index: node.to_index(),
            });
        }
    }
    if split == ComponentSplit::Active
        && let Some((owner, _)) = graph
            .nodes_with_ids()
            .find(|(node, data)| data.nested_graph.is_some() && *node >= first)
    {
        return Err(ComponentError::InvalidReference {
            kind: "graph owner after a dropped node",
            index: owner.to_index(),
        });
    }
    if split == ComponentSplit::Inactive
        && let Some((owner, _)) = graph.nodes_with_ids().find(|(_, data)| {
            data.nested_graph.is_some_and(|nested| {
                graphs[nested]
                    .parent
                    .is_none_or(|parent| parent.graph != id)
            })
        })
    {
        // The check runs before anything changes, like the others, and names the id before the
        // drop.
        return Err(ComponentError::InvalidReference {
            kind: "graph parent",
            index: owner.to_index(),
        });
    }
    let keep_edge = graph
        .edges
        .iter()
        .map(|edge| {
            is_kept(&keep_node, edge.source().node) && is_kept(&keep_node, edge.target().node)
        })
        .collect::<IdVec<EdgeId, bool>>();
    let (node_map, kept_nodes) = renumber(&keep_node);
    let (edge_map, _) = renumber(&keep_edge);

    let graph = &mut graphs[id];
    // A long edge dummy of an inverted port can name a port of a dropped node as its end, and
    // the model order sorting copies that end into `long_edge_target_node` of the ports along
    // the edge. Java keeps these references to a node that is gone, and no step after the
    // routing reads them, so they are cleared here instead of renumbered.
    let is_dropped = |node: NodeId| !is_kept(&keep_node, node);
    for node in graph.layerless_nodes.iter_mut() {
        if let Some(ends) = node.long_edge_ends_mut() {
            for end in [&mut ends.source, &mut ends.target] {
                if end.is_some_and(|port| is_dropped(port.node)) {
                    *end = None;
                }
            }
        }
        for port in node.ports.slots_mut() {
            if port.long_edge_target_node.is_some_and(is_dropped) {
                port.long_edge_target_node = None;
            }
        }
    }
    graph.tables = graph.tables.subset(&kept_nodes);
    let nodes = std::mem::take(&mut graph.layerless_nodes);
    graph.layerless_nodes = nodes
        .into_iter()
        .zip(keep_node.iter())
        .filter_map(|(node, &keep)| keep.then_some(node))
        .collect();
    let edges = std::mem::take(&mut graph.edges);
    graph.edges = edges
        .into_iter()
        .zip(keep_edge.iter())
        .filter_map(|(edge, &keep)| keep.then_some(edge))
        .collect();
    remap_references(graph, &node_map, &edge_map)?;

    let nested = graphs_inside(graphs, &graphs[id]);
    let mut above = Vec::new();
    let mut parent = graphs[id].parent;
    while let Some(GraphParent { graph, .. }) = parent {
        above.push(graph);
        parent = graphs[graph].parent;
    }
    for other in nested.into_iter().chain(above) {
        remap_graph_references(&mut graphs[other], id, &node_map, &edge_map)?;
    }
    if split == ComponentSplit::Inactive {
        // The nodes are in their new places now, so each holder names the id it has now. A node
        // before the first dropped one kept its id. Every holder was checked above.
        let holders = graphs[id]
            .nodes_with_ids()
            .skip(first.to_index())
            .filter_map(|(node, data)| data.nested_graph.map(|nested| (node, nested)))
            .collect::<Vec<_>>();
        for (node, nested) in holders {
            if let Some(parent) = &mut graphs[nested].parent {
                parent.node = node;
            }
        }
    }
    Ok(())
}

/// Whether `node` stays. An id past the end counts as kept, so that the renumbering reports it as
/// an error instead of a panic here.
fn is_kept(keep: &IdVec<NodeId, bool>, node: NodeId) -> bool {
    keep.get(node).copied().unwrap_or(true)
}

/// For a list of keep flags, the new id of each kept entry, and the old id of each new one.
fn renumber<I: VecId>(keep: &IdVec<I, bool>) -> (IdVec<I, Option<I>>, IdVec<I, I>) {
    let mut kept = IdVec::new();
    let map = keep
        .iter_with_ids()
        .map(|(old, &keep)| keep.then(|| kept.push(old)))
        .collect();
    (map, kept)
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
    for id in graphs_inside(graphs, graph) {
        remap_graph_references(&mut graphs[id], graph.id, nodes, edges)?;
    }
    Ok(())
}

/// Every graph inside the nodes of `graph`, at any depth.
fn graphs_inside(graphs: &GraphArena, graph: &LGraph) -> Vec<GraphId> {
    graph
        .layerless_nodes
        .iter()
        .filter_map(|node| node.nested_graph)
        .flat_map(|nested| graphs.preorder(nested))
        .collect()
}

/// Maps the references in `current` that name the graph `graph_id`.
fn remap_graph_references<N: Slot<NodeId>, E: Slot<EdgeId>>(
    current: &mut LGraph,
    graph_id: GraphId,
    nodes: &IdVec<NodeId, N>,
    edges: &IdVec<EdgeId, E>,
) -> Result<(), ComponentError> {
    for node in &mut current.layerless_nodes {
        if let Some(origin) = node
            .external_port_mut()
            .and_then(|external| external.origin_port.as_mut())
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
            for &edge in port.outgoing_edges() {
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
            // ELK casts the square root to `float`, which elkjs leaves a double: see
            // `crate::gwt::float_value`.
            row_width = row_width.max(crate::gwt::float_value(area.sqrt()) * aspect_ratio);
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
    let mut nodes = IdVec::<NodeId, _>::filled(node_count, None);
    let mut edges = IdVec::<EdgeId, _>::filled(edge_count, None);
    let mut next_node = original_nodes;
    let mut next_edge = original_edges;
    let mut hidden = Vec::new();
    let mut replaced = Vec::new();
    let mut self_loops = Vec::new();
    let mut tables = NodeTables::default();
    let mut cyclic = false;
    for mut component in components {
        let mut node_map = component.original_nodes;
        let added_nodes = component.graph.layerless_nodes.len() - node_map.len();
        node_map.extend((next_node..next_node + added_nodes).map(NodeId::new));
        next_node += added_nodes;
        let mut edge_map = component.original_edges;
        let added_edges = component.graph.edges.len() - edge_map.len();
        edge_map.extend((next_edge..next_edge + added_edges).map(EdgeId::new));
        next_edge += added_edges;
        component.graph.clear_layers();
        remap_references(&mut component.graph, &node_map, &edge_map)?;
        remap_nested_references(graphs, &component.graph, &node_map, &edge_map)?;
        tables.absorb(&node_map, component.graph.tables);
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
    target.tables = tables;
    target.layerless_nodes = layerless_nodes.into();
    target.edges = edges.into();
    target.clear_layers();
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
mod tests;
