//! Compound graph preprocessing support.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/compound/CompoundGraphPreprocessor.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/compound/CrossHierarchyEdgeComparator.java

use std::collections::{HashMap, HashSet};

use crate::graph::{
    CompoundEdgeSegment, CrossHierarchyEdge, EdgeLabelPlacement, GraphArena, GraphId, GraphNodeRef,
    GraphPortRef, HierarchyEdge, LGraph, LLabel, LNodeKind, LPoint, LPort, LSize, LayeredEdge,
    PortId, PortRef, PortSide, PortType, apply_port_index_to_dummy, create_external_port_dummy,
    port_side_from_direction,
};
use crate::options::{ElkDirection, PortConstraints};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingCompoundSegment {
    pub(crate) graph_parent: Option<String>,
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
pub(crate) fn source_ported_cross_hierarchy_segments<SourcePath, TargetPath>(
    source: &str,
    target: &str,
    source_port_key: &str,
    target_port_key: &str,
    source_path: &[SourcePath],
    target_path: &[TargetPath],
) -> Vec<PendingCompoundSegment>
where
    SourcePath: AsRef<str>,
    TargetPath: AsRef<str>,
{
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
            source_path[depth].as_ref().to_string()
        };
        let connects_parent_node = target == source_path[depth - 1].as_ref();
        segments.push(PendingCompoundSegment {
            graph_parent: Some(source_path[depth - 1].as_ref().to_string()),
            source: PendingSegmentEndpoint::LocalNode {
                node_id: segment_source,
                port_key: source_port_key.to_string(),
            },
            target: PendingSegmentEndpoint::ParentBoundary {
                node_id: source_path[depth - 1].as_ref().to_string(),
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
            source_path[common_depth].as_ref().to_string()
        } else {
            source.to_string()
        };
        let segment_target = if target_path.len() > common_depth {
            target_path[common_depth].as_ref().to_string()
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
            graph_parent: common_depth
                .checked_sub(1)
                .map(|parent_depth| source_path[parent_depth].as_ref().to_string()),
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
            target_path[depth].as_ref().to_string()
        };
        let connects_parent_node = source == target_path[depth - 1].as_ref();
        segments.push(PendingCompoundSegment {
            graph_parent: Some(target_path[depth - 1].as_ref().to_string()),
            source: PendingSegmentEndpoint::ParentBoundary {
                node_id: target_path[depth - 1].as_ref().to_string(),
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

fn cross_hierarchy_shape<SourcePath, TargetPath>(
    source: &str,
    target: &str,
    source_path: &[SourcePath],
    target_path: &[TargetPath],
) -> CrossHierarchyShape
where
    SourcePath: AsRef<str>,
    TargetPath: AsRef<str>,
{
    let common_depth = common_graph_depth(source_path, target_path);
    CrossHierarchyShape {
        common_depth,
        source_is_target_ancestor: target_path.len() > common_depth
            && source == target_path[common_depth].as_ref(),
        target_is_source_ancestor: source_path.len() > common_depth
            && target == source_path[common_depth].as_ref(),
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
    dummy_node: usize,
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
    dummy_node: usize,
    origin_graph_id: GraphId,
    origin_port: PortRef,
) {
    let Some(dummy) = dummy_graph.layerless_nodes.get_mut(dummy_node) else {
        return;
    };
    dummy.origin_port = Some(GraphPortRef {
        graph_id: origin_graph_id,
        port: origin_port,
    });
}

pub(crate) fn record_cross_hierarchy_edge_segment(
    graph: &mut LGraph,
    original_edge_id: impl Into<String>,
    original_model_order: Option<usize>,
    edge: usize,
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
    let mut original_constraints = vec![Vec::new(); graphs.len()];
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

fn port_constraints_of(graph: &LGraph) -> Vec<PortConstraints> {
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
    local_ports: HashMap<Option<String>, Vec<LocalPortPublication>>,
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
                    scoped.pending.graph_parent.clone(),
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
/// in by the id of that graph's parent node, or by `None` for `scope` itself.
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
    let mut segments_by_parent: HashMap<Option<String>, Vec<ScopedHierarchySegment>> =
        HashMap::new();
    let mut original_parent_offsets: HashMap<String, ParentNodeInfo> = HashMap::new();
    for scoped in publications.segments {
        for endpoint in [&scoped.pending.source, &scoped.pending.target] {
            if let PendingSegmentEndpoint::ParentBoundary { node_id, .. } = endpoint {
                original_parent_offsets.entry(node_id.clone()).or_default();
            }
        }
        segments_by_parent
            .entry(scoped.pending.graph_parent.clone())
            .or_default()
            .push(scoped);
    }
    let mut first_graph = true;
    for graph in graphs.preorder(scope) {
        let graph = &mut graphs[graph];
        // The traversal visits parents before children. Each requested parent's ports
        // are copied once, preserving the same first-match identity as GraphPortIndex.
        for node in &graph.layerless_nodes {
            if let Some(info) = original_parent_offsets.get_mut(&node.id) {
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
        let parent_offsets = graph
            .parent_node_id
            .as_ref()
            .and_then(|id| original_parent_offsets.remove(id))
            .unwrap_or_default();
        let parent = if first_graph {
            first_graph = false;
            None
        } else {
            graph.parent_node_id.clone()
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
    new_edge: usize,
    dummy_node: usize,
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
    nodes: HashMap<String, usize>,
    ports: HashMap<(usize, PortType), HashMap<String, PortId>>,
}

impl GraphPortIndex {
    fn new(graph: &LGraph) -> Self {
        let mut nodes = HashMap::with_capacity(graph.layerless_nodes.len());
        let mut ports = HashMap::new();
        for (node, data) in graph.layerless_nodes.iter().enumerate() {
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
    owner: usize,
    ports: HashMap<PortType, HashMap<String, PortId>>,
}

impl NodePortIndex {
    fn new(graph: &LGraph, owner: usize) -> Self {
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

    fn owner(&self) -> usize {
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
    dummy_node: usize,
    external_port_side: PortSide,
    border_offset: Option<f64>,
}

struct NestedExternalDummyIndex {
    graph_id: GraphId,
    by_edge_id: HashMap<String, ExternalDummyMetadata>,
}

#[derive(Debug)]
struct CompoundEndpoint {
    port: PortRef,
    edge: usize,
}

struct CompoundEndpointPlan {
    by_node: Vec<Vec<CompoundEndpoint>>,
}

impl CompoundEndpointPlan {
    fn new(graph: &LGraph, observer: &mut impl CompoundPreprocessObserver) -> Self {
        let mut by_node = (0..graph.layerless_nodes.len())
            .map(|_| Vec::new())
            .collect::<Vec<_>>();
        for (edge_index, edge) in graph.edges.iter().enumerate() {
            observer.visit_graph_edge_for_endpoint_plan();
            let Some(segment) = edge.compound_segment else {
                continue;
            };
            let port = match segment {
                CompoundEdgeSegment::Output { .. } => edge.source,
                CompoundEdgeSegment::Input { .. } => edge.target,
            };
            if graph
                .layerless_nodes
                .get(port.node)
                .is_some_and(|node| node.compound)
            {
                by_node[port.node].push(CompoundEndpoint {
                    port,
                    edge: edge_index,
                });
            }
        }
        Self { by_node }
    }

    fn for_node(&self, node_index: usize) -> &[CompoundEndpoint] {
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
        for (dummy_node, node) in graph.layerless_nodes.iter().enumerate() {
            observer.visit_endpoint_nested_node();
            if node.kind != LNodeKind::ExternalPort || node.id != dummy_id {
                continue;
            }
            let Some(port) = node.ports.first() else {
                continue;
            };
            let metadata = ExternalDummyMetadata {
                dummy_node,
                external_port_side: node.external_port_side,
                border_offset: port.border_offset,
            };
            for port in &node.ports {
                for edge_index in port
                    .incoming_edges
                    .iter()
                    .chain(port.outgoing_edges.iter())
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
) -> usize {
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
        .add_edge(LayeredEdge {
            id: edge.id.clone(),
            source,
            target,
            source_node_id: edge.source_node_id.clone(),
            target_node_id: edge.target_node_id.clone(),
            labels,
            minlen: edge.minlen,
            reversed: false,
            bend_points: Vec::new(),
            junction_points: Vec::new(),
            model_order: edge.model_order,
            priority_direction: edge.priority_direction,
            priority_shortness: edge.priority_shortness,
            priority_straightness: edge.priority_straightness,
            // `createDummyEdge` copies the original edge's properties, so the thickness is ELK's default.
            thickness: 1.0,
            original_opposite_port: None,
            compound_segment: Some(pending.segment),
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
            .is_some_and(|node| node.kind == LNodeKind::ExternalPort)
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
    let mut recorded_edges = vec![false; graph.edges.len()];
    for record in &graph.cross_hierarchy_edges {
        observer.visit_existing_cross_hierarchy_edge();
        if let Some(recorded) = recorded_edges.get_mut(record.edge) {
            *recorded = true;
        }
    }

    for (edge_index, recorded_edge) in recorded_edges.iter_mut().enumerate() {
        let Some(segment) = graph.edges[edge_index].compound_segment else {
            continue;
        };
        observer.process_segment_edge();
        if !*recorded_edge {
            let edge_id = graph.edges[edge_index].id.clone();
            let model_order = graph.edges[edge_index].model_order;
            record_cross_hierarchy_edge_segment(graph, edge_id, model_order, edge_index, segment);
            *recorded_edge = true;
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
    original_constraints: &[Vec<PortConstraints>],
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
    original_constraints: &[PortConstraints],
    net_flows: &HashMap<OriginalNetFlowKey, isize>,
) {
    for (node_index, &parent_constraints) in original_constraints.iter().enumerate() {
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
        let port_ids = graph.layerless_nodes[node_index]
            .ports
            .ids()
            .collect::<Vec<_>>();

        for port_index in port_ids {
            if graph.layerless_nodes[node_index].ports[port_index]
                .port_dummy
                .is_some()
            {
                continue;
            }

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
            let mut dummy = create_external_port_dummy(
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
            );
            apply_port_index_to_dummy(&mut dummy, port_constraints, port.port_index);
            dummy.parent_port_key = Some(port.id.clone());
            dummy.parent_port_type = Some(port.port_type);
            let dummy_side = dummy.external_port_side;
            let dummy_border_offset = port.border_offset;
            let dummy_node = nested_graph.layerless_nodes.len();
            dummy.ports[PortId::new(0)].node = dummy_node;
            nested_graph.layerless_nodes.push(dummy);

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
    }
}

#[derive(Debug, Clone)]
struct ExternalDummyInfo {
    dummy_node: usize,
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
    // Where each nested graph lies: the graph and the node of the graph it is inside.
    let mut parents = vec![None; graphs.len()];
    let preorder = graphs.preorder(GraphId::ROOT);
    for &graph in &preorder {
        for (node, data) in graphs[graph].layerless_nodes.iter().enumerate() {
            if let Some(nested) = data.nested_graph {
                parents[nested.index()] = Some((graph, node));
            }
        }
    }
    // Each nested graph, in the order of a walk that finishes one graph's subtree before the next
    // sibling.
    for &child in preorder.iter().skip(1) {
        let (parent, parent_node) =
            parents[child.index()].expect("every nested graph lies inside a node");
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
    node_index: usize,
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
        .layerless_nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.kind == LNodeKind::ExternalPort && node.id == dummy_id)
        .filter_map(|(dummy_node, node)| {
            let port = node.ports.first()?;
            Some(ExternalDummyInfo {
                dummy_node,
                port_type: port.port_type,
                parent_port_key: node
                    .parent_port_key
                    .clone()
                    .unwrap_or_else(|| port.id.clone()),
                parent_port_type: node.parent_port_type.unwrap_or(port.port_type),
                external_port_side: node.external_port_side,
                border_offset: port.border_offset,
                origin_port: node.origin_port,
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
    parent_node: usize,
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
    let mut dummy = create_external_port_dummy(
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
        graph.options.direction,
    );
    apply_port_index_to_dummy(
        &mut dummy,
        parent_constraints,
        named.and_then(|named| named.port_index),
    );
    let node = graph.layerless_nodes.len();
    dummy.parent_port_key = (!parent_port_key.is_empty()).then(|| parent_port_key.to_string());
    dummy.parent_port_type = if connects_parent_node {
        Some(parent_port_type)
    } else {
        Some(dummy_port_type)
    };
    dummy.ports[PortId::new(0)].node = node;
    graph.layerless_nodes.push(dummy);
    PortRef {
        node,
        port: PortId::new(0),
    }
}

/// A port that holds only a position and a size, for [`calc_port_side`] and the port offset.
///
/// ELK's `ElkUtil.calcPortSide` and `LGraphUtil.calcPortOffset` read these from the port they
/// get. Here they take an `LPort`, and the caller has no such port yet.
pub(crate) fn port_at(id: &str, x: f64, y: f64, width: f64, height: f64) -> LPort {
    let mut port = LPort::new(id, 0, PortType::Output);
    port.position = LPoint { x, y };
    port.size = LSize { width, height };
    port
}

pub(crate) fn calc_port_side(port: &LPort, node_size: LSize, direction: ElkDirection) -> PortSide {
    let node_width = node_size.width;
    let node_height = node_size.height;
    if node_width <= 0.0 && node_height <= 0.0 {
        return PortSide::Undefined;
    }

    let xpos = port.position.x;
    let ypos = port.position.y;
    let width = port.size.width;
    let height = port.size.height;

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

fn common_graph_depth<Left, Right>(left: &[Left], right: &[Right]) -> usize
where
    Left: AsRef<str>,
    Right: AsRef<str>,
{
    left.iter()
        .zip(right.iter())
        .take_while(|(left, right)| left.as_ref() == right.as_ref())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::LPoint;
    use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputNode, import_graph};
    use crate::options::{HierarchyHandling, LayeredOptions, NodeLabelPlacement};

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
            node_label_placement: NodeLabelPlacement::FIXED,
            node_flexibility: crate::options::NodeFlexibility::None,
            ports_surrounding: None,
            nested_options: None,
            labels: Vec::new(),
            node_size: Default::default(),
            self_loop_distribution: Default::default(),
            self_loop_ordering: Default::default(),
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

    fn graph(nodes: Vec<ElkInputNode>, edges: Vec<ElkInputEdge>) -> ElkInputGraph {
        ElkInputGraph {
            id: "root".to_string(),
            options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
            nodes,
            edges,
        }
    }

    fn compound_fanout_graph(edge_count: usize) -> ElkInputGraph {
        let mut group = node("group");
        group.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut nodes = vec![group, node("outer")];
        let mut edges = Vec::with_capacity(edge_count);
        for index in 0..edge_count {
            let leaf_id = format!("leaf-{index}");
            let mut leaf = node(&leaf_id);
            leaf.parent = Some("group".to_string());
            nodes.push(leaf);
            edges.push(edge(&format!("edge-{index}"), &leaf_id, "outer"));
        }
        let mut input = graph(nodes, edges);
        input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
        input.options.merge_hierarchy_edges = false;
        input
    }

    fn wide_compound_sibling_graph(sibling_count: usize) -> ElkInputGraph {
        let mut nodes = Vec::with_capacity(sibling_count * 2 + 1);
        for index in 0..sibling_count {
            let group_id = format!("group-{index}");
            let leaf_id = format!("leaf-{index}");
            let mut group = node(&group_id);
            group.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
            let mut leaf = node(&leaf_id);
            leaf.parent = Some(group_id);
            nodes.push(group);
            nodes.push(leaf);
        }
        nodes.push(node("outer"));
        let mut input = graph(nodes, vec![edge("crossing", "leaf-0", "outer")]);
        input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
        input.options.merge_hierarchy_edges = false;
        input
    }

    fn compound_observed_work(input: &ElkInputGraph) -> CompoundPreprocessWork {
        let mut graph = import_graph(input).expect("compound fixture import");
        let mut work = CompoundPreprocessWork::default();
        preprocess_source_ported_compound_graph_observed(&mut graph, &mut work);
        work
    }

    #[test]
    fn source_ported_segments_sort_like_cross_hierarchy_edge_comparator() {
        let segments = source_ported_cross_hierarchy_segments(
            "A",
            "B",
            "A:source",
            "B:target",
            &["outer", "inner"],
            &["sibling"],
        );

        let mut sorted = segments
            .iter()
            .map(|segment| segment.segment)
            .collect::<Vec<_>>();
        sorted.sort_by(|left, right| compare_compound_segments(*left, *right));

        assert_eq!(
            sorted,
            vec![
                CompoundEdgeSegment::Output { depth: 2 },
                CompoundEdgeSegment::Output { depth: 1 },
                CompoundEdgeSegment::Output { depth: 0 },
                CompoundEdgeSegment::Input { depth: 1 },
            ]
        );
    }

    #[test]
    fn source_ported_segments_mark_parent_boundary_endpoints() {
        let segments = source_ported_cross_hierarchy_segments(
            "A",
            "B",
            "A:source",
            "B:target",
            &["outer", "inner"],
            &["sibling"],
        );

        assert!(matches!(
            &segments[0].target,
            PendingSegmentEndpoint::ParentBoundary {
                node_id,
                port_key,
                connects_parent_node: false,
                ..
            } if node_id == "inner" && port_key == "A:source"
        ));
        assert!(matches!(
            &segments[2].target,
            PendingSegmentEndpoint::LocalNode { node_id, port_key }
                if node_id == "sibling" && port_key == "B:target"
        ));
        assert!(matches!(
            &segments[3].source,
            PendingSegmentEndpoint::ParentBoundary {
                node_id,
                port_key,
                connects_parent_node: false,
                ..
            } if node_id == "sibling" && port_key == "B:target"
        ));
    }

    // Mutation: vote by the dummy's port type alone (`votes_output = !leaves_port`), as a port
    // with one edge would allow. The shared port then reads -1 instead of 1.
    #[test]
    fn edges_that_share_a_parent_port_vote_together() {
        let edge = |id: &str,
                    source: &str,
                    source_key: &str,
                    source_path: &[&str],
                    target: &str,
                    target_key: &str,
                    target_path: &[&str]| HierarchyEdge {
            id: id.into(),
            source_node_id: source.into(),
            target_node_id: target.into(),
            source_port_key: source_key.into(),
            target_port_key: target_key.into(),
            source_port_named: false,
            target_port_named: false,
            source_path: source_path.iter().map(|id| id.to_string()).collect(),
            target_path: target_path.iter().map(|id| id.to_string()).collect(),
            labels: Vec::new(),
            minlen: 1,
            model_order: None,
            priority_direction: 0,
            priority_shortness: 0,
            priority_straightness: 0,
        };
        let edges = [
            // Leaves the port for a direct child: votes "input".
            edge("e1", "P", "P:shared", &[], "c", "c:in", &["P"]),
            // Leaves the port for a grandchild: votes "output".
            edge("e2", "P", "P:shared", &[], "g", "g:in", &["P", "q"]),
            // Enters the port from a direct child: votes "output".
            edge("e3", "c", "c:out", &["P"], "P", "P:shared", &[]),
            // Another port that only feeds a direct child.
            edge("e4", "P", "P:other", &[], "c", "c:in2", &["P"]),
        ];
        let segments = edges
            .iter()
            .flat_map(materialize_hierarchy_edge_segments)
            .collect::<Vec<_>>();

        let flows = parent_port_net_flows(&segments);

        assert_eq!(flows.len(), 2);
        assert_eq!(flows[&("P".to_string(), "P:shared".to_string())], 1);
        assert_eq!(flows[&("P".to_string(), "P:other".to_string())], -1);
    }

    // Mutation: in `original_net_flows`, key the source of a hierarchy edge as a made-up port
    // (`false` for `edge.source_port_named`). The edge out of the named port `G:2` then counts at
    // the made-up port `G:2`, and the named port reads 0 instead of -1.
    #[test]
    fn a_made_up_port_keeps_its_net_flow_apart_from_a_named_port_of_the_same_id() {
        let port = |id: &str| crate::importer::ElkInputPort {
            id: id.to_string(),
            x: 0.0,
            y: 0.0,
            width: 6.0,
            height: 6.0,
            side: crate::graph::PortSide::West,
            index: None,
            border_offset: None,
            net_flow: 0,
            child_edges: 0,
            connected_to_external_nodes: false,
            allow_non_flow_ports_to_switch_sides: false,
        };
        let mut group = node("G");
        group.ports = vec![port("G:1"), port("G:2")];
        let mut child = node("c");
        child.parent = Some("G".to_string());
        // The named port `G:2` leaves for its child: net flow -1.
        let mut leaving = edge("e1", "G", "c");
        leaving.source_port = Some("G:2".to_string());
        leaving.container = Some("root".to_string());
        // `s` enters `G` at a made-up port. G has two ports, so ELK's `createPort` would add a
        // third, and this importer calls it `G:2`, too. Net flow +1.
        let entering = edge("e2", "s", "G");
        let mut input = graph(vec![node("s"), group, child], vec![leaving, entering]);
        input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
        let lgraph = import_graph(&input).unwrap();

        let flows = original_net_flows(&lgraph);

        assert_eq!(flows[&("G".to_string(), "G:2".to_string(), true)], -1);
        assert_eq!(flows[&("G".to_string(), "G:2".to_string(), false)], 1);
    }

    #[test]
    fn compound_label_buckets_preserve_order_identity_and_single_ownership() {
        for depth in [1, 16, 128] {
            let edge = HierarchyEdge {
                id: "ab".into(),
                source_node_id: "A".into(),
                target_node_id: "B".into(),
                source_port_key: "A:source".into(),
                target_port_key: "B:target".into(),
                source_port_named: false,
                target_port_named: false,
                source_path: (0..depth).map(|index| format!("group-{index}")).collect(),
                target_path: vec!["other".into()],
                labels: (0..4096)
                    .map(|index| {
                        let mut label = LLabel::new(index.to_string(), 12.0, 10.0);
                        label.placement = [
                            EdgeLabelPlacement::Tail,
                            EdgeLabelPlacement::Center,
                            EdgeLabelPlacement::Head,
                        ][index % 3];
                        label.source_index = Some(index);
                        label
                    })
                    .collect(),
                minlen: 1,
                model_order: None,
                priority_direction: 0,
                priority_shortness: 0,
                priority_straightness: 0,
            };
            let output = materialize_hierarchy_edge_segments(&edge);
            let mut seen = vec![false; edge.labels.len()];
            for segment in output {
                let indices = segment
                    .labels
                    .iter()
                    .map(|label| label.source_index.unwrap())
                    .collect::<Vec<_>>();
                assert!(indices.windows(2).all(|pair| pair[0] < pair[1]));
                for label in segment.labels {
                    let index = label.source_index.unwrap();
                    assert!(!seen[index]);
                    seen[index] = true;
                    assert_eq!(label.original_label_edge.as_deref(), Some("ab"));
                }
            }
            assert!(seen.into_iter().all(|seen| seen));
        }
    }

    #[test]
    fn center_label_uses_shallowest_segment() {
        let segments = source_ported_cross_hierarchy_segments(
            "A",
            "B",
            "A:source",
            "B:target",
            &["outer", "inner"],
            &["sibling"],
        );

        let label_index = compound_label_segment_index(&segments, EdgeLabelPlacement::Center);

        assert_eq!(
            segments[label_index].segment,
            CompoundEdgeSegment::Output { depth: 0 }
        );
    }

    #[test]
    fn allocation_free_label_selection_matches_stable_sorted_identity() {
        let segments = source_ported_cross_hierarchy_segments(
            "A",
            "B",
            "A:source",
            "B:target",
            &["outer", "inner"],
            &["sibling"],
        );
        let reference = |placement| {
            let mut sorted = segments
                .iter()
                .enumerate()
                .map(|(index, segment)| (index, segment.segment))
                .collect::<Vec<_>>();
            sorted.sort_by(|(_, left), (_, right)| compare_compound_segments(*left, *right));
            match placement {
                EdgeLabelPlacement::Tail => sorted.first().map(|(index, _)| *index).unwrap_or(0),
                EdgeLabelPlacement::Head => sorted.last().map(|(index, _)| *index).unwrap_or(0),
                EdgeLabelPlacement::Center => sorted
                    .iter()
                    .position(|(_, segment)| matches!(segment, CompoundEdgeSegment::Input { .. }))
                    .map(|index| index.saturating_sub(1))
                    .or_else(|| sorted.len().checked_sub(1))
                    .and_then(|index| sorted.get(index).map(|(segment_index, _)| *segment_index))
                    .unwrap_or(0),
            }
        };

        for placement in [
            EdgeLabelPlacement::Tail,
            EdgeLabelPlacement::Center,
            EdgeLabelPlacement::Head,
        ] {
            assert_eq!(
                compound_label_segment_index(&segments, placement),
                reference(placement)
            );
        }
    }

    #[test]
    fn compound_segment_registration_and_endpoint_indexing_are_linear() {
        let mut previous_total = None;
        for edge_count in [1usize, 8, 64, 256] {
            let work = compound_observed_work(&compound_fanout_graph(edge_count));
            assert_eq!(
                work.existing_cross_hierarchy_edges_indexed,
                work.segment_edges_processed
            );
            assert!(work.endpoint_nested_nodes_indexed > 0);
            assert!(work.endpoint_incident_edges_indexed > 0);

            let total = work.graph_edges_planned
                + work.existing_cross_hierarchy_edges_indexed
                + work.endpoint_nested_nodes_indexed
                + work.endpoint_incident_edges_indexed
                + work.segment_edges_processed;
            if let Some((previous_edges, previous_total)) = previous_total {
                assert!(
                    total * previous_edges <= previous_total * edge_count + 8 * edge_count,
                    "compound preprocessing work grew faster than its owned edge/node payload"
                );
            }
            previous_total = Some((edge_count, total));
        }
    }

    #[test]
    fn compound_endpoint_link_uses_exact_port_identity_when_ids_repeat() {
        let mut graph = import_graph(&compound_fanout_graph(1)).expect("compound fixture import");
        introduce_source_ported_hierarchy_edge_segments(&mut graph);
        let group = graph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "group")
            .expect("compound parent node");
        let edge_index = graph
            .edges
            .iter()
            .position(|edge| {
                edge.compound_segment.is_some()
                    && (edge.source.node == group || edge.target.node == group)
            })
            .expect("root compound segment");
        let original_port = if graph.edges[edge_index].source.node == group {
            graph.edges[edge_index].source
        } else {
            graph.edges[edge_index].target
        };
        let mut duplicate = graph.layerless_nodes[group].ports[original_port.port].clone();
        duplicate.incoming_edges.clear();
        duplicate.outgoing_edges.clear();
        duplicate.port_dummy = None;
        let duplicate_port = PortRef {
            node: group,
            port: PortId::new(graph.layerless_nodes[group].ports.len()),
        };
        graph.layerless_nodes[group].ports.push(duplicate);
        if graph.edges[edge_index].source == original_port {
            assert!(graph.set_edge_source(edge_index, duplicate_port));
        } else {
            assert!(graph.set_edge_target(edge_index, duplicate_port));
        }

        preprocess_source_ported_compound_graph(&mut graph);

        let nested = graph.layerless_nodes[group]
            .nested_graph
            .map(|id| &graph[id])
            .expect("compound child graph");
        let incident_dummy = nested
            .layerless_nodes
            .iter()
            .enumerate()
            .find(|(_, node)| {
                node.kind == LNodeKind::ExternalPort
                    && node.ports.iter().any(|port| {
                        port.incoming_edges
                            .iter()
                            .chain(port.outgoing_edges.iter())
                            .any(|edge| nested.edges[*edge].id == "edge-0")
                    })
            })
            .map(|(index, _)| index)
            .expect("incident external dummy");
        assert_eq!(
            nested.layerless_nodes[incident_dummy]
                .origin_port
                .as_ref()
                .map(|origin| origin.port),
            Some(duplicate_port)
        );
        assert_eq!(
            graph.layerless_nodes[group].ports[duplicate_port.port]
                .port_dummy
                .as_ref()
                .map(|dummy| dummy.node),
            Some(incident_dummy)
        );
    }

    #[test]
    fn parent_port_index_is_owner_local_in_wide_hierarchy() {
        for sibling_count in [1usize, 8, 64, 256] {
            let work = compound_observed_work(&wide_compound_sibling_graph(sibling_count));

            // Only group-0 owns an external dummy. Unrelated compound siblings must not trigger
            // whole-parent port-index rebuilds.
            assert_eq!(work.parent_port_indexes_built, 1);
        }
    }

    #[test]
    fn nested_exports_publish_child_ports_and_dummies_before_parent_edges() {
        // Minimal State007 provider oracle, including the reversed input direction:
        // Outer -> After is already local; Deep's export precedes InnerB's own edge.
        for reverse in [false, true] {
            for spacing in [20.0, 40.0] {
                let mut outer = node("Outer");
                outer.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
                let mut inner = node("InnerB");
                inner.parent = Some("Outer".to_string());
                inner.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
                let mut deep = node("Deep");
                deep.parent = Some("InnerB".to_string());
                let mut input = graph(
                    vec![outer, inner, deep, node("After")],
                    ["Outer", "InnerB", "Deep"]
                        .into_iter()
                        .map(|id| {
                            if reverse {
                                edge(id, "After", id)
                            } else {
                                edge(id, id, "After")
                            }
                        })
                        .collect(),
                );
                input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
                input.options.spacing.edge_edge = spacing;
                let mut graph = import_graph(&input).unwrap();
                graph
                    .try_for_each_graph_mut(|graph| {
                        graph.options.spacing.edge_edge = spacing;
                        Ok::<(), std::convert::Infallible>(())
                    })
                    .unwrap();
                preprocess_source_ported_compound_graph(&mut graph);
                let outer = graph
                    .layerless_nodes
                    .iter()
                    .find(|node| node.id == "Outer")
                    .unwrap();
                let incident = |port: &LPort| {
                    if reverse {
                        &port.incoming_edges
                    } else {
                        &port.outgoing_edges
                    }
                    .first()
                    .copied()
                };
                let published = outer
                    .ports
                    .iter()
                    .filter_map(incident)
                    .map(|edge| graph.edges[edge].id.as_str())
                    .collect::<Vec<_>>();
                assert_eq!(published, ["Outer", "Deep", "InnerB"]);

                let nested = outer.nested_graph.map(|id| &graph[id]).unwrap();
                let dummy_edges = nested
                    .layerless_nodes
                    .iter()
                    .filter(|node| node.kind == LNodeKind::ExternalPort)
                    .filter_map(|node| {
                        let port = &node.ports[PortId::new(0)];
                        let edge = if reverse {
                            port.outgoing_edges.first()
                        } else {
                            port.incoming_edges.first()
                        }?;
                        assert_eq!(port.border_offset, Some(spacing / 2.0));
                        let origin = node.origin_port.as_ref().unwrap().port;
                        let parent = &graph.layerless_nodes[origin.node].ports[origin.port];
                        assert_eq!(parent.border_offset, Some(spacing / 2.0));
                        assert_eq!(
                            graph.edges[incident(parent).unwrap()].id,
                            nested.edges[*edge].id
                        );
                        Some(nested.edges[*edge].id.as_str())
                    })
                    .collect::<Vec<_>>();
                assert_eq!(dummy_edges, ["Deep", "InnerB"]);
                for edge in &graph.edges {
                    let (source, target) = if reverse {
                        ("After", edge.id.as_str())
                    } else {
                        (edge.id.as_str(), "After")
                    };
                    assert_eq!(edge.source_node_id, source);
                    assert_eq!(edge.target_node_id, target);
                }
            }
        }
    }

    #[test]
    fn nested_publication_keeps_child_graph_then_local_node_order() {
        let mut outer = node("Outer");
        outer.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut first = node("First");
        first.parent = Some("Outer".to_string());
        let mut inner = node("Inner");
        inner.parent = Some("Outer".to_string());
        inner.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut deep = node("Deep");
        deep.parent = Some("Inner".to_string());
        let mut last = node("Last");
        last.parent = Some("Outer".to_string());
        let mut input = graph(
            vec![outer, first, inner, deep, last, node("After")],
            vec![
                edge("Last", "Last", "After"),
                edge("First", "First", "After"),
                edge("Deep", "Deep", "After"),
            ],
        );
        input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
        let mut graph = import_graph(&input).unwrap();
        preprocess_source_ported_compound_graph(&mut graph);
        let outer = graph
            .layerless_nodes
            .iter()
            .find(|node| node.id == "Outer")
            .unwrap();
        let publication = outer
            .ports
            .iter()
            .filter_map(|port| port.outgoing_edges.first())
            .map(|edge| graph.edges[*edge].id.as_str())
            .collect::<Vec<_>>();
        // All child exports precede direct nodes, including First which occurs before Inner.
        // The direct nodes then follow node order, not the reversed edge declaration order.
        assert_eq!(publication, ["Deep", "First", "Last"]);
    }

    #[test]
    fn sibling_groups_publish_each_end_in_its_own_child_order() {
        let mut nodes = Vec::new();
        for (group_id, children) in [("Left", ["A", "B"]), ("Right", ["C", "D"])] {
            let mut group = node(group_id);
            group.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
            nodes.push(group);
            for child_id in children {
                let mut child = node(child_id);
                child.parent = Some(group_id.to_string());
                nodes.push(child);
            }
        }
        let mut input = graph(nodes, vec![edge("AD", "A", "D"), edge("BC", "B", "C")]);
        input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
        let mut graph = import_graph(&input).unwrap();
        preprocess_source_ported_compound_graph(&mut graph);
        for (id, expected) in [("Left", ["AD", "BC"]), ("Right", ["BC", "AD"])] {
            let group = graph
                .layerless_nodes
                .iter()
                .find(|node| node.id == id)
                .unwrap();
            let published = group
                .ports
                .iter()
                .map(|port| {
                    let edge = port
                        .outgoing_edges
                        .first()
                        .or_else(|| port.incoming_edges.first())
                        .unwrap();
                    graph.edges[*edge].id.as_str()
                })
                .collect::<Vec<_>>();
            assert_eq!(published, expected, "{id} publishes its own child ports");
        }
    }

    #[test]
    fn scoped_publication_uses_local_input_side_without_changing_absolute_side() {
        for source_is_boundary in [false, true] {
            let mut inner = node("Inner");
            inner.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
            let mut a = node("A");
            a.parent = Some("Inner".to_string());
            let mut b = node("B");
            b.parent = Some("Inner".to_string());
            let mut input = graph(vec![node("Outside"), inner, a, b], Vec::new());
            input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
            let mut graph = import_graph(&input).unwrap();
            let mut segments = Vec::new();
            for target in ["B", "A"] {
                let edge = HierarchyEdge {
                    id: target.to_string(),
                    source_node_id: "Outside".to_string(),
                    target_node_id: target.to_string(),
                    source_port_key: "Outside:shared".to_string(),
                    target_port_key: format!("{target}:input"),
                    source_port_named: false,
                    target_port_named: false,
                    source_path: Vec::new(),
                    target_path: vec!["Inner".to_string()],
                    labels: Vec::new(),
                    minlen: 1,
                    model_order: None,
                    priority_direction: 0,
                    priority_shortness: 0,
                    priority_straightness: 0,
                };
                for mut scoped in materialize_hierarchy_edge_segments(&edge) {
                    let depth = match scoped.pending.segment {
                        CompoundEdgeSegment::Input { depth }
                        | CompoundEdgeSegment::Output { depth } => depth,
                    };
                    scoped.pending.segment = CompoundEdgeSegment::Output { depth };
                    if source_is_boundary && scoped.pending.graph_parent.is_none() {
                        scoped.pending.source = PendingSegmentEndpoint::ParentBoundary {
                            node_id: "Outside".to_string(),
                            port_key: "Outside:shared".to_string(),
                            port_type: PortType::Input,
                            parent_port_type: PortType::Input,
                            connects_parent_node: false,
                        };
                    }
                    segments.push(scoped);
                }
            }
            introduce_source_ported_scoped_edge_segments(&mut graph, GraphId::ROOT, segments);
            assert_eq!(
                graph
                    .edges
                    .iter()
                    .map(|edge| edge.id.as_str())
                    .collect::<Vec<_>>(),
                ["A", "B"]
            );
            let inner = graph
                .layerless_nodes
                .iter()
                .find(|node| node.id == "Inner")
                .unwrap()
                .nested_graph
                .map(|id| &graph[id])
                .unwrap();
            assert_eq!(
                inner
                    .edges
                    .iter()
                    .map(|edge| edge.id.as_str())
                    .collect::<Vec<_>>(),
                ["A", "B"]
            );
            for graph in [&graph, inner] {
                assert!(graph.edges.iter().all(|edge| matches!(
                    edge.compound_segment,
                    Some(CompoundEdgeSegment::Output { .. })
                )));
            }
        }
    }

    fn parent_and_crossing_edge_graph(
        reverse: bool,
        spacing: f64,
        original_offset: Option<f64>,
    ) -> (GraphArena, String) {
        let mut group = node("group");
        group.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut child = node("child");
        child.parent = Some("group".to_string());
        let edges = if reverse {
            vec![
                edge("direct", "child", "group"),
                edge("crossing", "child", "outside"),
            ]
        } else {
            vec![
                edge("direct", "group", "child"),
                edge("crossing", "outside", "child"),
            ]
        };
        let mut input = graph(vec![group, child, node("outside")], edges);
        input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
        input.options.merge_hierarchy_edges = false;
        input.options.spacing.edge_edge = spacing;
        let mut graph = import_graph(&input).unwrap();
        let direct = graph
            .hierarchy_edges
            .iter()
            .find(|edge| edge.id == "direct")
            .unwrap();
        let original_key = if reverse {
            direct.target_port_key.clone()
        } else {
            direct.source_port_key.clone()
        };
        let group = graph
            .layerless_nodes
            .iter_mut()
            .find(|node| node.id == "group")
            .unwrap();
        group
            .ports
            .slots_mut()
            .find(|port| port.id == original_key)
            .unwrap()
            .border_offset = original_offset;
        let nested = group.nested_graph.unwrap();
        graph[nested].options.spacing.edge_edge = spacing;
        (graph, original_key)
    }

    #[test]
    fn parent_border_offsets_survive_alongside_crossing_spacing_margins() {
        // Captured elkjs oracle: spacing 20/40 never changes a direct parent's offset;
        // original port offsets +7/-4 survive, while exported crossing ports keep spacing/2.
        for reverse in [false, true] {
            for spacing in [20.0, 40.0] {
                for original_offset in [None, Some(7.0), Some(-4.0)] {
                    let (mut graph, key) =
                        parent_and_crossing_edge_graph(reverse, spacing, original_offset);
                    let group_index = graph
                        .layerless_nodes
                        .iter()
                        .position(|node| node.id == "group")
                        .unwrap();
                    let original_port = graph.layerless_nodes[group_index]
                        .ports
                        .iter_with_ids()
                        .find(|(_, port)| port.id == key)
                        .map(|(id, _)| id)
                        .unwrap();
                    preprocess_source_ported_compound_graph(&mut graph);
                    let group = &graph.layerless_nodes[group_index];
                    let nested = group.nested_graph.map(|id| &graph[id]).unwrap();
                    let parent = &group.ports[original_port];
                    let dummy_ref = parent.port_dummy.as_ref().unwrap();
                    let direct = &nested.layerless_nodes[dummy_ref.node];
                    assert_eq!(
                        direct.origin_port.as_ref().unwrap().port,
                        PortRef {
                            node: group_index,
                            port: original_port
                        }
                    );
                    assert_eq!(parent.border_offset, Some(original_offset.unwrap_or(0.0)));
                    assert_eq!(
                        direct.ports[PortId::new(0)].border_offset,
                        Some(original_offset.unwrap_or(0.0))
                    );
                    if original_offset == Some(-4.0) {
                        let depth = match direct.external_port_side {
                            PortSide::North | PortSide::South => direct.size.height,
                            PortSide::East | PortSide::West => direct.size.width,
                            PortSide::Undefined => panic!("external port side must be resolved"),
                        };
                        assert_eq!(
                            depth, 4.0,
                            "negative offsets must size the dummy before layout"
                        );
                    }
                    let crossing = nested
                        .edges
                        .iter()
                        .find(|edge| edge.id == "crossing")
                        .unwrap();
                    let boundary = if reverse {
                        crossing.target
                    } else {
                        crossing.source
                    };
                    let crossing_dummy = &nested.layerless_nodes[boundary.node];
                    assert_eq!(crossing_dummy.kind, LNodeKind::ExternalPort);
                    assert_ne!(boundary.node, dummy_ref.node);
                    assert_eq!(
                        crossing_dummy.ports[boundary.port].border_offset,
                        Some(spacing / 2.0)
                    );
                    let crossing_parent = crossing_dummy.origin_port.as_ref().unwrap().port;
                    assert_eq!(
                        graph.layerless_nodes[crossing_parent.node].ports[crossing_parent.port]
                            .border_offset,
                        Some(spacing / 2.0)
                    );
                }
            }
        }
    }

    #[test]
    fn routed_parent_ports_keep_their_original_border_offset() {
        for reverse in [false, true] {
            for original_offset in [None, Some(7.0), Some(-4.0)] {
                let (mut graph, key) =
                    parent_and_crossing_edge_graph(reverse, 40.0, original_offset);
                crate::pipeline::execute_ported_compound_processors(&mut graph).unwrap();
                let group = graph
                    .layerless_nodes
                    .iter()
                    .find(|node| node.id == "group")
                    .unwrap();
                let port = group.ports.iter().find(|port| port.id == key).unwrap();
                let anchor = LPoint {
                    x: port.position.x + port.anchor.x,
                    y: port.position.y + port.anchor.y,
                };
                let offset = match port.side {
                    PortSide::North => -anchor.y,
                    PortSide::South => anchor.y - group.size.height,
                    PortSide::West => -anchor.x,
                    PortSide::East => anchor.x - group.size.width,
                    PortSide::Undefined => panic!("routed parent port has no side"),
                };
                assert!(
                    (offset - original_offset.unwrap_or(0.0)).abs() < 1e-9,
                    "reverse={reverse}, requested={original_offset:?}, actual={offset}"
                );
            }
        }
    }

    #[test]
    fn parent_end_external_dummy_uses_fixed_parent_port_side() {
        let mut cluster = node("cluster");
        cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        cluster.port_constraints = Some(PortConstraints::FixedSide);
        let mut child = node("A");
        child.parent = Some("cluster".to_string());

        let mut lgraph = import_graph(&graph(
            vec![cluster, child],
            vec![edge("cluster-A", "cluster", "A")],
        ))
        .unwrap();
        let cluster_index = lgraph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "cluster")
            .unwrap();
        {
            let parent_port = &mut lgraph.layerless_nodes[cluster_index].ports[PortId::new(0)];
            parent_port.set_side(PortSide::East);
            parent_port.position = LPoint { x: 80.0, y: 20.0 };
            parent_port.size = LSize {
                width: 6.0,
                height: 8.0,
            };
            parent_port.border_offset = Some(-4.0);
        }

        preprocess_source_ported_compound_graph(&mut lgraph);

        let cluster = &lgraph.layerless_nodes[cluster_index];
        let parent_port = &cluster.ports[PortId::new(0)];
        let port_dummy = parent_port
            .port_dummy
            .as_ref()
            .expect("parent port should link to the nested external dummy");
        let nested = cluster.nested_graph.map(|id| &lgraph[id]).unwrap();
        let external = &nested.layerless_nodes[port_dummy.node];

        assert_eq!(parent_port.side, PortSide::South);
        assert_eq!(external.external_port_side, PortSide::South);
        assert_eq!(
            external.parent_port_key.as_deref(),
            Some("cluster:0:source")
        );
        assert_eq!(external.parent_port_type, Some(PortType::Output));
        assert_eq!(external.ports[PortId::new(0)].side, PortSide::North);
    }

    #[test]
    fn parent_end_external_dummy_uses_parent_port_net_flow() {
        let mut cluster = node("cluster");
        cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut child = node("A");
        child.parent = Some("cluster".to_string());

        let mut lgraph = import_graph(&graph(
            vec![cluster, child],
            vec![edge("cluster-A", "cluster", "A")],
        ))
        .unwrap();
        let cluster_index = lgraph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "cluster")
            .unwrap();

        preprocess_source_ported_compound_graph(&mut lgraph);

        let cluster = &lgraph.layerless_nodes[cluster_index];
        let parent_port = &cluster.ports[PortId::new(0)];
        assert_eq!(parent_port.net_flow(), 0);
        let port_dummy = parent_port
            .port_dummy
            .as_ref()
            .expect("parent port should link to the nested external dummy");
        let nested = cluster.nested_graph.map(|id| &lgraph[id]).unwrap();
        let external = &nested.layerless_nodes[port_dummy.node];

        assert_eq!(external.parent_port_type, Some(PortType::Output));
        assert_eq!(external.external_port_side, PortSide::North);
        assert_eq!(external.ports[PortId::new(0)].side, PortSide::South);
    }

    // Mutation: read the port constraints of S2 after the sibling's dummy is linked, so S2 is
    // fixed by side and its unlinked port keeps the side it got from the direction.
    #[test]
    fn unlinked_parent_port_uses_net_flow_after_sibling_external_port_fixing() {
        let mut s1 = node("S1");
        s1.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut sub1 = node("sub1");
        sub1.parent = Some("S1".to_string());
        let mut s2 = node("S2");
        s2.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut sub4 = node("sub4");
        sub4.parent = Some("S2".to_string());

        let mut lgraph = import_graph(&graph(
            vec![s1, sub1, s2, sub4],
            vec![edge("S1-S2", "S1", "S2"), edge("sub1-sub4", "sub1", "sub4")],
        ))
        .unwrap();
        let s2_index = lgraph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "S2")
            .unwrap();

        preprocess_source_ported_compound_graph(&mut lgraph);

        let s2 = &lgraph.layerless_nodes[s2_index];
        let parent_port = s2
            .ports
            .iter()
            .find(|port| port.id == "S2:0" && port.port_type == PortType::Input)
            .expect("top-level edge target port should exist on S2");
        let port_dummy = parent_port
            .port_dummy
            .as_ref()
            .expect("S2 top-level target port should link to nested external dummy");
        let nested = s2.nested_graph.map(|id| &lgraph[id]).unwrap();
        let external = &nested.layerless_nodes[port_dummy.node];

        // One edge enters the port, so the flow is -1 for the dummy. The layout flows down, so
        // the dummy goes to the north, whatever side the port had before.
        assert_eq!(parent_port.net_flow(), 1);
        assert_eq!(parent_port.side, PortSide::North);
        assert_eq!(external.external_port_side, PortSide::North);
    }

    #[test]
    fn nested_graph_hierarchy_edges_resolve_their_relative_root_scope() {
        let mut group = node("group");
        group.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut inner = node("inner");
        inner.parent = Some("group".to_string());
        inner.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut leaf = node("leaf");
        leaf.parent = Some("inner".to_string());
        let mut sibling = node("sibling");
        sibling.parent = Some("group".to_string());
        let mut input = graph(vec![group, inner, leaf, sibling], Vec::new());
        input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
        let mut lgraph = import_graph(&input).unwrap();
        let group_graph = lgraph.layerless_nodes[0]
            .nested_graph
            .expect("group graph is materialized");
        lgraph[group_graph].hierarchy_edges.push(HierarchyEdge {
            id: "leaf-sibling".to_string(),
            source_node_id: "leaf".to_string(),
            target_node_id: "sibling".to_string(),
            source_port_key: "leaf:source".to_string(),
            target_port_key: "sibling:target".to_string(),
            source_port_named: false,
            target_port_named: false,
            source_path: vec!["inner".to_string()],
            target_path: Vec::new(),
            labels: Vec::new(),
            minlen: 1,
            model_order: Some(3),
            priority_direction: 0,
            priority_shortness: 0,
            priority_straightness: 0,
        });

        preprocess_source_ported_compound_graph(&mut lgraph);

        let group_graph = lgraph.layerless_nodes[0]
            .nested_graph
            .map(|id| &lgraph[id])
            .expect("group graph remains materialized");
        assert!(group_graph.hierarchy_edges.is_empty());
        assert_eq!(group_graph.cross_hierarchy_edges.len(), 1);
        assert_eq!(
            group_graph.cross_hierarchy_edges[0].original_model_order,
            Some(3)
        );
        let inner_graph = group_graph.layerless_nodes[0]
            .nested_graph
            .map(|id| &lgraph[id])
            .expect("inner graph remains materialized");
        assert_eq!(inner_graph.cross_hierarchy_edges.len(), 1);
        assert_eq!(
            inner_graph.cross_hierarchy_edges[0].original_model_order,
            Some(3)
        );
    }

    #[test]
    fn compound_external_ports_are_small_stack_safe_for_deep_mixed_hierarchy() {
        const DEPTH: usize = 512;

        std::thread::Builder::new()
            .name("elk-compound-external-port-small-stack".to_string())
            .stack_size(128 * 1024)
            .spawn(|| {
                let mut nodes = Vec::with_capacity(DEPTH + 2);
                for depth in 0..DEPTH {
                    let group_id = format!("group-{depth}");
                    let mut group = node(&group_id);
                    group.parent = depth.checked_sub(1).map(|parent| format!("group-{parent}"));
                    group.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
                    nodes.push(group);
                }
                let mut leaf = node("leaf");
                leaf.parent = Some(format!("group-{}", DEPTH - 1));
                nodes.push(leaf);
                nodes.push(node("outer"));
                let mut input = graph(nodes, vec![edge("leaf-outer", "leaf", "outer")]);
                input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;

                let mut lgraph = std::mem::ManuallyDrop::new(
                    import_graph(&input).expect("deep hierarchy import"),
                );
                // The recursive-layout owner materializes SeparateChildren scopes before they
                // reach this pass. Alternate the retained scope policy after import so this one
                // materialized chain covers the mixed-policy ancestry seen by preprocessing.
                let mut graph_order = 0usize;
                lgraph
                    .try_for_each_graph_mut(|graph| {
                        graph.options.hierarchy_handling = if graph_order.is_multiple_of(2) {
                            HierarchyHandling::IncludeChildren
                        } else {
                            HierarchyHandling::SeparateChildren
                        };
                        graph_order += 1;
                        Ok::<(), std::convert::Infallible>(())
                    })
                    .expect("hierarchy option update is infallible");

                preprocess_source_ported_compound_graph(&mut lgraph);

                let mut current: &LGraph = &lgraph;
                for depth in 0..DEPTH {
                    let group_id = format!("group-{depth}");
                    let group = current
                        .layerless_nodes
                        .iter()
                        .find(|node| node.id == group_id)
                        .expect("every group remains in its parent graph");
                    assert!(
                        group.ports.iter().any(|port| port.port_dummy.is_some()),
                        "group {group_id} must link to its nested external dummy"
                    );
                    current = group
                        .nested_graph
                        .map(|id| &lgraph[id])
                        .expect("every group retains its materialized child graph");
                }
            })
            .expect("small-stack worker should start")
            .join()
            .expect("compound preprocessing must not overflow the worker stack");
    }
}
