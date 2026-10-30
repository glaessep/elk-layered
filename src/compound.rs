//! Compound graph preprocessing support.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/compound/CompoundGraphPreprocessor.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/compound/CrossHierarchyEdgeComparator.java

use std::collections::{HashMap, HashSet};

use crate::graph::{
    CompoundEdgeSegment, CrossHierarchyEdge, EdgeId, EdgeLabelPlacement, ExternalPortData,
    GraphArena, GraphId, GraphNodeRef, GraphParent, GraphPortRef, HierarchyEdge, HierarchyStep,
    IdVec, LGraph, LLabel, LNodeKind, LPoint, LPort, LSize, LayeredEdge, NodeId, PortId, PortRef,
    PortSide, PortType, apply_port_index_to_dummy, compute_inside_part, create_external_port_dummy,
    port_side_from_direction,
};
use crate::options::{ElkDirection, PortConstraints, PortLabelPlacement};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingCompoundSegment {
    /// The graph the segment lands in, by its `GraphId`, or `None` for the graph of the scope
    /// being introduced. A segment with a parent boundary lands in the graph inside that parent.
    pub(crate) lands_in: Option<GraphId>,
    pub(crate) source: PendingSegmentEndpoint,
    pub(crate) target: PendingSegmentEndpoint,
    pub(crate) segment: CompoundEdgeSegment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PendingSegmentEndpoint {
    LocalNode {
        node_id: String,
        port_key: String,
    },
    ParentBoundary {
        node_id: String,
        port_key: String,
        port_type: PortType,
        parent_port_type: PortType,
        connects_parent_node: bool,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct ScopedHierarchySegment {
    pub(crate) edge: HierarchyEdge,
    pub(crate) pending: PendingCompoundSegment,
    pub(crate) labels: Vec<LLabel>,
}

/// Build the graph-local segments for a hierarchy-crossing edge.
///
/// This is the current segment-building core of the Rust `CompoundGraphPreprocessor` port. The
/// full ELK algorithm introduces segments through `ExternalPort` records while walking the graph
/// recursively; this helper keeps segment ordering and label-placement semantics in the compound
/// boundary while that recursive port is filled in.
pub(crate) fn source_ported_cross_hierarchy_segments(
    source: &str,
    target: &str,
    source_port_key: &str,
    target_port_key: &str,
    source_path: &[HierarchyStep],
    target_path: &[HierarchyStep],
) -> Vec<PendingCompoundSegment> {
    let shape = cross_hierarchy_shape(source, target, source_path, target_path);
    let common_depth = shape.common_depth;

    let mut segments = Vec::with_capacity(
        source_path.len().saturating_sub(common_depth)
            + target_path.len().saturating_sub(common_depth)
            + usize::from(!shape.source_is_target_ancestor && !shape.target_is_source_ancestor),
    );

    for depth in (common_depth + 1..=source_path.len()).rev() {
        let segment_source = if depth == source_path.len() {
            source.to_string()
        } else {
            source_path[depth].node_id.to_string()
        };
        let connects_parent_node = target == source_path[depth - 1].node_id;
        segments.push(PendingCompoundSegment {
            lands_in: Some(source_path[depth - 1].graph),
            source: PendingSegmentEndpoint::LocalNode {
                node_id: segment_source,
                port_key: source_port_key.to_string(),
            },
            target: PendingSegmentEndpoint::ParentBoundary {
                node_id: source_path[depth - 1].node_id.to_string(),
                port_key: if connects_parent_node {
                    target_port_key.to_string()
                } else {
                    source_port_key.to_string()
                },
                port_type: PortType::Output,
                parent_port_type: if connects_parent_node {
                    PortType::Input
                } else {
                    PortType::Output
                },
                connects_parent_node,
            },
            segment: CompoundEdgeSegment::Output { depth },
        });
    }

    if !shape.source_is_target_ancestor && !shape.target_is_source_ancestor {
        let segment_source = if source_path.len() > common_depth {
            source_path[common_depth].node_id.to_string()
        } else {
            source.to_string()
        };
        let segment_target = if target_path.len() > common_depth {
            target_path[common_depth].node_id.to_string()
        } else {
            target.to_string()
        };
        let segment = if source_path.len() > common_depth {
            CompoundEdgeSegment::Output {
                depth: common_depth,
            }
        } else {
            CompoundEdgeSegment::Input {
                depth: common_depth,
            }
        };
        segments.push(PendingCompoundSegment {
            lands_in: common_depth
                .checked_sub(1)
                .map(|parent_depth| source_path[parent_depth].graph),
            source: PendingSegmentEndpoint::LocalNode {
                node_id: segment_source,
                port_key: source_port_key.to_string(),
            },
            target: PendingSegmentEndpoint::LocalNode {
                node_id: segment_target,
                port_key: target_port_key.to_string(),
            },
            segment,
        });
    }

    for depth in common_depth + 1..=target_path.len() {
        let segment_target = if depth == target_path.len() {
            target.to_string()
        } else {
            target_path[depth].node_id.to_string()
        };
        let connects_parent_node = source == target_path[depth - 1].node_id;
        segments.push(PendingCompoundSegment {
            lands_in: Some(target_path[depth - 1].graph),
            source: PendingSegmentEndpoint::ParentBoundary {
                node_id: target_path[depth - 1].node_id.to_string(),
                port_key: if connects_parent_node {
                    source_port_key.to_string()
                } else {
                    target_port_key.to_string()
                },
                port_type: PortType::Input,
                parent_port_type: if connects_parent_node {
                    PortType::Output
                } else {
                    PortType::Input
                },
                connects_parent_node,
            },
            target: PendingSegmentEndpoint::LocalNode {
                node_id: segment_target,
                port_key: target_port_key.to_string(),
            },
            segment: CompoundEdgeSegment::Input { depth },
        });
    }

    segments
}

#[derive(Debug, Clone, Copy)]
struct CrossHierarchyShape {
    common_depth: usize,
    source_is_target_ancestor: bool,
    target_is_source_ancestor: bool,
}

fn cross_hierarchy_shape(
    source: &str,
    target: &str,
    source_path: &[HierarchyStep],
    target_path: &[HierarchyStep],
) -> CrossHierarchyShape {
    let common_depth = common_graph_depth(source_path, target_path);
    CrossHierarchyShape {
        common_depth,
        source_is_target_ancestor: target_path.len() > common_depth
            && source == target_path[common_depth].node_id,
        target_is_source_ancestor: source_path.len() > common_depth
            && target == source_path[common_depth].node_id,
    }
}

pub(crate) fn compound_label_segment_index(
    segments: &[PendingCompoundSegment],
    placement: EdgeLabelPlacement,
) -> usize {
    match placement {
        EdgeLabelPlacement::Tail => stable_extreme_segment_index(segments, false),
        EdgeLabelPlacement::Head => stable_extreme_segment_index(segments, true),
        EdgeLabelPlacement::Center => center_segment_index(segments),
    }
}

fn stable_extreme_segment_index(segments: &[PendingCompoundSegment], maximum: bool) -> usize {
    let mut selected = None;
    for (index, candidate) in segments.iter().enumerate() {
        let Some((_, current)) = selected else {
            selected = Some((index, candidate.segment));
            continue;
        };
        let ordering = compare_compound_segments(candidate.segment, current);
        let replace = if maximum {
            ordering != std::cmp::Ordering::Less
        } else {
            ordering == std::cmp::Ordering::Less
        };
        if replace {
            selected = Some((index, candidate.segment));
        }
    }
    selected.map(|(index, _)| index).unwrap_or(0)
}

fn center_segment_index(segments: &[PendingCompoundSegment]) -> usize {
    let mut shallowest_output = None;
    let mut shallowest_input = None;
    for (index, segment) in segments.iter().enumerate() {
        match segment.segment {
            CompoundEdgeSegment::Output { depth } => {
                if shallowest_output.is_none_or(|(_, current)| depth <= current) {
                    shallowest_output = Some((index, depth));
                }
            }
            CompoundEdgeSegment::Input { depth } => {
                if shallowest_input.is_none_or(|(_, current)| depth < current) {
                    shallowest_input = Some((index, depth));
                }
            }
        }
    }
    shallowest_output
        .or(shallowest_input)
        .map(|(index, _)| index)
        .unwrap_or(0)
}

/// Mirror `CompoundGraphPreprocessor#setSidesOfPortsToSidesOfDummyNodes` for an exported
/// compound port / external-port dummy pair.
///
/// This keeps the ELK `ORIGIN`, `PORT_DUMMY`, and `INSIDE_CONNECTIONS` semantics represented in
/// the graph model for segment endpoints introduced by compound preprocessing.
pub(crate) fn link_external_port_dummy(
    parent_graph: &mut LGraph,
    parent_port: PortRef,
    dummy_side: PortSide,
    dummy_graph_id: GraphId,
    dummy_node: NodeId,
    dummy_border_offset: Option<f64>,
) {
    let Some(parent_node) = parent_graph.layerless_nodes.get_mut(parent_port.node) else {
        return;
    };
    let Some(parent_port_data) = parent_node.ports.get_mut(parent_port.port) else {
        return;
    };

    parent_port_data.port_dummy = Some(GraphNodeRef {
        graph_id: dummy_graph_id,
        node: dummy_node,
    });
    parent_port_data.inside_connections = true;
    parent_port_data.set_side(dummy_side);
    if parent_port_data.border_offset.is_none()
        && let Some(border_offset) = dummy_border_offset
    {
        parent_port_data.border_offset = Some(border_offset);
    }
    parent_node.port_constraints = PortConstraints::FixedSide;
    parent_graph.graph_properties.non_free_ports = true;
}

pub(crate) fn set_external_dummy_origin(
    dummy_graph: &mut LGraph,
    dummy_node: NodeId,
    origin_graph_id: GraphId,
    origin_port: PortRef,
) {
    // Only an external port dummy has an origin port.
    let Some(external) = dummy_graph
        .layerless_nodes
        .get_mut(dummy_node)
        .and_then(|dummy| dummy.external_port_mut())
    else {
        return;
    };
    external.origin_port = Some(GraphPortRef {
        graph_id: origin_graph_id,
        port: origin_port,
    });
}

pub(crate) fn record_cross_hierarchy_edge_segment(
    graph: &mut LGraph,
    original_edge_id: impl Into<String>,
    original_model_order: Option<usize>,
    edge: EdgeId,
    segment: CompoundEdgeSegment,
) {
    graph.cross_hierarchy_edges.push(CrossHierarchyEdge {
        original_edge_id: original_edge_id.into(),
        original_model_order,
        graph_id: graph.id,
        edge,
        segment,
    });
}

/// Compatibility entry point for the source-backed compound preprocessor boundary.
///
/// ELK runs `CompoundGraphPreprocessor` after import and before recursive layout. The Rust port now
/// keeps hierarchy-crossing input edges as `HierarchyEdge` records during import and introduces
/// hierarchy-local layout segments here. The second pass still accepts already segmented edges as a
/// migration bridge for tests and later postprocessor work.
pub fn preprocess_source_ported_compound_graph(graphs: &mut GraphArena) {
    preprocess_source_ported_compound_graph_observed(graphs, &mut NoopCompoundPreprocessObserver);
}

trait CompoundPreprocessObserver {
    fn visit_graph_edge_for_endpoint_plan(&mut self) {}

    fn visit_existing_cross_hierarchy_edge(&mut self) {}

    fn visit_endpoint_nested_node(&mut self) {}

    fn visit_endpoint_incident_edge(&mut self) {}

    fn process_segment_edge(&mut self) {}

    fn build_parent_port_index(&mut self) {}
}

struct NoopCompoundPreprocessObserver;

impl CompoundPreprocessObserver for NoopCompoundPreprocessObserver {}

#[cfg(test)]
#[derive(Debug, Default, PartialEq, Eq)]
struct CompoundPreprocessWork {
    graph_edges_planned: usize,
    existing_cross_hierarchy_edges_indexed: usize,
    endpoint_nested_nodes_indexed: usize,
    endpoint_incident_edges_indexed: usize,
    segment_edges_processed: usize,
    parent_port_indexes_built: usize,
}

#[cfg(test)]
impl CompoundPreprocessObserver for CompoundPreprocessWork {
    fn visit_graph_edge_for_endpoint_plan(&mut self) {
        self.graph_edges_planned += 1;
    }

    fn visit_existing_cross_hierarchy_edge(&mut self) {
        self.existing_cross_hierarchy_edges_indexed += 1;
    }

    fn visit_endpoint_nested_node(&mut self) {
        self.endpoint_nested_nodes_indexed += 1;
    }

    fn visit_endpoint_incident_edge(&mut self) {
        self.endpoint_incident_edges_indexed += 1;
    }

    fn process_segment_edge(&mut self) {
        self.segment_edges_processed += 1;
    }

    fn build_parent_port_index(&mut self) {
        self.parent_port_indexes_built += 1;
    }
}

fn preprocess_source_ported_compound_graph_observed(
    graphs: &mut GraphArena,
    observer: &mut impl CompoundPreprocessObserver,
) {
    // The segments replace the hierarchy edges, so the net flows must be read before they are
    // introduced. Without a hierarchy edge nothing changes, so a graph that ends up with no
    // external ports never reads them.
    let has_hierarchy_edges = hierarchy_has_pending_edges(graphs);
    let flows_before_segments = has_hierarchy_edges.then(|| original_net_flows(graphs));
    introduce_source_ported_hierarchy_edge_segments(graphs);
    if !hierarchy_has_external_ports(graphs) {
        let result = graphs.try_for_each_graph_mut(|graph| {
            prepare_compound_endpoint_metadata(graph, observer);
            Ok::<(), std::convert::Infallible>(())
        });
        result.expect("compound endpoint preprocessing is infallible");
        return;
    }
    let original_net_flows = flows_before_segments.unwrap_or_else(|| original_net_flows(graphs));

    // The root first, then the graphs below it level by level.
    let order = graphs.breadth_first(GraphId::ROOT);
    // `transformHierarchyEdges` reads the port constraints of each compound node before
    // `setSidesOfPortsToSidesOfDummyNodes` fixes them, so keep them.
    let mut original_constraints = vec![IdVec::new(); graphs.len()];
    for &graph in &order {
        original_constraints[graph.index()] = port_constraints_of(&graphs[graph]);
    }
    let mut endpoint_plans = (0..graphs.len()).map(|_| None).collect::<Vec<_>>();
    for &graph in &order {
        endpoint_plans[graph.index()] = Some(prepare_compound_endpoint_metadata(
            &mut graphs[graph],
            observer,
        ));
    }
    link_compound_external_dummy_metadata(graphs, &endpoint_plans, observer);
    ensure_nested_external_dummies_for_parent_ports(
        graphs,
        &order,
        &original_constraints,
        &original_net_flows,
    );
}

/// A port by the id of its node and its own id. `named` says whether the caller named the port:
/// a made-up port can have the id of a named port of the same node, such as the implicit port
/// `G:2` beside a named port `G:2`, and the two must not share a net flow.
type OriginalNetFlowKey = (String, String, bool);

/// The net flow of every port, as `LPort.getNetFlow` reads it while `transformHierarchyEdges`
/// still sees the edges as the importer made them: incoming minus outgoing edges, the ones that
/// cross a level included. Edges that are split into segments no longer show up at the port in
/// the same way.
fn original_net_flows(graphs: &GraphArena) -> HashMap<OriginalNetFlowKey, isize> {
    let mut flows = HashMap::<OriginalNetFlowKey, isize>::new();
    // The sums do not depend on the order, so slot order will do.
    for graph in graphs.iter() {
        for node in &graph.layerless_nodes {
            for port in &node.ports {
                *flows
                    .entry((node.id.clone(), port.id.clone(), port.named))
                    .or_default() += port.net_flow();
            }
        }
        for edge in &graph.hierarchy_edges {
            *flows
                .entry((
                    edge.source_node_id.clone(),
                    edge.source_port_key.clone(),
                    edge.source_port_named,
                ))
                .or_default() -= 1;
            *flows
                .entry((
                    edge.target_node_id.clone(),
                    edge.target_port_key.clone(),
                    edge.target_port_named,
                ))
                .or_default() += 1;
        }
    }
    flows
}

fn port_constraints_of(graph: &LGraph) -> IdVec<NodeId, PortConstraints> {
    graph
        .layerless_nodes
        .iter()
        .map(|node| node.port_constraints)
        .collect()
}

fn introduce_source_ported_hierarchy_edge_segments(graphs: &mut GraphArena) {
    if !hierarchy_has_pending_edges(graphs) {
        return;
    }
    // A hierarchy edge owns a traversal of its relative graph scope. Skipping empty owners is
    // essential on deep compound chains: otherwise every graph would rescan its remaining
    // subtree even though the official preprocessor has no segments to introduce.
    for graph in graphs.breadth_first(GraphId::ROOT) {
        if graphs[graph].hierarchy_edges.is_empty() {
            continue;
        }
        let hierarchy_edges = std::mem::take(&mut graphs[graph].hierarchy_edges);
        let mut segments = Vec::new();
        for edge in hierarchy_edges {
            segments.extend(materialize_hierarchy_edge_segments(&edge));
        }
        introduce_source_ported_scoped_edge_segments(graphs, graph, segments);
    }
}

fn hierarchy_has_pending_edges(graphs: &GraphArena) -> bool {
    graphs.iter().any(|graph| !graph.hierarchy_edges.is_empty())
}

fn hierarchy_has_external_ports(graphs: &GraphArena) -> bool {
    graphs
        .iter()
        .any(|graph| graph.graph_properties.external_ports)
}

fn materialize_hierarchy_edge_segments(edge: &HierarchyEdge) -> Vec<ScopedHierarchySegment> {
    let segments = source_ported_cross_hierarchy_segments(
        edge.source_node_id.as_str(),
        edge.target_node_id.as_str(),
        edge.source_port_key.as_str(),
        edge.target_port_key.as_str(),
        &edge.source_path,
        &edge.target_path,
    );
    let mut tail_segment = None;
    let mut center_segment = None;
    let mut head_segment = None;
    let mut labels_by_segment = (0..segments.len())
        .map(|_| Vec::new())
        .collect::<Vec<Vec<LLabel>>>();
    for label in &edge.labels {
        let segment_index = match label.placement {
            EdgeLabelPlacement::Tail => *tail_segment.get_or_insert_with(|| {
                compound_label_segment_index(&segments, EdgeLabelPlacement::Tail)
            }),
            EdgeLabelPlacement::Center => *center_segment.get_or_insert_with(|| {
                compound_label_segment_index(&segments, EdgeLabelPlacement::Center)
            }),
            EdgeLabelPlacement::Head => *head_segment.get_or_insert_with(|| {
                compound_label_segment_index(&segments, EdgeLabelPlacement::Head)
            }),
        };
        let mut label = label.clone();
        label.original_label_edge = Some(edge.id.clone());
        labels_by_segment[segment_index].push(label);
    }

    segments
        .into_iter()
        .enumerate()
        .map(|(segment_index, pending)| ScopedHierarchySegment {
            edge: HierarchyEdge {
                id: edge.id.clone(),
                source_node_id: edge.source_node_id.clone(),
                target_node_id: edge.target_node_id.clone(),
                source_port_key: edge.source_port_key.clone(),
                target_port_key: edge.target_port_key.clone(),
                source_port_named: edge.source_port_named,
                target_port_named: edge.target_port_named,
                source_path: Vec::new(),
                target_path: Vec::new(),
                labels: Vec::new(),
                minlen: edge.minlen,
                model_order: edge.model_order,
                priority_direction: edge.priority_direction,
                priority_shortness: edge.priority_shortness,
                priority_straightness: edge.priority_straightness,
            },
            pending,
            labels: std::mem::take(&mut labels_by_segment[segment_index]),
        })
        .collect()
}

// CompoundGraphPreprocessor#transformHierarchyEdges publishes all child exports before
// processOuterHierarchicalEdgeSegments visits the graph's own nodes and ports. Keep that
// publication order for both parent ports and their external dummies, independently of the
// original edge/model order. The explicit stack and stable buckets keep this linear and safe
// for deep hierarchies; no segment walks its ancestor chain again.
struct LocalPortPublication {
    node_id: String,
    port_key: String,
    port_type: PortType,
}

struct HierarchyPublications {
    segments: Vec<ScopedHierarchySegment>,
    local_ports: HashMap<Option<GraphId>, Vec<LocalPortPublication>>,
}

/// Walks `scope` and the graphs below it, children before the graph they lie in.
fn order_segments_by_publication(
    graphs: &GraphArena,
    scope: GraphId,
    segments: Vec<ScopedHierarchySegment>,
) -> HierarchyPublications {
    let mut node_order = HashMap::new();
    let mut nested_nodes = HashSet::new();
    let mut port_order = HashMap::new();
    let mut buckets = Vec::new();
    for current in graphs.postorder(scope) {
        let current = &graphs[current];
        for node in &current.layerless_nodes {
            if node.nested_graph.is_some() {
                nested_nodes.insert(node.id.as_str());
            }
            for port in &node.ports {
                // `processOuterHierarchicalEdgeSegments` visits the outgoing edges of a port,
                // then its incoming edges. A port the caller named can have both.
                for &port_type in lookup_types(port) {
                    port_order
                        .entry((node.id.as_str(), port_type, port.id.as_str()))
                        .or_insert(buckets.len());
                    buckets.push(Vec::new());
                }
            }
            // Already scoped inputs may introduce a port that is not present in the imported
            // node yet. Preserve their supplied order within that node's publication slot.
            node_order.entry(node.id.as_str()).or_insert(buckets.len());
            buckets.push(Vec::new());
        }
    }
    let unindexed = buckets.len();
    buckets.push(Vec::new());
    let publication_order =
        |node_id: &str, port_key: &str, port_type: PortType, endpoint: &PendingSegmentEndpoint| {
            port_order
                .get(&(node_id, port_type, port_key))
                .or_else(|| node_order.get(node_id))
                .or_else(|| match endpoint {
                    PendingSegmentEndpoint::LocalNode { node_id, .. } => {
                        node_order.get(node_id.as_str())
                    }
                    PendingSegmentEndpoint::ParentBoundary { .. } => None,
                })
                .copied()
                .unwrap_or(unindexed)
        };
    let mut port_buckets = (0..buckets.len()).map(|_| Vec::new()).collect::<Vec<_>>();
    for scoped in segments {
        let source_order = publication_order(
            &scoped.edge.source_node_id,
            &scoped.edge.source_port_key,
            PortType::Output,
            &scoped.pending.source,
        );
        let target_order = publication_order(
            &scoped.edge.target_node_id,
            &scoped.edge.target_port_key,
            PortType::Input,
            &scoped.pending.target,
        );
        // A sibling-to-sibling segment publishes each endpoint in its own child's order.
        // Creating both ports when that central segment is attached would incorrectly force
        // the target child's input ports to follow the source child's output order.
        for (endpoint, port_type, order) in [
            (&scoped.pending.source, PortType::Output, source_order),
            (&scoped.pending.target, PortType::Input, target_order),
        ] {
            if let PendingSegmentEndpoint::LocalNode { node_id, port_key } = endpoint
                && !port_order.contains_key(&(node_id.as_str(), port_type, port_key.as_str()))
            {
                port_buckets[order].push((
                    scoped.pending.lands_in,
                    LocalPortPublication {
                        node_id: node_id.clone(),
                        port_key: port_key.clone(),
                        port_type,
                    },
                ));
            }
        }
        // A pre-scoped segment retains its caller's absolute side during import rebasing.
        // Publication follows the local child/boundary relationship, not that label side.
        let order = match (&scoped.pending.source, &scoped.pending.target) {
            (PendingSegmentEndpoint::ParentBoundary { .. }, _) => target_order,
            (_, PendingSegmentEndpoint::ParentBoundary { .. }) => source_order,
            (
                PendingSegmentEndpoint::LocalNode {
                    node_id: source, ..
                },
                PendingSegmentEndpoint::LocalNode {
                    node_id: target, ..
                },
            ) => {
                if source != &scoped.edge.source_node_id && nested_nodes.contains(source.as_str()) {
                    source_order
                } else if target != &scoped.edge.target_node_id
                    && nested_nodes.contains(target.as_str())
                {
                    target_order
                } else {
                    match scoped.pending.segment {
                        CompoundEdgeSegment::Output { .. } => source_order,
                        CompoundEdgeSegment::Input { .. } => target_order,
                    }
                }
            }
        };
        buckets[order].push(scoped);
    }
    let mut local_ports: HashMap<_, Vec<_>> = HashMap::new();
    for (parent, port) in port_buckets.into_iter().flatten() {
        local_ports.entry(parent).or_default().push(port);
    }
    HierarchyPublications {
        segments: buckets.into_iter().flatten().collect(),
        local_ports,
    }
}

/// The port types under which a port is found. A port ELK makes up has one type. A port the
/// caller named has none, so an edge finds it by its id alone: it counts under both. The output
/// type comes first, as the outgoing edges of a port are visited before the incoming ones.
fn lookup_types(port: &LPort) -> &'static [PortType] {
    if port.named {
        &[PortType::Output, PortType::Input]
    } else if port.port_type == PortType::Output {
        &[PortType::Output]
    } else {
        &[PortType::Input]
    }
}

/// What a segment in a nested graph needs to know about one port of the graph's parent node.
#[derive(Debug, Clone, Copy)]
struct ParentPortInfo {
    /// The port's border offset, or 0 when it has none.
    border_offset: f64,
    /// `None` for a port ELK makes up. A port the caller named has the net flow, side, position
    /// and size that `createExternalPortDummy` reads from it, and its own dummy.
    named: Option<NamedParentPort>,
}

#[derive(Debug, Clone, Copy)]
struct NamedParentPort {
    /// `CompoundGraphPreprocessor.calculateNetFlow(outsidePort)`.
    net_flow: isize,
    side: PortSide,
    position: LPoint,
    size: LSize,
    port_index: Option<isize>,
}

/// What the parent node of a graph tells the segments introduced in that graph. A child graph
/// cannot look up its parent's ports through `GraphPortIndex`, so the ports are copied before
/// segment introduction creates exported parent ports.
#[derive(Debug, Clone, Default)]
struct ParentNodeInfo {
    /// The parent node's `PORT_CONSTRAINTS`, which `createExternalPortDummy` reads.
    port_constraints: PortConstraints,
    ports: HashMap<PortType, HashMap<String, ParentPortInfo>>,
}

/// What the ports of a graph's parent node give to the segments introduced in that graph.
struct ParentPorts<'a> {
    node: &'a ParentNodeInfo,
    net_flows: &'a ParentPortFlows,
}

/// Introduces the segments in `scope` and the graphs below it. A segment names the graph it lands
/// in by that graph's `GraphId`, or by `None` for `scope` itself.
pub(crate) fn introduce_source_ported_scoped_edge_segments(
    graphs: &mut GraphArena,
    scope: GraphId,
    segments: Vec<ScopedHierarchySegment>,
) {
    if segments.is_empty() {
        return;
    }
    let publications = order_segments_by_publication(graphs, scope, segments);
    let parent_port_flows = parent_port_net_flows(&publications.segments);
    let mut local_port_publications = publications.local_ports;
    let mut segments_by_parent: HashMap<Option<GraphId>, Vec<ScopedHierarchySegment>> =
        HashMap::new();
    // What the parent node of each graph with a parent boundary tells its segments, by the id of
    // that graph. The parent node's own id cannot key this: two nodes in different graphs may
    // share it.
    let mut original_parent_offsets: HashMap<GraphId, ParentNodeInfo> = HashMap::new();
    for scoped in publications.segments {
        let has_parent_boundary = [&scoped.pending.source, &scoped.pending.target]
            .into_iter()
            .any(|endpoint| matches!(endpoint, PendingSegmentEndpoint::ParentBoundary { .. }));
        if let (true, Some(lands_in)) = (has_parent_boundary, scoped.pending.lands_in) {
            original_parent_offsets.entry(lands_in).or_default();
        }
        segments_by_parent
            .entry(scoped.pending.lands_in)
            .or_default()
            .push(scoped);
    }
    let mut first_graph = true;
    for graph_id in graphs.preorder(scope) {
        let graph = &mut graphs[graph_id];
        // The traversal visits parents before children. The ports of each requested parent node
        // are copied once, before the segments of this graph add to them, preserving the same
        // first-match identity as GraphPortIndex.
        for node in &graph.layerless_nodes {
            let Some(nested) = node.nested_graph else {
                continue;
            };
            if let Some(info) = original_parent_offsets.get_mut(&nested) {
                info.port_constraints = node.port_constraints;
                for port in &node.ports {
                    let parent_port = ParentPortInfo {
                        border_offset: port.border_offset.unwrap_or(0.0),
                        named: port.named.then(|| NamedParentPort {
                            net_flow: port.hierarchy_net_flow.unwrap_or(0),
                            side: port.side,
                            position: port.position,
                            size: port.size,
                            port_index: port.port_index,
                        }),
                    };
                    for &port_type in lookup_types(port) {
                        info.ports
                            .entry(port_type)
                            .or_default()
                            .entry(port.id.clone())
                            .or_insert(parent_port);
                    }
                }
            }
        }
        let parent_offsets = original_parent_offsets
            .remove(&graph_id)
            .unwrap_or_default();
        let parent = if first_graph {
            first_graph = false;
            None
        } else {
            Some(graph_id)
        };
        if let Some(segments) = segments_by_parent.remove(&parent) {
            let mut state = SegmentState::default();
            let mut local_ports = GraphPortIndex::new(graph);
            for port in local_port_publications.remove(&parent).unwrap_or_default() {
                local_ports
                    .ensure_local_node_port(graph, &port.node_id, &port.port_key, port.port_type)
                    .expect("scoped local endpoints were validated before publication");
            }
            for scoped in segments {
                let edge_index = introduce_hierarchical_edge_segment(
                    graph,
                    &scoped.edge,
                    &scoped.pending,
                    scoped.labels,
                    &mut state,
                    &mut local_ports,
                    &ParentPorts {
                        node: &parent_offsets,
                        net_flows: &parent_port_flows,
                    },
                );
                let model_order = scoped.edge.model_order;
                record_cross_hierarchy_edge_segment(
                    graph,
                    scoped.edge.id,
                    model_order,
                    edge_index,
                    scoped.pending.segment,
                );
            }
        }
    }
    assert!(
        segments_by_parent.is_empty(),
        "scoped hierarchy segment owners were validated before graph mutation"
    );
}

/// What the segments of one graph share while they are introduced.
#[derive(Default)]
struct SegmentState {
    /// The external ports exported so far, which an edge at the same opposite port may reuse.
    external_ports: HashMap<ExternalPortKey, ExternalPort>,
    /// The dummy of each port of the parent node that the caller named, by the port's key.
    /// `dummyNodeMap` keeps one dummy for a port, however many edges end at it.
    parent_dummies: HashMap<String, PortRef>,
}

#[derive(Debug, Clone)]
struct ExternalPort {
    original_edges: Vec<String>,
    new_edge: EdgeId,
    dummy_node: NodeId,
    dummy_port: PortRef,
    port_type: PortType,
    exported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ExternalPortKey {
    opposite_port_key: String,
    port_type: PortType,
}

struct GraphPortIndex {
    nodes: HashMap<String, NodeId>,
    ports: HashMap<(NodeId, PortType), HashMap<String, PortId>>,
}

impl GraphPortIndex {
    fn new(graph: &LGraph) -> Self {
        let mut nodes = HashMap::with_capacity(graph.layerless_nodes.len());
        let mut ports = HashMap::new();
        for (node, data) in graph.nodes_with_ids() {
            nodes.entry(data.id.clone()).or_insert(node);
            for (port, data) in data.ports.iter_with_ids() {
                for &port_type in lookup_types(data) {
                    ports
                        .entry((node, port_type))
                        .or_insert_with(HashMap::new)
                        .entry(data.id.clone())
                        .or_insert(port);
                }
            }
        }
        Self { nodes, ports }
    }

    fn port(&self, node_id: &str, port_key: &str, port_type: PortType) -> Option<PortRef> {
        let node = *self.nodes.get(node_id)?;
        let port = *self.ports.get(&(node, port_type))?.get(port_key)?;
        Some(PortRef { node, port })
    }

    fn ensure_local_node_port(
        &mut self,
        graph: &mut LGraph,
        node_id: &str,
        port_key: &str,
        port_type: PortType,
    ) -> Option<PortRef> {
        if let Some(port) = self.port(node_id, port_key, port_type) {
            return Some(port);
        }
        let node = *self.nodes.get(node_id)?;
        let port = graph.layerless_nodes[node].ports.push(LPort::new(
            port_key.to_string(),
            node,
            port_type,
        ));
        self.ports
            .entry((node, port_type))
            .or_default()
            .insert(port_key.to_string(), port);
        Some(PortRef { node, port })
    }
}

struct NodePortIndex {
    owner: NodeId,
    ports: HashMap<PortType, HashMap<String, PortId>>,
}

impl NodePortIndex {
    fn new(graph: &LGraph, owner: NodeId) -> Self {
        let mut ports = HashMap::new();
        let node = graph
            .layerless_nodes
            .get(owner)
            .expect("compound parent node must exist while indexing its ports");
        for (port_index, port) in node.ports.iter_with_ids() {
            for &port_type in lookup_types(port) {
                ports
                    .entry(port_type)
                    .or_insert_with(HashMap::new)
                    .entry(port.id.clone())
                    .or_insert(port_index);
            }
        }
        Self { owner, ports }
    }

    fn owner(&self) -> NodeId {
        self.owner
    }

    fn port(&self, port_key: &str, port_type: PortType) -> Option<PortRef> {
        let port = *self.ports.get(&port_type)?.get(port_key)?;
        Some(PortRef {
            node: self.owner,
            port,
        })
    }

    fn record_port(&mut self, graph: &LGraph, port: PortId) {
        let port_data = graph
            .layerless_nodes
            .get(self.owner)
            .and_then(|node| node.ports.get(port))
            .expect("new compound parent port must belong to the indexed owner");
        for &port_type in lookup_types(port_data) {
            self.ports
                .entry(port_type)
                .or_default()
                .entry(port_data.id.clone())
                .or_insert(port);
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ExternalDummyMetadata {
    dummy_node: NodeId,
    external_port_side: PortSide,
    border_offset: Option<f64>,
}

struct NestedExternalDummyIndex {
    graph_id: GraphId,
    by_edge_id: HashMap<String, ExternalDummyMetadata>,
}

#[derive(Debug, Clone)]
struct CompoundEndpoint {
    port: PortRef,
    edge: EdgeId,
}

struct CompoundEndpointPlan {
    by_node: IdVec<NodeId, Vec<CompoundEndpoint>>,
}

impl CompoundEndpointPlan {
    fn new(graph: &LGraph, observer: &mut impl CompoundPreprocessObserver) -> Self {
        let mut by_node = IdVec::filled(graph.layerless_nodes.len(), Vec::new());
        for (edge_index, edge) in graph.edges_with_ids() {
            observer.visit_graph_edge_for_endpoint_plan();
            let Some(segment) = edge.compound_segment else {
                continue;
            };
            let port = match segment {
                CompoundEdgeSegment::Output { .. } => edge.source(),
                CompoundEdgeSegment::Input { .. } => edge.target(),
            };
            if graph
                .layerless_nodes
                .get(port.node)
                .is_some_and(|node| node.nested_graph.is_some())
            {
                by_node[port.node].push(CompoundEndpoint {
                    port,
                    edge: edge_index,
                });
            }
        }
        Self { by_node }
    }

    fn for_node(&self, node_index: NodeId) -> &[CompoundEndpoint] {
        self.by_node
            .get(node_index)
            .map(Vec::as_slice)
            .expect("compound endpoint plan must match the graph node arena")
    }

    fn has_endpoints(&self) -> bool {
        self.by_node.iter().any(|endpoints| !endpoints.is_empty())
    }
}

impl NestedExternalDummyIndex {
    fn new(
        graph: &LGraph,
        parent_node_id: &str,
        observer: &mut impl CompoundPreprocessObserver,
    ) -> Self {
        let dummy_id = format!("external:{parent_node_id}");
        let mut by_edge_id = HashMap::new();
        for (dummy_node, node) in graph.nodes_with_ids() {
            observer.visit_endpoint_nested_node();
            let Some(external) = node.external_port() else {
                continue;
            };
            if node.id != dummy_id {
                continue;
            }
            let Some(port) = node.ports.first() else {
                continue;
            };
            let metadata = ExternalDummyMetadata {
                dummy_node,
                external_port_side: external.side,
                border_offset: port.border_offset,
            };
            for port in &node.ports {
                for edge_index in port
                    .incoming_edges()
                    .iter()
                    .chain(port.outgoing_edges().iter())
                    .copied()
                {
                    observer.visit_endpoint_incident_edge();
                    // Keep the first match in nested node/port/list order, exactly like the
                    // source-backed external_dummy_for_compound_edge scan.
                    let edge_id = graph.edges[edge_index].id.clone();
                    by_edge_id.entry(edge_id).or_insert(metadata);
                }
            }
        }
        Self {
            graph_id: graph.id,
            by_edge_id,
        }
    }

    fn external_dummy(&self, edge_id: &str) -> Option<(GraphId, ExternalDummyMetadata)> {
        let metadata = *self.by_edge_id.get(edge_id)?;
        Some((self.graph_id, metadata))
    }
}

/// Source-backed equivalent of ELK's `introduceHierarchicalEdgeSegment(...)`.
fn introduce_hierarchical_edge_segment(
    graph: &mut LGraph,
    edge: &HierarchyEdge,
    pending: &PendingCompoundSegment,
    labels: Vec<LLabel>,
    state: &mut SegmentState,
    local_ports: &mut GraphPortIndex,
    parent_ports: &ParentPorts,
) -> EdgeId {
    let parent_boundary = match (&pending.source, &pending.target) {
        (
            PendingSegmentEndpoint::LocalNode { port_key, .. },
            PendingSegmentEndpoint::ParentBoundary {
                port_type,
                connects_parent_node,
                ..
            },
        ) => Some((*port_type, port_key.as_str(), *connects_parent_node)),
        (
            PendingSegmentEndpoint::ParentBoundary {
                port_type,
                connects_parent_node,
                ..
            },
            PendingSegmentEndpoint::LocalNode { port_key, .. },
        ) => Some((*port_type, port_key.as_str(), *connects_parent_node)),
        _ => None,
    };

    if let Some((port_type, opposite_port_key, connects_parent_node)) = parent_boundary
        && graph.options.merge_hierarchy_edges
        && !connects_parent_node
    {
        let key = ExternalPortKey {
            opposite_port_key: opposite_port_key.to_string(),
            port_type,
        };
        if let Some(external_port) = state.external_ports.get_mut(&key) {
            debug_assert_eq!(external_port.port_type, port_type);
            debug_assert!(external_port.exported);
            debug_assert_eq!(external_port.dummy_port.node, external_port.dummy_node);
            external_port.original_edges.push(edge.id.clone());
            debug_assert_eq!(external_port.original_edges.last(), Some(&edge.id));
            apply_label_graph_properties(graph, &labels);
            graph.edges[external_port.new_edge].labels.extend(labels);
            return external_port.new_edge;
        }
    }

    let source = ensure_segment_endpoint_port(
        graph,
        local_ports,
        &pending.source,
        PortType::Output,
        parent_ports,
        &mut state.parent_dummies,
    );
    let target = ensure_segment_endpoint_port(
        graph,
        local_ports,
        &pending.target,
        PortType::Input,
        parent_ports,
        &mut state.parent_dummies,
    );

    if source.node == target.node {
        graph.graph_properties.self_loops = true;
    }

    apply_label_graph_properties(graph, &labels);

    let edge_index = graph
        .add_edge({
            let mut built_edge = LayeredEdge::new(edge.id.clone(), source, target);
            built_edge.source_node_id = edge.source_node_id.clone();
            built_edge.target_node_id = edge.target_node_id.clone();
            built_edge.labels = labels;
            built_edge.minlen = edge.minlen;
            built_edge.model_order = edge.model_order;
            built_edge.priority_direction = edge.priority_direction;
            built_edge.priority_shortness = edge.priority_shortness;
            built_edge.priority_straightness = edge.priority_straightness;
            // `createDummyEdge` copies the original edge's properties, so the thickness is ELK's default.
            built_edge.thickness = 1.0;
            built_edge.compound_segment = Some(pending.segment);
            built_edge
        })
        .expect("ports were created before adding hierarchy edge segment");

    if let Some((port_type, opposite_port_key, connects_parent_node)) = parent_boundary {
        let dummy_port = match port_type {
            PortType::Output => target,
            PortType::Input => source,
        };
        if graph
            .layerless_nodes
            .get(dummy_port.node)
            .is_some_and(|node| node.kind() == LNodeKind::ExternalPort)
        {
            let exported = !connects_parent_node;
            let external_port = ExternalPort {
                original_edges: vec![edge.id.clone()],
                new_edge: edge_index,
                dummy_node: dummy_port.node,
                dummy_port,
                port_type,
                exported,
            };
            if exported {
                state.external_ports.insert(
                    ExternalPortKey {
                        opposite_port_key: opposite_port_key.to_string(),
                        port_type,
                    },
                    external_port,
                );
            }
        }
    }

    edge_index
}

fn apply_label_graph_properties(graph: &mut LGraph, labels: &[LLabel]) {
    for label in labels {
        match label.placement {
            EdgeLabelPlacement::Center => graph.graph_properties.center_labels = true,
            EdgeLabelPlacement::Head | EdgeLabelPlacement::Tail => {
                graph.graph_properties.end_labels = true;
            }
        }
    }
}

/// The net flow of each parent port that hierarchy edges end at, keyed by the parent node's id
/// and the port key.
type ParentPortFlows = HashMap<(String, String), isize>;

/// Port of `CompoundGraphPreprocessor.calculateNetFlow(outsidePort)`, for every parent port that
/// the edges in `segments` end at.
///
/// Java walks the port's edges and counts a vote for "output port of the parent" or for "input
/// port of the parent" per edge. It returns `outputPortVote - inputPortVote`.
///
/// - An edge that leaves the port votes "input" if its far end sits directly in the parent's
///   graph, as the port then feeds the nested graph. Any other far end votes "output": a node
///   deeper down, or a node outside the parent.
/// - An edge that enters the port votes the other way round.
/// - Java adds two self-loop cases. An inside self loop votes like an edge from the inside, an
///   outside self loop like an edge from the outside. Neither is a hierarchy edge here. The
///   importer turns inside self loops into dummies on their own port keys.
///
/// An edge reaches the port through exactly one segment that ends at the parent node, so each
/// edge votes once. This holds for a port that ELK made up, which gets one edge. A port the caller
/// named can have edges that never enter the parent's graph, and those vote too. The importer
/// counts them all, and `NamedParentPort::net_flow` carries the total, which the dummy of such a
/// port uses instead of the votes counted here.
fn parent_port_net_flows(segments: &[ScopedHierarchySegment]) -> ParentPortFlows {
    let mut flows = ParentPortFlows::new();
    for scoped in segments {
        let ends = [
            (
                &scoped.pending.source,
                &scoped.pending.target,
                &scoped.edge.target_node_id,
            ),
            (
                &scoped.pending.target,
                &scoped.pending.source,
                &scoped.edge.source_node_id,
            ),
        ];
        for (boundary, far_end, far_original_end) in ends {
            let PendingSegmentEndpoint::ParentBoundary {
                node_id,
                port_key,
                port_type,
                connects_parent_node: true,
                ..
            } = boundary
            else {
                continue;
            };
            // The segment next to the parent ends at the original end only if that end is a
            // direct child of the parent.
            let far_end_is_child_of_parent = match far_end {
                PendingSegmentEndpoint::LocalNode { node_id, .. } => node_id == far_original_end,
                // Every segment has one `ParentBoundary` end and one `LocalNode` end, and `boundary`
                // is the first. A second boundary end would need a vote this function does not
                // define, so it is a bug in the code that builds segments, not input.
                PendingSegmentEndpoint::ParentBoundary { .. } => unreachable!(
                    "a compound segment has exactly one parent boundary end and one local end"
                ),
            };
            // The dummy's port type is `Input` for an edge that leaves the parent's port.
            let leaves_port = *port_type == PortType::Input;
            let votes_output = leaves_port != far_end_is_child_of_parent;
            *flows
                .entry((node_id.clone(), port_key.clone()))
                .or_insert(0) += if votes_output { 1 } else { -1 };
        }
    }
    flows
}

fn ensure_segment_endpoint_port(
    graph: &mut LGraph,
    local_ports: &mut GraphPortIndex,
    endpoint: &PendingSegmentEndpoint,
    port_type: PortType,
    parent_ports: &ParentPorts,
    parent_dummies: &mut HashMap<String, PortRef>,
) -> PortRef {
    match endpoint {
        PendingSegmentEndpoint::LocalNode { node_id, port_key } => local_ports
            .ensure_local_node_port(graph, node_id.as_str(), port_key.as_str(), port_type)
            .expect("compound segment local endpoint should exist in the current graph"),
        PendingSegmentEndpoint::ParentBoundary { node_id, .. } => {
            let dummy_port_type = endpoint_port_type(endpoint, port_type);
            let parent_port_type = endpoint_parent_port_type(endpoint, port_type);
            let port_key = endpoint_port_key(endpoint).unwrap_or_default();
            let connects_parent_node = endpoint_connects_parent_node(endpoint);
            let parent_port = connects_parent_node
                .then(|| {
                    parent_ports
                        .node
                        .ports
                        .get(&parent_port_type)
                        .and_then(|ports| ports.get(port_key))
                        .copied()
                })
                .flatten();
            let named = parent_port.and_then(|port| port.named);
            // `createExternalPortDummy`, parent-node branch: a port of the parent that has a
            // dummy keeps it, and every edge at the port connects to that dummy.
            if named.is_some()
                && let Some(&dummy) = parent_dummies.get(port_key)
            {
                return dummy;
            }
            // `calculateNetFlow(outsidePort)` counts every edge at a named port. Its other
            // branch makes a fresh port, which takes -1 for an input and 1 for an output.
            let net_flow = if let Some(named) = named {
                named.net_flow
            } else if connects_parent_node {
                parent_ports
                    .net_flows
                    .get(&(node_id.clone(), port_key.to_string()))
                    .copied()
                    .expect("every port a parent end connects to has its votes counted")
            } else {
                match dummy_port_type {
                    PortType::Input => -1,
                    PortType::Output => 1,
                }
            };
            let original_border_offset =
                connects_parent_node.then(|| parent_port.map_or(0.0, |port| port.border_offset));
            let dummy = create_parent_boundary_port(
                graph,
                local_ports,
                ParentBoundaryPort {
                    parent_node_id: node_id,
                    dummy_port_type,
                    parent_port_type,
                    parent_port_key: port_key,
                    net_flow,
                    original_border_offset,
                    parent_constraints: parent_ports.node.port_constraints,
                    named,
                },
            );
            if named.is_some() {
                parent_dummies.insert(port_key.to_string(), dummy);
            }
            dummy
        }
    }
}

fn endpoint_port_type(endpoint: &PendingSegmentEndpoint, fallback: PortType) -> PortType {
    match endpoint {
        PendingSegmentEndpoint::ParentBoundary { port_type, .. } => *port_type,
        PendingSegmentEndpoint::LocalNode { .. } => fallback,
    }
}

fn endpoint_parent_port_type(endpoint: &PendingSegmentEndpoint, fallback: PortType) -> PortType {
    match endpoint {
        PendingSegmentEndpoint::ParentBoundary {
            parent_port_type, ..
        } => *parent_port_type,
        PendingSegmentEndpoint::LocalNode { .. } => fallback,
    }
}

fn endpoint_port_key(endpoint: &PendingSegmentEndpoint) -> Option<&str> {
    match endpoint {
        PendingSegmentEndpoint::ParentBoundary { port_key, .. }
        | PendingSegmentEndpoint::LocalNode { port_key, .. } => Some(port_key.as_str()),
    }
}

fn endpoint_connects_parent_node(endpoint: &PendingSegmentEndpoint) -> bool {
    match endpoint {
        PendingSegmentEndpoint::ParentBoundary {
            connects_parent_node,
            ..
        } => *connects_parent_node,
        PendingSegmentEndpoint::LocalNode { .. } => false,
    }
}

fn prepare_compound_endpoint_metadata(
    graph: &mut LGraph,
    observer: &mut impl CompoundPreprocessObserver,
) -> CompoundEndpointPlan {
    let endpoint_plan = CompoundEndpointPlan::new(graph, observer);
    let mut recorded_edges = IdVec::filled(graph.edges.len(), false);
    for record in &graph.cross_hierarchy_edges {
        observer.visit_existing_cross_hierarchy_edge();
        if let Some(recorded) = recorded_edges.get_mut(record.edge) {
            *recorded = true;
        }
    }

    for edge_index in recorded_edges.ids() {
        let Some(segment) = graph.edges[edge_index].compound_segment else {
            continue;
        };
        observer.process_segment_edge();
        if !recorded_edges[edge_index] {
            let edge_id = graph.edges[edge_index].id.clone();
            let model_order = graph.edges[edge_index].model_order;
            record_cross_hierarchy_edge_segment(graph, edge_id, model_order, edge_index, segment);
            recorded_edges[edge_index] = true;
        }
    }

    if endpoint_plan.has_endpoints() {
        graph.options.port_constraints = if graph.options.port_constraints.is_side_fixed() {
            PortConstraints::FixedSide
        } else {
            PortConstraints::Free
        };
        graph.graph_properties.non_free_ports = true;
    }

    endpoint_plan
}

/// Gives each parent port that has no dummy yet its dummy in the graph inside the node. Graphs are
/// handled deepest first, so a graph's nested graphs are done before the graph itself.
fn ensure_nested_external_dummies_for_parent_ports(
    graphs: &mut GraphArena,
    order: &[GraphId],
    original_constraints: &[IdVec<NodeId, PortConstraints>],
    net_flows: &HashMap<OriginalNetFlowKey, isize>,
) {
    for &graph in order.iter().rev() {
        ensure_direct_nested_external_dummies_for_parent_ports(
            graphs,
            graph,
            &original_constraints[graph.index()],
            net_flows,
        );
    }
}

fn ensure_direct_nested_external_dummies_for_parent_ports(
    graphs: &mut GraphArena,
    graph_id: GraphId,
    original_constraints: &IdVec<NodeId, PortConstraints>,
    net_flows: &HashMap<OriginalNetFlowKey, isize>,
) {
    for (node_index, &parent_constraints) in original_constraints.iter_with_ids() {
        let Some(nested_graph_id) = graphs[graph_id].layerless_nodes[node_index].nested_graph
        else {
            continue;
        };
        if !graphs[nested_graph_id].graph_properties.external_ports {
            continue;
        }
        let (graph, nested_graph) = graphs.pair_mut(graph_id, nested_graph_id);

        let parent_node_id = graph.layerless_nodes[node_index].id.clone();
        let parent_size = graph.layerless_nodes[node_index].size;
        let nested_direction = nested_graph.options.direction;

        // Linking a dummy writes the port's dummy and never changes the node's list.
        for position in 0..graph.layerless_nodes[node_index].ports.len() {
            let Some(port_index) = graph.layerless_nodes[node_index].ports.id_at(position) else {
                continue;
            };
            if graph.layerless_nodes[node_index].ports[port_index]
                .port_dummy
                .is_none()
            {
                let parent_port = PortRef {
                    node: node_index,
                    port: port_index,
                };
                let port = &graph.layerless_nodes[node_index].ports[port_index];
                // `createExternalPortDummy(port, portConstraints, port.getSide(), ...)`: the node's
                // constraints from before the dummies were linked, and the port's own side.
                let (port_constraints, port_side) = (parent_constraints, port.side);
                // ELK passes no position and no node size here: `new KVector()` and `null`. The node
                // size only guards against a division by zero for fixed ratios, where ELK fails.
                let dummy_node = nested_graph.push_node_with(|node| {
                    let mut dummy = create_external_port_dummy(
                        node,
                        format!("external:{parent_node_id}"),
                        format!("external:{parent_node_id}:0"),
                        port.port_type,
                        port_constraints,
                        port_side,
                        -net_flows
                            .get(&(parent_node_id.clone(), port.id.clone(), port.named))
                            .copied()
                            .unwrap_or_else(|| port.net_flow()),
                        LPoint::default(),
                        port.size,
                        parent_size,
                        port.border_offset.unwrap_or(0.0),
                        nested_direction,
                        ExternalPortData {
                            parent_port_key: Some(port.id.clone()),
                            parent_port_type: Some(port.port_type),
                            ..ExternalPortData::default()
                        },
                    );
                    apply_port_index_to_dummy(&mut dummy, port_constraints, port.port_index);
                    dummy
                });
                let dummy_side = nested_graph.layerless_nodes[dummy_node].external_port_side();
                let dummy_border_offset = port.border_offset;

                link_external_port_dummy(
                    graph,
                    parent_port,
                    dummy_side,
                    nested_graph_id,
                    dummy_node,
                    dummy_border_offset,
                );
                set_external_dummy_origin(nested_graph, dummy_node, graph_id, parent_port);
            }
            reserve_room_for_port_labels(
                graph,
                node_index,
                port_index,
                nested_graph,
                parent_constraints,
            );
        }
    }
}

/// Gives the dummy of a port, in the graph inside the port's node, a label for each label of the
/// port, so the graph leaves room for them.
///
/// Ports the label loop of `CompoundGraphPreprocessor.transformHierarchyEdges`. A port that is
/// placed outside needs room only inside the node: the label keeps its size along the side's axis
/// only if its placement is fixed and the label reaches into the node.
fn reserve_room_for_port_labels(
    graph: &LGraph,
    node: NodeId,
    port: PortId,
    nested_graph: &mut LGraph,
    parent_constraints: PortConstraints,
) {
    let node_data = &graph.layerless_nodes[node];
    let port_data = &node_data.ports[port];
    let Some(dummy) = port_data.port_dummy else {
        return;
    };
    let inside_port_labels = node_data
        .port_label_placement
        .contains(PortLabelPlacement::INSIDE);
    let placement_fixed = node_data.port_label_placement.is_fixed();
    let mut labels = Vec::with_capacity(port_data.labels.len());
    for label in &port_data.labels {
        let mut dummy_label = LLabel::new("", label.size.width, label.size.height);
        // At this point the side of the port may not be known if the port constraints are free.
        // Then the port ends up on the east or west side (ELK issue 596).
        if !inside_port_labels {
            let inside_part = if placement_fixed {
                // Border offset 0: only the part of the label inside the node "after" the port.
                compute_inside_part(
                    label.position,
                    label.size,
                    port_data.size,
                    0.0,
                    port_data.side,
                )
            } else {
                0.0
            };
            if parent_constraints == PortConstraints::Free
                || matches!(port_data.side, PortSide::East | PortSide::West)
            {
                dummy_label.size.width = inside_part;
            } else {
                dummy_label.size.height = inside_part;
            }
        }
        labels.push(dummy_label);
    }
    nested_graph.layerless_nodes[dummy.node].ports[PortId::new(0)]
        .labels
        .extend(labels);
}

#[derive(Debug, Clone)]
struct ExternalDummyInfo {
    dummy_node: NodeId,
    port_type: PortType,
    parent_port_key: String,
    parent_port_type: PortType,
    external_port_side: PortSide,
    border_offset: Option<f64>,
    origin_port: Option<GraphPortRef>,
}

fn link_compound_external_dummy_metadata(
    graphs: &mut GraphArena,
    endpoint_plans: &[Option<CompoundEndpointPlan>],
    observer: &mut impl CompoundPreprocessObserver,
) {
    // Each nested graph, in the order of a walk that finishes one graph's subtree before the next
    // sibling.
    for child in graphs.preorder(GraphId::ROOT).into_iter().skip(1) {
        let GraphParent {
            graph: parent,
            node: parent_node,
        } = graphs[child]
            .parent
            .expect("every nested graph lies inside a node");
        let plan = endpoint_plans[parent.index()]
            .as_ref()
            .expect("every graph has an endpoint plan");
        let (parent_graph, nested_graph) = graphs.pair_mut(parent, child);
        link_direct_compound_external_dummy_metadata(
            parent_graph,
            parent_node,
            nested_graph,
            plan.for_node(parent_node),
            observer,
        );
    }
}

fn link_direct_compound_external_dummy_metadata(
    graph: &mut LGraph,
    node_index: NodeId,
    nested_graph: &mut LGraph,
    endpoints: &[CompoundEndpoint],
    observer: &mut impl CompoundPreprocessObserver,
) {
    let graph_id = graph.id;
    let parent_node_id = graph.layerless_nodes[node_index].id.clone();
    let nested_graph_id = nested_graph.id;
    if !endpoints.is_empty() {
        let endpoint_index =
            NestedExternalDummyIndex::new(nested_graph, parent_node_id.as_str(), observer);
        for endpoint in endpoints {
            // The endpoint plan is consumed before edge slots can be reordered or removed, so the
            // stable edge index avoids cloning IDs while retaining official edge-list order.
            let Some((dummy_graph_id, dummy)) =
                endpoint_index.external_dummy(graph.edges[endpoint.edge].id.as_str())
            else {
                continue;
            };
            // The edge endpoint is the authoritative port identity. The later key-based pass is
            // only a compatibility fallback and must not collapse duplicate port IDs to the first
            // matching port.
            link_external_port_dummy(
                graph,
                endpoint.port,
                dummy.external_port_side,
                dummy_graph_id,
                dummy.dummy_node,
                dummy.border_offset,
            );
            set_external_dummy_origin(nested_graph, dummy.dummy_node, graph_id, endpoint.port);
        }
    }

    let external_dummies = external_dummies_for_parent_node(nested_graph, parent_node_id.as_str());
    if external_dummies.is_empty() {
        return;
    }

    observer.build_parent_port_index();
    // Every nested graph belongs to exactly one compound parent node. Index only that owner's
    // ports, and append newly created ports in the existing dummy traversal order. This preserves
    // ELK's list-order semantics without rescanning unrelated siblings in the parent graph.
    let mut parent_ports = NodePortIndex::new(graph, node_index);

    for external_dummy in external_dummies {
        let parent_port = if let Some(port) =
            parent_port_for_external_dummy(graph, &parent_ports, &external_dummy)
        {
            port
        } else {
            let port = create_parent_external_port(
                graph,
                node_index,
                external_dummy.port_type,
                external_dummy.external_port_side,
                Some(external_dummy.parent_port_key.as_str()),
            );
            parent_ports.record_port(graph, port.port);
            port
        };
        link_external_port_dummy(
            graph,
            parent_port,
            external_dummy.external_port_side,
            nested_graph_id,
            external_dummy.dummy_node,
            external_dummy.border_offset,
        );
        set_external_dummy_origin(
            nested_graph,
            external_dummy.dummy_node,
            graph_id,
            parent_port,
        );
    }
}

fn external_dummies_for_parent_node(
    nested_graph: &LGraph,
    parent_node_id: &str,
) -> Vec<ExternalDummyInfo> {
    let dummy_id = format!("external:{parent_node_id}");
    nested_graph
        .nodes_with_ids()
        .filter(|(_, node)| node.id == dummy_id)
        .filter_map(|(dummy_node, node)| {
            let external = node.external_port()?;
            let port = node.ports.first()?;
            Some(ExternalDummyInfo {
                dummy_node,
                port_type: port.port_type,
                parent_port_key: external
                    .parent_port_key
                    .clone()
                    .unwrap_or_else(|| port.id.clone()),
                parent_port_type: external.parent_port_type.unwrap_or(port.port_type),
                external_port_side: external.side,
                border_offset: port.border_offset,
                origin_port: external.origin_port,
            })
        })
        .collect()
}

fn parent_port_for_external_dummy(
    graph: &LGraph,
    parent_ports: &NodePortIndex,
    external_dummy: &ExternalDummyInfo,
) -> Option<PortRef> {
    let parent_node = parent_ports.owner();
    if let Some(origin_port) = external_dummy.origin_port.as_ref()
        && origin_port.graph_id == graph.id
        && origin_port.port.node == parent_node
        && graph
            .layerless_nodes
            .get(parent_node)
            .and_then(|node| node.ports.get(origin_port.port.port))
            .is_some()
    {
        return Some(origin_port.port);
    }

    if !external_dummy.parent_port_key.is_empty()
        && let Some(port) = parent_ports.port(
            external_dummy.parent_port_key.as_str(),
            external_dummy.parent_port_type,
        )
    {
        return Some(port);
    }

    None
}

fn create_parent_external_port(
    graph: &mut LGraph,
    parent_node: NodeId,
    port_type: PortType,
    port_side: PortSide,
    port_key: Option<&str>,
) -> PortRef {
    let port_side = if port_side == PortSide::Undefined {
        match port_type {
            PortType::Output => port_side_from_direction(graph.options.direction),
            PortType::Input => port_side_from_direction(graph.options.direction).opposed(),
        }
    } else {
        port_side
    };
    let port = graph
        .add_port(parent_node, port_type, port_side, Default::default())
        .expect("parent compound node should exist when linking external dummy");
    if let Some(port_key) = port_key
        && !port_key.is_empty()
        && let Some(port_data) = graph.layerless_nodes[parent_node].ports.get_mut(port.port)
    {
        port_data.id = port_key.to_string();
    }
    port
}

/// What `create_parent_boundary_port` needs to know about the parent's port.
struct ParentBoundaryPort<'a> {
    parent_node_id: &'a str,
    dummy_port_type: PortType,
    parent_port_type: PortType,
    parent_port_key: &'a str,
    /// `calculateNetFlow(outsidePort)` for an edge that ends at the parent node itself, else the
    /// fixed flow of a fresh port.
    net_flow: isize,
    /// `Some(original border offset)` when the edge ends at the parent node itself.
    original_border_offset: Option<f64>,
    /// The `PORT_CONSTRAINTS` of the parent node.
    parent_constraints: PortConstraints,
    /// The parent's port when the caller named it.
    named: Option<NamedParentPort>,
}

fn create_parent_boundary_port(
    graph: &mut LGraph,
    local_ports: &GraphPortIndex,
    boundary: ParentBoundaryPort,
) -> PortRef {
    let ParentBoundaryPort {
        parent_node_id,
        dummy_port_type,
        parent_port_type,
        parent_port_key,
        net_flow,
        original_border_offset: original_parent_border_offset,
        parent_constraints,
        named,
    } = boundary;
    let connects_parent_node = original_parent_border_offset.is_some();
    graph.graph_properties.external_ports = true;
    graph.options.port_constraints = if graph.options.port_constraints.is_side_fixed() {
        PortConstraints::FixedSide
    } else {
        PortConstraints::Free
    };
    let parent_port_side = if let Some(named) = named {
        // `parentEndPort.getSide()`
        Some(named.side)
    } else if connects_parent_node {
        local_ports
            .port(parent_node_id, parent_port_key, parent_port_type)
            .and_then(|port| graph.layerless_nodes[port.node].ports.get(port.port))
            .map(|port| port.side)
    } else {
        None
    };
    let port_side = if let Some(port_side) = parent_port_side {
        port_side
    } else if parent_constraints.is_side_fixed() {
        if connects_parent_node {
            // The parent's port is one that ELK made, on the default side of its type.
            match parent_port_type {
                PortType::Input => port_side_from_direction(graph.options.direction).opposed(),
                PortType::Output => port_side_from_direction(graph.options.direction),
            }
        } else {
            // "We try to infer the port side from the port type": west or east, whatever the
            // direction of the layout is.
            match dummy_port_type {
                PortType::Input => PortSide::West,
                PortType::Output => PortSide::East,
            }
        }
    } else {
        PortSide::Undefined
    };
    // CompoundGraphPreprocessor#createExternalPortDummy inherits the original port's properties
    // for a direct parent connection. Only a genuinely exported port gets createExternalPortProperties
    // and its spacing/2 margin; that margin is not a default for ports on the parent itself.
    let border_offset =
        original_parent_border_offset.unwrap_or(graph.options.spacing.edge_edge / 2.0);
    // `outsidePort.getPosition()` and `getSize()`; a made-up port has neither. The parent's
    // size is only read for fixed ratios, where ELK's call passes `null` and fails.
    let (port_position, port_size) = named.map_or((LPoint::default(), LSize::default()), |named| {
        (named.position, named.size)
    });
    let direction = graph.options.direction;
    let node = graph.push_node_with(|node| {
        let mut dummy = create_external_port_dummy(
            node,
            format!("external:{parent_node_id}"),
            if parent_port_key.is_empty() {
                format!("external:{parent_node_id}:0")
            } else {
                parent_port_key.to_string()
            },
            dummy_port_type,
            parent_constraints,
            port_side,
            net_flow,
            port_position,
            port_size,
            LSize::default(),
            border_offset,
            direction,
            ExternalPortData {
                parent_port_key: (!parent_port_key.is_empty()).then(|| parent_port_key.to_string()),
                parent_port_type: if connects_parent_node {
                    Some(parent_port_type)
                } else {
                    Some(dummy_port_type)
                },
                ..ExternalPortData::default()
            },
        );
        apply_port_index_to_dummy(
            &mut dummy,
            parent_constraints,
            named.and_then(|named| named.port_index),
        );
        dummy
    });
    PortRef {
        node,
        port: PortId::new(0),
    }
}

/// ELK's `ElkUtil.calcPortSide`. It reads only the position and size of the port it gets, so
/// here it takes those two. A caller with no `LPort` yet has nothing to invent.
pub(crate) fn calc_port_side(
    position: LPoint,
    size: LSize,
    node_size: LSize,
    direction: ElkDirection,
) -> PortSide {
    let node_width = node_size.width;
    let node_height = node_size.height;
    if node_width <= 0.0 && node_height <= 0.0 {
        return PortSide::Undefined;
    }

    let xpos = position.x;
    let ypos = position.y;
    let width = size.width;
    let height = size.height;

    match direction {
        ElkDirection::Left | ElkDirection::Right => {
            if xpos < 0.0 {
                PortSide::West
            } else if xpos + width > node_width {
                PortSide::East
            } else {
                calc_port_side_from_percentages(node_width, node_height, xpos, ypos, width, height)
            }
        }
        ElkDirection::Up | ElkDirection::Down => {
            if ypos < 0.0 {
                PortSide::North
            } else if ypos + height > node_height {
                PortSide::South
            } else {
                calc_port_side_from_percentages(node_width, node_height, xpos, ypos, width, height)
            }
        }
        ElkDirection::Undefined => {
            calc_port_side_from_percentages(node_width, node_height, xpos, ypos, width, height)
        }
    }
}

fn calc_port_side_from_percentages(
    node_width: f64,
    node_height: f64,
    xpos: f64,
    ypos: f64,
    width: f64,
    height: f64,
) -> PortSide {
    let width_percent = (xpos + width / 2.0) / node_width;
    let height_percent = (ypos + height / 2.0) / node_height;

    if width_percent + height_percent <= 1.0 && width_percent - height_percent <= 0.0 {
        PortSide::West
    } else if width_percent + height_percent >= 1.0 && width_percent - height_percent >= 0.0 {
        PortSide::East
    } else if height_percent < 0.5 {
        PortSide::North
    } else {
        PortSide::South
    }
}

pub fn compare_compound_segments(
    left: CompoundEdgeSegment,
    right: CompoundEdgeSegment,
) -> std::cmp::Ordering {
    match (left, right) {
        (CompoundEdgeSegment::Output { .. }, CompoundEdgeSegment::Input { .. }) => {
            std::cmp::Ordering::Less
        }
        (CompoundEdgeSegment::Input { .. }, CompoundEdgeSegment::Output { .. }) => {
            std::cmp::Ordering::Greater
        }
        (
            CompoundEdgeSegment::Output { depth: left },
            CompoundEdgeSegment::Output { depth: right },
        ) => right.cmp(&left),
        (
            CompoundEdgeSegment::Input { depth: left },
            CompoundEdgeSegment::Input { depth: right },
        ) => left.cmp(&right),
    }
}

/// How many steps two paths share. A node without a graph falls back to the root graph, so the
/// graph alone cannot tell two such nodes apart. Both the node and the graph must match.
fn common_graph_depth(left: &[HierarchyStep], right: &[HierarchyStep]) -> usize {
    left.iter()
        .zip(right.iter())
        .take_while(|(left, right)| left.node_id == right.node_id && left.graph == right.graph)
        .count()
}

#[cfg(test)]
mod tests;
