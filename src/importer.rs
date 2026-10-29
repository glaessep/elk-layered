//! ELK graph importer scaffold.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/graph/transform/ElkGraphImporter.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/LayeredLayoutProvider.java
//! - https://github.com/mermaid-js/mermaid/blob/7c0cafcf42e76bfaf79d0cbbd12edb986612f014/packages/mermaid-layout-elk/src/render.ts

use std::collections::{HashMap, HashSet, VecDeque};

use crate::common::cells::{self, CellLabel, CellNode};
use crate::compound::{
    PendingCompoundSegment, PendingSegmentEndpoint, ScopedHierarchySegment,
    compound_label_segment_index, introduce_source_ported_scoped_edge_segments, port_at,
    source_ported_cross_hierarchy_segments,
};
use crate::graph::{NodeId, NodeSlice, 
    CompoundEdgeSegment, EdgeLabelPlacement, GraphArena, GraphId, HierarchyEdge, HierarchyStep,
    LGraph, LLabel, LNode, LNodeKind, LPoint, LPort, LSize, LayeredEdge, PortId, PortRef, PortSide,
    PortType, apply_port_index_to_dummy, create_external_port_dummy, is_across_flow,
    port_side_from_direction,
};
use crate::options::{
    CycleBreakingStrategy, ElkDirection, ElkPadding, HierarchyHandling, LayerConstraint,
    LayeredOptions, LayeringStrategy, NodeLabelPlacement, OrderingStrategy, PortConstraints,
};

// `org.eclipse.elk.edge.thickness` defaults to 1 in the pinned elkjs 0.9.3 CoreOptions.
const ELK_DEFAULT_EDGE_THICKNESS: f64 = 1.0;

#[derive(Debug, Clone, PartialEq)]
pub struct ElkInputGraph {
    pub id: String,
    pub options: LayeredOptions,
    pub nodes: Vec<ElkInputNode>,
    pub edges: Vec<ElkInputEdge>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ElkInputNode {
    pub id: String,
    pub width: f64,
    pub height: f64,
    pub parent: Option<String>,
    pub direction: Option<ElkDirection>,
    pub hierarchy_handling: Option<HierarchyHandling>,
    pub layer_constraint: Option<LayerConstraint>,
    pub port_constraints: Option<PortConstraints>,
    pub port_alignment: Option<crate::options::PortAlignment>,
    pub node_label_placement: NodeLabelPlacement,
    pub node_flexibility: crate::options::NodeFlexibility,
    pub ports_surrounding: Option<crate::options::SpacingMargin>,
    /// The node's ports, in input order.
    pub ports: Vec<ElkInputPort>,
    /// Resolved options for the graph inside this node. Absent values use ELK defaults.
    pub nested_options: Option<Box<LayeredOptions>>,
    /// The node's labels, in input order. ELK drops a label with empty text from the layered
    /// graph but keeps it for the node size and label padding computations.
    pub labels: Vec<ElkInputLabel>,
    /// The node's own `nodeSize` options.
    pub node_size: crate::options::NodeSizing,
    /// The node's own `layered.edgeRouting.selfLoopDistribution`.
    pub self_loop_distribution: crate::options::SelfLoopDistributionStrategy,
    /// The node's own `layered.edgeRouting.selfLoopOrdering`.
    pub self_loop_ordering: crate::options::SelfLoopOrderingStrategy,
    /// What the node's parent sets itself. The caller works this out once, so every path that
    /// sizes a node reads the same values.
    pub parent_settings: ParentSettings,
}

/// A port of a node, as `ElkGraphImporter.transformPort` reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct ElkInputPort {
    /// Unique among the ports and nodes of the graph.
    pub id: String,
    /// Left side, relative to the node. `LGraphUtil.initializePort` reads it to find the side of
    /// a port that has none, and `FIXED_POS` and `FIXED_RATIO` keep it.
    pub x: f64,
    /// Top side, relative to the node.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
    /// `elk.port.side`, or `Undefined`.
    pub side: PortSide,
    /// `elk.port.index`, when set.
    pub index: Option<i32>,
    /// `elk.port.borderOffset`, when set.
    pub border_offset: Option<f64>,
    /// `layered.allowNonFlowPortsToSwitchSides`: the crossing minimization may move a port across
    /// the flow to the other side of its node.
    pub allow_non_flow_ports_to_switch_sides: bool,
    /// The net flow of the port: the votes of every edge at the port, those outside its node
    /// too. A positive number makes the port an output port of its node.
    ///
    /// Ports `ElkGraphImporter.calculateNetFlow` and `CompoundGraphPreprocessor.calculateNetFlow`,
    /// which count the same votes.
    pub net_flow: i32,
    /// How many edges at the port connect to a child of the port's node, at most two.
    /// `ElkGraphImporter.checkExternalPorts` counts them.
    pub child_edges: usize,
    /// Whether an edge at the port connects to a node outside the port's node.
    /// Ports `ElkGraphImporter.isConnectedToExternalNodes`.
    pub connected_to_external_nodes: bool,
}

/// What ELK reads from a node's parent when it sizes the node.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParentSettings {
    /// The options the parent sets itself, before the importer touches them. They supply the
    /// label spacings, as `IndividualSpacings.getIndividualOrInherited` reads them.
    pub options: LayeredOptions,
    /// The direction the parent sets itself, or `Undefined`. It decides the
    /// `horizontalLayoutMode` of `calculateMinimumGraphSize`.
    pub direction: ElkDirection,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ElkInputEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub labels: Vec<ElkInputLabel>,
    pub minlen: usize,
    pub inside_self_loops_yo: bool,
    /// Explicit input-model position within the edge's owning hierarchy scope.
    ///
    /// When absent, standalone imports retain the historical filtered-input ordinal.
    pub model_order: Option<usize>,
    pub priority_direction: i32,
    pub priority_shortness: i32,
    pub priority_straightness: i32,
    /// The node that owns the edge, as `ElkGraphUtil.findBestEdgeContainment` chooses it; the
    /// graph's own id for the root. `None` leaves the choice to the importer, which takes the
    /// shared parent or the endpoint that holds the other one, and the root otherwise.
    pub container: Option<String>,
    /// The id of the source node's port that the edge starts at. `None` leaves the choice to
    /// the importer, which makes a port, as `LGraphUtil.createPort` does.
    pub source_port: Option<String>,
    /// The id of the target node's port that the edge ends at, like `source_port`.
    pub target_port: Option<String>,
}

/// One endpoint of a hierarchy edge segment materialized by an outer recursive-layout owner.
///
/// `ParentBoundary` reuses the compound preprocessor's external-port dummy semantics. The named
/// node is the collapsed compound node in the parent scope; it is intentionally absent from the
/// current scope's node list.
#[derive(Debug, Clone, PartialEq, Eq)]
// Only tests build scoped segments: `layout` lays out a whole hierarchy in one run.
#[allow(dead_code)]
pub enum ElkInputEdgeSegmentEndpoint {
    Node { id: String },
    ParentBoundary { id: String, connects_node: bool },
}

/// A scope-local piece of one original hierarchy-crossing edge.
#[derive(Debug, Clone, PartialEq)]
pub struct ElkInputEdgeSegment {
    pub edge: ElkInputEdge,
    pub source: ElkInputEdgeSegmentEndpoint,
    pub target: ElkInputEdgeSegmentEndpoint,
    pub segment: CompoundEdgeSegment,
    pub edge_order: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ElkInputLabel {
    /// Caller-owned stable label index, preserved in exported labels.
    pub source_index: Option<usize>,
    pub text: String,
    pub width: f64,
    pub height: f64,
    pub placement: EdgeLabelPlacement,
    pub inline: bool,
    /// The label's own `nodeLabels.placement`, which wins over its node's.
    pub node_label_placement: Option<crate::options::NodeLabelPlacement>,
}

impl ElkInputLabel {
    #[cfg(test)]
    pub fn center(text: impl Into<String>, width: f64, height: f64) -> Self {
        Self {
            source_index: None,
            text: text.into(),
            width,
            height,
            placement: EdgeLabelPlacement::Center,
            inline: true,
            node_label_placement: None,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ImportError {
    #[error(transparent)]
    NodeSize(#[from] crate::options::NodeSizeError),
    #[error("ELK graph has duplicate node id: {id}")]
    DuplicateNode { id: String },
    #[error("ELK graph has duplicate edge id: {id}")]
    DuplicateEdge { id: String },
    #[error("ELK graph has duplicate port id: {id}")]
    DuplicatePort { id: String },
    #[error("ELK edge `{edge_id}` references missing node `{node_id}`")]
    MissingEndpoint { edge_id: String, node_id: String },
    /// An edge ends at a port of the node the run lays out, but no child connects to a port of it.
    #[error("ELK edge `{edge_id}` ends at a port of its graph, which has no external ports")]
    ScopePortWithoutDummy { edge_id: String },
    #[error("ELK edge `{edge_id}` names port `{port_id}`, which is no port of node `{node_id}`")]
    MissingPort {
        edge_id: String,
        node_id: String,
        port_id: String,
    },
    /// A node with children has a port across the layout flow, which needs the hierarchical part
    /// of the north and south port steps.
    #[error("ELK port `{port_id}` of node `{node_id}` is on a north or south side")]
    NorthSouthPort { node_id: String, port_id: String },
    #[error("ELK node `{node_id}` references missing parent `{parent_id}`")]
    MissingParent { node_id: String, parent_id: String },
    #[error("ELK parent assignment would create a cycle at node `{node_id}`")]
    ParentCycle { node_id: String },
    #[error("ELK scoped edge `{edge_id}` targets unavailable hierarchy scope `{graph_parent}`")]
    UnavailableSegmentScope {
        edge_id: String,
        graph_parent: String,
    },
    /// The caller's segment depth plus the depth inside this scope does not fit a `usize`.
    #[error("ELK scoped edge `{edge_id}` has a segment depth that overflows once rebased")]
    SegmentDepthOverflow {
        /// Id of the scoped edge.
        edge_id: String,
    },
}

pub type ImportResult<T> = Result<T, ImportError>;

/// Imports the adapter graph into an ELK layered `LGraph`.
///
/// This mirrors the front half of `ElkGraphImporter.importGraph(...)`: create the root `LGraph`,
/// transform nodes into `layerless_nodes`, create nested graphs for hierarchy-enabled compound
/// nodes, transform edges with synthetic ports, and mark graph properties discovered during import.
#[cfg(test)]
pub fn import_graph(input: &ElkInputGraph) -> ImportResult<GraphArena> {
    import_graph_with_segments(input, &[])
}

/// Imports a separately materialized hierarchy scope and its boundary edge pieces.
///
/// Each segment is one scope-local piece of a hierarchy-crossing edge. The caller splits the
/// edge; this function only builds the pieces that fall into the scope `input` describes.
#[cfg(test)]
pub fn import_graph_with_segments(
    input: &ElkInputGraph,
    segments: &[ElkInputEdgeSegment],
) -> ImportResult<GraphArena> {
    import_scope(input, segments, ElkPadding::uniform(0.0), ScopePorts::NONE)
}

/// The ports of the node a run lays out, which ELK sees as the external ports of the graph.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ScopePorts<'a> {
    /// In input order. Each port has its side defined: `ElkGraphImporter.ensureDefinedPortSide`.
    pub(crate) ports: &'a [ElkInputPort],
    /// The size of the node, which the dummies of ports with fixed ratios read.
    pub(crate) node_size: LSize,
}

impl ScopePorts<'_> {
    /// A graph whose node has no ports.
    #[cfg(test)]
    pub(crate) const NONE: ScopePorts<'static> = ScopePorts {
        ports: &[],
        node_size: LSize {
            width: 0.0,
            height: 0.0,
        },
    };
}

/// Imports a graph whose root has inside node labels. `root_label_padding` is the padding
/// `NodeLabelAndSizeCalculator.computeInsideNodeLabelPadding` gives the root node.
pub(crate) fn import_scope(
    input: &ElkInputGraph,
    segments: &[ElkInputEdgeSegment],
    root_label_padding: ElkPadding,
    scope: ScopePorts<'_>,
) -> ImportResult<GraphArena> {
    let mut index = InputIndex::new(input, scope.ports)?;
    validate_scoped_edge_segments(&index, segments)?;
    let mut root = LGraph::new(input.id.clone(), input.options.clone());
    root.options.direction = resolve_direction(root.options.direction, root.options.aspect_ratio);
    root.options.hierarchy_handling =
        resolve_root_hierarchy_handling(root.options.hierarchy_handling);
    apply_graph_padding_from_options(&mut root);
    add_padding(&mut root, root_label_padding);
    root.options = root.options.with_spacing_base_value_applied();
    // `importGraph` makes the dummies of the external ports before it imports any child, so
    // they come first in the graph.
    index.scope_dummies = import_external_ports(&mut root, &scope, input.id.as_str());
    let mut port_index = ImportPortIndex::new();

    let (mut graphs, nested_graphs) =
        if root.options.hierarchy_handling == HierarchyHandling::IncludeChildren {
            import_hierarchical_graph(&index, root, &mut port_index)?
        } else {
            import_flat_graph(input, &index, &mut root, None, &mut port_index)?;
            (GraphArena::new(root), vec![None; index.node_ids.len()])
        };

    check_north_south_ports(&graphs)?;
    // The graph inside a node. A node without one reads as the root, as `graph_slot_for_parent`
    // does.
    let graph_of = |node: &str| {
        index
            .node_index(node)
            .and_then(|node| nested_graphs[node])
            .unwrap_or(GraphId::ROOT)
    };
    let prepared_segments =
        materialize_scoped_edge_segments(&index, segments, input.options.merge_edges, &graph_of);
    introduce_source_ported_scoped_edge_segments(
        &mut graphs,
        GraphId::ROOT,
        prepared_segments.segments,
    );

    graphs.sync_graph_properties_to_options();
    Ok(graphs)
}

/// Makes the external port dummy of each port of the graph's node, if the graph has external
/// ports. Gives the dummy of each port by the port's id.
///
/// Ports `ElkGraphImporter.checkExternalPorts` and `transformExternalPort`. Port labels are
/// refused before this runs, and inside self loops are refused, so a port is external only
/// when an edge connects it to a child.
fn import_external_ports(
    root: &mut LGraph,
    scope: &ScopePorts<'_>,
    scope_id: &str,
) -> HashMap<String, NodeId> {
    let mut dummies = HashMap::new();
    if scope.ports.iter().any(|port| port.child_edges > 0) {
        root.graph_properties.external_ports = true;
    }
    if scope.ports.iter().any(|port| port.child_edges > 1) {
        root.graph_properties.hyperedges = true;
    }
    if !root.graph_properties.external_ports {
        return dummies;
    }
    let constraints = root.options.port_constraints;
    for (position, port) in scope.ports.iter().enumerate() {
        // If the port has no offset, infer one. A port at (0, 0) gets none, to spare the
        // common case.
        let border_offset = port.border_offset.unwrap_or_else(|| {
            if port.x == 0.0 && port.y == 0.0 {
                0.0
            } else {
                let lport = port_at(&port.id, port.x, port.y, port.width, port.height);
                calc_port_offset(&lport, scope.node_size, port.side)
            }
        });
        let mut dummy = create_external_port_dummy(
            format!("{scope_id}:external:{}", port.id),
            port.id.clone(),
            PortType::Output,
            constraints,
            port.side,
            port.net_flow as isize,
            LPoint {
                x: port.x + port.width / 2.0,
                y: port.y + port.height / 2.0,
            },
            LSize {
                width: port.width,
                height: port.height,
            },
            scope.node_size,
            border_offset,
            root.options.direction,
        );
        apply_port_index_to_dummy(&mut dummy, constraints, port.index.map(|i| i as isize));
        let node = root.next_node_id();
        let dummy_port = &mut dummy.ports[PortId::new(0)];
        dummy_port.node = node;
        dummy_port.named = true;
        dummy_port.connected_to_external_nodes = port.connected_to_external_nodes;
        dummy.scope_port = Some(position);
        root.layerless_nodes.push(dummy);
        dummies.insert(port.id.clone(), node);
    }
    dummies
}

/// Refuses a node with children that has a port across the layout flow, while its graph has
/// ports that are not free.
///
/// `NorthSouthPortPreprocessor` handles the ports across the flow of a node without children. A
/// node with children needs more: its ports have dummies inside the node as well, and the sweep
/// over the hierarchy must place them. That part is not ported. The condition is the one of
/// `OrthogonalEdgeRouter.getLayoutProcessorConfiguration`, which adds the step when a graph has
/// ports that are not free and a port across the layout flow. A graph with only free ports needs
/// no step: `PortSideProcessor` puts every port on a side along the flow.
fn check_north_south_ports(graphs: &GraphArena) -> ImportResult<()> {
    let mut pending = vec![GraphId::ROOT];
    while let Some(id) = pending.pop() {
        let graph = &graphs[id];
        let properties = &graph.graph_properties;
        if properties.north_south_ports && properties.non_free_ports {
            let offender = graph
                .layerless_nodes
                .iter()
                .filter(|node| has_children(graphs, node))
                .flat_map(|node| node.ports.iter().map(move |port| (node, port)))
                .find(|(_, port)| {
                    port.input_index.is_some() && is_across_flow(graph.options.direction, port.side)
                });
            if let Some((node, port)) = offender {
                return Err(ImportError::NorthSouthPort {
                    node_id: node.id.clone(),
                    port_id: port.id.clone(),
                });
            }
        }
        pending.extend(graphs.children(id).into_iter().rev());
    }
    Ok(())
}

/// Whether the importer put children into the nested graph of a node. A node can also have a
/// nested graph of dummies only, for an inside self loop: it has no children, as
/// `node_materializes_children` says, and every node the importer makes for a child is a normal
/// node.
fn has_children(graphs: &GraphArena, node: &LNode) -> bool {
    node.nested_graph.is_some_and(|nested| {
        graphs[nested]
            .layerless_nodes
            .iter()
            .any(|child| child.kind == LNodeKind::Normal)
    })
}

/// Rejects a scoped segment before any graph object is built. It checks that every endpoint is
/// reachable in this scope and that rebasing the segment depth cannot overflow.
fn validate_scoped_edge_segments(
    index: &InputIndex<'_>,
    segments: &[ElkInputEdgeSegment],
) -> ImportResult<()> {
    for scoped in segments {
        validate_scoped_segment_endpoint(index, &scoped.edge, &scoped.source)?;
        validate_scoped_segment_endpoint(index, &scoped.edge, &scoped.target)?;
        let source_depth = index.scoped_endpoint_depth(&scoped.edge, &scoped.source)?;
        let target_depth = index.scoped_endpoint_depth(&scoped.edge, &scoped.target)?;
        // Every generated piece is at most as deep as the deeper endpoint. `rebase_scoped_segment`
        // adds that depth to the caller's depth, so check the sum once, here.
        scoped_segment_depth(scoped.segment)
            .checked_add(source_depth.max(target_depth))
            .ok_or_else(|| ImportError::SegmentDepthOverflow {
                edge_id: scoped.edge.id.clone(),
            })?;
    }
    Ok(())
}

fn materialize_scoped_edge_segments(
    index: &InputIndex<'_>,
    segments: &[ElkInputEdgeSegment],
    merge_edges: bool,
    graph_of: &dyn Fn(&str) -> GraphId,
) -> PreparedScopedSegments {
    let steps = |path: Vec<&str>| {
        path.into_iter()
            .map(|node| HierarchyStep {
                node_id: node.to_string(),
                graph: graph_of(node),
            })
            .collect::<Vec<_>>()
    };
    let mut pending_segments = Vec::new();

    for scoped in segments {
        let edge_order = scoped.edge_order;
        let source_port_key = hierarchy_port_key(
            scoped.edge.source.as_str(),
            edge_order,
            "source",
            merge_edges,
            PortType::Output,
        );
        let target_port_key = hierarchy_port_key(
            scoped.edge.target.as_str(),
            edge_order,
            "target",
            merge_edges,
            PortType::Input,
        );
        let source_placeholder = scoped_segment_endpoint_id(&scoped.source);
        let target_placeholder = scoped_segment_endpoint_id(&scoped.target);
        let source_path = match &scoped.source {
            ElkInputEdgeSegmentEndpoint::Node { id } => steps(
                index
                    .materialized_graph_path(id)
                    .expect("scoped endpoint reachability was validated before materialization"),
            ),
            ElkInputEdgeSegmentEndpoint::ParentBoundary { .. } => Vec::new(),
        };
        let target_path = match &scoped.target {
            ElkInputEdgeSegmentEndpoint::Node { id } => steps(
                index
                    .materialized_graph_path(id)
                    .expect("scoped endpoint reachability was validated before materialization"),
            ),
            ElkInputEdgeSegmentEndpoint::ParentBoundary { .. } => Vec::new(),
        };
        let mut local = source_ported_cross_hierarchy_segments(
            source_placeholder,
            target_placeholder,
            source_port_key.as_str(),
            target_port_key.as_str(),
            &source_path,
            &target_path,
        );
        if local.is_empty() {
            local.push(PendingCompoundSegment {
                graph_parent: None,
                source: PendingSegmentEndpoint::LocalNode {
                    node_id: source_placeholder.to_string(),
                    port_key: source_port_key.clone(),
                },
                target: PendingSegmentEndpoint::LocalNode {
                    node_id: target_placeholder.to_string(),
                    port_key: target_port_key.clone(),
                },
                segment: scoped.segment,
            });
        }

        for pending in &mut local {
            replace_scoped_boundary_endpoint(
                &mut pending.source,
                &scoped.source,
                true,
                source_port_key.as_str(),
                target_port_key.as_str(),
            );
            replace_scoped_boundary_endpoint(
                &mut pending.target,
                &scoped.target,
                false,
                source_port_key.as_str(),
                target_port_key.as_str(),
            );
            pending.segment = rebase_scoped_segment(pending.segment, scoped.segment);
        }

        let label_segments: Vec<_> = scoped
            .edge
            .labels
            .iter()
            .map(|label| compound_label_segment_index(&local, label.placement))
            .collect();
        for (segment_index, pending) in local.into_iter().enumerate() {
            let labels = scoped
                .edge
                .labels
                .iter()
                .zip(&label_segments)
                .filter(|(_, owner)| **owner == segment_index)
                .map(|(label, _)| {
                    let mut label = label_to_lgraph(label);
                    label.original_label_edge = Some(scoped.edge.id.clone());
                    label
                })
                .collect();
            pending_segments.push(ScopedHierarchySegment {
                edge: HierarchyEdge {
                    id: scoped.edge.id.clone(),
                    source_node_id: scoped.edge.source.clone(),
                    target_node_id: scoped.edge.target.clone(),
                    source_port_key: source_port_key.clone(),
                    target_port_key: target_port_key.clone(),
                    // A scoped segment never reaches `original_net_flows`, and the keys here are
                    // all made up.
                    source_port_named: false,
                    target_port_named: false,
                    source_path: Vec::new(),
                    target_path: Vec::new(),
                    labels: Vec::new(),
                    minlen: scoped.edge.minlen.max(1),
                    model_order: scoped.edge.model_order,
                    priority_direction: scoped.edge.priority_direction,
                    priority_shortness: scoped.edge.priority_shortness,
                    priority_straightness: scoped.edge.priority_straightness,
                },
                pending,
                labels,
            });
        }
    }

    PreparedScopedSegments {
        segments: pending_segments,
    }
}

fn scoped_segment_depth(segment: CompoundEdgeSegment) -> usize {
    match segment {
        CompoundEdgeSegment::Output { depth } | CompoundEdgeSegment::Input { depth } => depth,
    }
}

fn rebase_scoped_segment(
    local: CompoundEdgeSegment,
    scoped: CompoundEdgeSegment,
) -> CompoundEdgeSegment {
    // `local` is relative to this import scope, but `scoped` already carries the caller's absolute
    // compound side and depth. Preserve that side while rebasing every generated local piece.
    let depth = scoped_segment_depth(scoped)
        .checked_add(scoped_segment_depth(local))
        .expect("scoped segment depth was checked before materialization");
    match scoped {
        CompoundEdgeSegment::Output { .. } => CompoundEdgeSegment::Output { depth },
        CompoundEdgeSegment::Input { .. } => CompoundEdgeSegment::Input { depth },
    }
}

struct PreparedScopedSegments {
    segments: Vec<ScopedHierarchySegment>,
}

fn validate_scoped_segment_endpoint(
    index: &InputIndex<'_>,
    edge: &ElkInputEdge,
    endpoint: &ElkInputEdgeSegmentEndpoint,
) -> ImportResult<()> {
    let ElkInputEdgeSegmentEndpoint::Node { id } = endpoint else {
        return Ok(());
    };
    if index.nodes.contains_key(id.as_str()) {
        return Ok(());
    }
    Err(ImportError::MissingEndpoint {
        edge_id: edge.id.clone(),
        node_id: id.clone(),
    })
}

fn scoped_segment_endpoint_id(endpoint: &ElkInputEdgeSegmentEndpoint) -> &str {
    match endpoint {
        ElkInputEdgeSegmentEndpoint::Node { id }
        | ElkInputEdgeSegmentEndpoint::ParentBoundary { id, .. } => id.as_str(),
    }
}

fn replace_scoped_boundary_endpoint(
    pending: &mut PendingSegmentEndpoint,
    endpoint: &ElkInputEdgeSegmentEndpoint,
    source: bool,
    source_port_key: &str,
    target_port_key: &str,
) {
    let ElkInputEdgeSegmentEndpoint::ParentBoundary { id, connects_node } = endpoint else {
        return;
    };
    let PendingSegmentEndpoint::LocalNode { node_id, .. } = pending else {
        return;
    };
    if node_id != id {
        return;
    }

    let (port_type, parent_port_type, port_key) = if source {
        (
            PortType::Input,
            if *connects_node {
                PortType::Output
            } else {
                PortType::Input
            },
            if *connects_node {
                source_port_key
            } else {
                target_port_key
            },
        )
    } else {
        (
            PortType::Output,
            if *connects_node {
                PortType::Input
            } else {
                PortType::Output
            },
            if *connects_node {
                target_port_key
            } else {
                source_port_key
            },
        )
    };
    *pending = PendingSegmentEndpoint::ParentBoundary {
        node_id: id.clone(),
        port_key: port_key.to_string(),
        port_type,
        parent_port_type,
        connects_parent_node: *connects_node,
    };
}

/// Stands in the port index for the node a hierarchy edge's parent end sits at: the dummy of an
/// inside self loop belongs to no node of the graph.
const PARENT_BOUNDARY: NodeId = NodeId::new(usize::MAX);

#[derive(Default)]
struct ImportNodePortIndex {
    by_type: HashMap<PortType, HashMap<String, PortRef>>,
}

#[derive(Default)]
struct ImportGraphPortIndex {
    nodes: HashMap<NodeId, ImportNodePortIndex>,
}

struct ImportPortIndex {
    root: ImportGraphPortIndex,
    graphs: HashMap<String, ImportGraphPortIndex>,
    /// The edges that cross a level, as (incoming, outgoing) per port, keyed by the id of the node
    /// and the key of the port. ELK's importer attaches such an edge to its ports at once. This
    /// importer keeps it apart until the compound preprocessor splits it, so the count lives here.
    /// Two maps, so a lookup by `&str` needs no `String` for the key.
    crossing_edges: HashMap<String, NodeCrossings>,
}

/// The crossing edges of one node, by port key. A made-up port can have the id of a port the caller
/// named at the same node, such as the implicit port `G:2` beside a named port `G:2`. ELK tells
/// ports apart by object, so each kind of port has a map of its own.
#[derive(Default)]
struct NodeCrossings {
    named: HashMap<String, (usize, usize)>,
    made_up: HashMap<String, (usize, usize)>,
}

impl NodeCrossings {
    fn of(&self, named: bool) -> &HashMap<String, (usize, usize)> {
        if named { &self.named } else { &self.made_up }
    }

    fn of_mut(&mut self, named: bool) -> &mut HashMap<String, (usize, usize)> {
        if named {
            &mut self.named
        } else {
            &mut self.made_up
        }
    }
}

impl ImportPortIndex {
    fn new() -> Self {
        Self {
            root: ImportGraphPortIndex::default(),
            graphs: HashMap::new(),
            crossing_edges: HashMap::new(),
        }
    }

    /// The edges that cross a level and end at this port, as (incoming, outgoing). `named` says
    /// whether the caller named the port.
    fn crossing_at(&self, node_id: &str, port_key: &str, named: bool) -> (usize, usize) {
        self.crossing_edges
            .get(node_id)
            .and_then(|ports| ports.of(named).get(port_key))
            .copied()
            .unwrap_or_default()
    }

    fn get(
        &mut self,
        graph_parent: Option<&str>,
        node: NodeId,
        port_key: &str,
        port_type: PortType,
    ) -> Option<PortRef> {
        let graph = match graph_parent {
            Some(parent) => self.graphs.get(parent),
            None => Some(&self.root),
        };
        graph
            .and_then(|graph| graph.nodes.get(&node))
            .and_then(|node| node.by_type.get(&port_type))
            .and_then(|ports| ports.get(port_key))
            .copied()
    }

    fn insert(
        &mut self,
        graph_parent: Option<&str>,
        node: NodeId,
        port_key: &str,
        port_type: PortType,
        port: PortRef,
    ) {
        let graph = match graph_parent {
            Some(parent) => self.graphs.entry(parent.to_string()).or_default(),
            None => &mut self.root,
        };
        graph
            .nodes
            .entry(node)
            .or_default()
            .by_type
            .entry(port_type)
            .or_default()
            .insert(port_key.to_string(), port);
    }
}

fn import_flat_graph(
    input: &ElkInputGraph,
    index: &InputIndex<'_>,
    graph: &mut LGraph,
    parent: Option<&str>,
    port_index: &mut ImportPortIndex,
) -> ImportResult<()> {
    let needs_model_order = needs_model_order_based_on_parent(&graph.options);
    for (model_order, node) in index.children(parent).iter().copied().enumerate() {
        transform_node(node, graph, needs_model_order.then_some(model_order));
    }

    for (model_order, edge) in input.edges.iter().enumerate() {
        let source_parent = index.node_parent(edge.source.as_str());
        let target_parent = index.node_parent(edge.target.as_str());
        if source_parent == parent && target_parent == parent {
            transform_edge(
                edge,
                index,
                graph,
                parent,
                needs_model_order.then(|| edge.model_order.unwrap_or(model_order)),
                port_index,
            )?;
        }
    }

    Ok(())
}

/// The graphs of a hierarchical import while it is under way.
struct HierarchicalImportArena {
    graphs: GraphArena,
    // Use unique input-node positions, not graph ids: Mermaid may name a compound node exactly
    // like the adapter root, while the root and nested port namespaces must remain distinct.
    nested_graph_slots: Vec<Option<GraphId>>,
}

impl HierarchicalImportArena {
    fn new(root: LGraph, input_node_count: usize) -> Self {
        Self {
            graphs: GraphArena::new(root),
            nested_graph_slots: vec![None; input_node_count],
        }
    }

    fn root(&self) -> &LGraph {
        self.graphs.root()
    }

    fn root_mut(&mut self) -> &mut LGraph {
        self.graphs.root_mut()
    }

    fn graph(&self, slot: GraphId) -> &LGraph {
        &self.graphs[slot]
    }

    fn graph_mut(&mut self, slot: GraphId) -> &mut LGraph {
        &mut self.graphs[slot]
    }

    fn graph_slot_for_parent(&self, index: &InputIndex<'_>, parent: Option<&str>) -> GraphId {
        // Only IncludeChildren (plus the explicit inside-self-loop dummy case) creates a slot.
        // Unmaterialized SeparateChildren scopes retain the importer's historical root fallback.
        parent
            .and_then(|parent| index.node_index(parent))
            .and_then(|parent| self.nested_graph_slots[parent])
            .unwrap_or(GraphId::ROOT)
    }

    fn add_nested_graph(
        &mut self,
        input_node: usize,
        parent_slot: GraphId,
        parent_node: NodeId,
        graph: LGraph,
    ) -> GraphId {
        let slot = self.graphs.nest(parent_slot, parent_node, graph);
        let previous = self.nested_graph_slots[input_node].replace(slot);
        debug_assert!(
            previous.is_none(),
            "an input node owns at most one nested graph"
        );
        slot
    }

    /// The graphs, and the graph inside each input node by the node's position in the input.
    fn into_graphs(self) -> (GraphArena, Vec<Option<GraphId>>) {
        (self.graphs, self.nested_graph_slots)
    }
}

fn import_hierarchical_graph(
    index: &InputIndex<'_>,
    root: LGraph,
    port_index: &mut ImportPortIndex,
) -> ImportResult<(GraphArena, Vec<Option<GraphId>>)> {
    let mut arena = HierarchicalImportArena::new(root, index.node_ids.len());
    let mut queue = VecDeque::new();
    queue.extend(
        index
            .children(None)
            .iter()
            .copied()
            .map(|node| (node, GraphId::ROOT)),
    );

    let mut model_order = 0usize;
    while let Some((node, parent_slot)) = queue.pop_front() {
        let node_model_order = needs_model_order_for_child(index, node).then(|| {
            let order = model_order;
            model_order += 1;
            order
        });
        let nested = {
            let parent_graph = arena.graph_mut(parent_slot);
            let node_index = transform_node(node, parent_graph, node_model_order);
            if node_needs_nested_graph(index, &parent_graph.options, node) {
                let materializes_children =
                    node_materializes_children(index, &parent_graph.options, node);
                let nested_options = nested_graph_options(&parent_graph.options, node);
                let mut nested_graph = LGraph::new(node.id.clone(), nested_options);
                // ELK reads the spacings for a node's labels from the node's parent, as the
                // user set them: `IndividualSpacings.getIndividualOrInherited`.
                let cell_node = cell_node(node);
                apply_graph_padding_from_options(&mut nested_graph);
                add_padding(
                    &mut nested_graph,
                    cells::inside_node_label_padding(&cell_node),
                );
                nested_graph.options = nested_graph.options.with_spacing_base_value_applied();
                if !node.node_size.constraints.is_empty() {
                    let horizontal = !node.parent_settings.direction.is_vertical();
                    include_minimum_graph_size(&mut nested_graph.options, &cell_node, horizontal);
                }
                parent_graph.layerless_nodes[node_index].compound = true;
                Some((node_index, nested_graph, materializes_children))
            } else {
                None
            }
        };

        if let Some((parent_node, nested_graph, materializes_children)) = nested {
            let input_node = index
                .node_index(node.id.as_str())
                .expect("every imported node has a stable input index");
            let nested_slot =
                arena.add_nested_graph(input_node, parent_slot, parent_node, nested_graph);
            if materializes_children {
                // Keep the existing ELK importer breadth-first model-order traversal. The graph
                // slot is only an ownership locator and does not reorder Mermaid siblings.
                queue.extend(
                    index
                        .children(Some(node.id.as_str()))
                        .iter()
                        .copied()
                        .map(|child| (child, nested_slot)),
                );
            }
        }
    }

    let mut edge_order = 0usize;
    let mut edge_graph_queue = VecDeque::new();
    edge_graph_queue.push_back((None, GraphId::ROOT));
    while let Some((parent, owner_slot)) = edge_graph_queue.pop_front() {
        for edge in index.edges(parent).iter().copied() {
            let edge_model_order = needs_model_order_based_on_parent(&arena.root().options)
                .then(|| edge.model_order.unwrap_or(edge_order));
            let source_parent = index.node_parent(edge.source.as_str());
            let target_parent = index.node_parent(edge.target.as_str());
            let source_container_slot = arena.graph_slot_for_parent(index, source_parent);
            let inside_self_loops_activate = arena
                .graph(source_container_slot)
                .options
                .inside_self_loops_activate;

            if edge.inside_self_loops_yo && edge.source == edge.target && inside_self_loops_activate
            {
                let graph_slot = arena.graph_slot_for_parent(index, Some(edge.source.as_str()));
                let graph = arena.graph_mut(graph_slot);
                transform_inside_self_loop(edge, graph, edge_model_order, port_index);
            } else if source_parent == target_parent {
                let graph = arena.graph_mut(owner_slot);
                transform_edge(
                    edge,
                    index,
                    graph,
                    source_parent,
                    edge_model_order,
                    port_index,
                )?;
            } else {
                transform_cross_hierarchy_edge(
                    edge,
                    &mut arena,
                    owner_slot,
                    index,
                    edge_order,
                    edge_model_order,
                    port_index,
                )?;
            }
            edge_order += 1;
        }

        for child in index
            .children(parent)
            .iter()
            .copied()
            .filter(|child| input_node_uses_include_children_layout(index, child))
        {
            let child_slot = arena.graph_slot_for_parent(index, Some(child.id.as_str()));
            edge_graph_queue.push_back((Some(child.id.as_str()), child_slot));
        }
    }

    Ok(arena.into_graphs())
}

fn transform_inside_self_loop(
    edge: &ElkInputEdge,
    graph: &mut LGraph,
    model_order: Option<usize>,
    port_index: &mut ImportPortIndex,
) -> usize {
    let source_port_key = inside_self_loop_port_key(edge.source.as_str(), "source");
    let target_port_key = inside_self_loop_port_key(edge.target.as_str(), "target");
    let source = ensure_inside_self_loop_dummy(
        graph,
        edge.source.as_str(),
        PortType::Output,
        source_port_key.as_str(),
        port_index,
    );
    let target = ensure_inside_self_loop_dummy(
        graph,
        edge.target.as_str(),
        PortType::Input,
        target_port_key.as_str(),
        port_index,
    );

    let mut labels = Vec::new();
    for label in &edge.labels {
        match label.placement {
            EdgeLabelPlacement::Center => graph.graph_properties.center_labels = true,
            EdgeLabelPlacement::Head | EdgeLabelPlacement::Tail => {
                graph.graph_properties.end_labels = true;
            }
        }
        labels.push(label_to_lgraph(label));
    }

    let edge_index = graph
        .add_edge(LayeredEdge {
            id: edge.id.clone(),
            source,
            target,
            source_node_id: edge.source.clone(),
            target_node_id: edge.target.clone(),
            labels,
            minlen: edge.minlen.max(1),
            reversed: false,
            bend_points: Vec::new(),
            junction_points: Vec::new(),
            model_order,
            priority_direction: edge.priority_direction,
            priority_shortness: edge.priority_shortness,
            priority_straightness: edge.priority_straightness,
            thickness: ELK_DEFAULT_EDGE_THICKNESS,
            original_opposite_port: None,
            compound_segment: None,
        })
        .expect("inside self-loop dummies were created before adding edge");

    graph.graph_properties.self_loops = true;
    if has_parallel_port_edges(&graph.layerless_nodes[source.node].ports[source.port])
        || has_parallel_port_edges(&graph.layerless_nodes[target.node].ports[target.port])
    {
        graph.graph_properties.hyperedges = true;
    }

    edge_index
}

fn ensure_inside_self_loop_dummy(
    graph: &mut LGraph,
    parent_node_id: &str,
    port_type: PortType,
    parent_port_key: &str,
    port_index: &mut ImportPortIndex,
) -> PortRef {
    if let Some(port) = port_index.get(Some(parent_node_id), PARENT_BOUNDARY, parent_port_key, port_type)
    {
        return port;
    }
    graph.graph_properties.external_ports = true;
    graph.graph_properties.non_free_ports = true;

    let dummy_id = format!("external:{parent_node_id}");
    let mut dummy = create_external_port_dummy(
        dummy_id,
        parent_port_key.to_string(),
        port_type,
        PortConstraints::Free,
        PortSide::Undefined,
        match port_type {
            PortType::Input => 1,
            PortType::Output => -1,
        },
        Default::default(),
        LSize::default(),
        LSize::default(),
        0.0,
        graph.options.direction,
    );
    dummy.parent_port_key = Some(parent_port_key.to_string());
    dummy.parent_port_type = Some(port_type);

    let node = graph.next_node_id();
    dummy.ports[PortId::new(0)].node = node;
    graph.layerless_nodes.push(dummy);
    let port = PortRef {
        node,
        port: PortId::new(0),
    };
    port_index.insert(
        Some(parent_node_id),
        PARENT_BOUNDARY,
        parent_port_key,
        port_type,
        port,
    );
    port
}

fn inside_self_loop_port_key(node_id: &str, role: &str) -> String {
    format!("{node_id}:{role}")
}

/// Mirrors `ElkGraphImporter#needsModelOrder(...)`.
fn needs_model_order_for_child(index: &InputIndex<'_>, child: &ElkInputNode) -> bool {
    needs_model_order_based_on_input_parent(index, child)
}

/// Mirrors `ElkGraphImporter#needsModelOrderBasedOnParent(...)` for currently ported options.
fn needs_model_order_based_on_parent(options: &LayeredOptions) -> bool {
    options.consider_model_order_strategy != OrderingStrategy::None
        || matches!(
            options.cycle_breaking_strategy,
            CycleBreakingStrategy::ModelOrder | CycleBreakingStrategy::GreedyModelOrder
        )
        || options.force_node_model_order
        || matches!(
            options.layering_strategy,
            LayeringStrategy::BreadthFirstModelOrder | LayeringStrategy::DepthFirstModelOrder
        )
}

fn needs_model_order_based_on_input_parent(index: &InputIndex<'_>, child: &ElkInputNode) -> bool {
    let Some(parent_id) = child.parent.as_deref() else {
        return needs_model_order_based_on_parent(&index.root_options);
    };
    let Some(parent_options) = index.effective_options.get(parent_id) else {
        return false;
    };
    needs_model_order_based_on_parent(parent_options)
}

fn input_node_uses_include_children_layout(index: &InputIndex<'_>, node: &ElkInputNode) -> bool {
    index
        .effective_options
        .get(node.id.as_str())
        .is_some_and(|options| options.hierarchy_handling == HierarchyHandling::IncludeChildren)
}

/// Returns the graph scope that owns an edge: the shared parent, or the endpoint that contains
/// the other one. `None` means the root scope owns it.
fn input_edge_containing_parent<'a>(
    index: &InputIndex<'a>,
    edge: &'a ElkInputEdge,
) -> Option<&'a str> {
    if let Some(container) = edge.container.as_deref() {
        return index.nodes.contains_key(container).then_some(container);
    }
    // An edge at a port of the node the run lays out belongs to that node's graph, the root.
    if edge.source == index.scope_id || edge.target == index.scope_id {
        return None;
    }
    let source_parent = index.node_parent(edge.source.as_str());
    let target_parent = index.node_parent(edge.target.as_str());
    if source_parent == target_parent {
        return source_parent;
    }

    if index.is_node_ancestor(edge.source.as_str(), edge.target.as_str()) {
        return Some(edge.source.as_str());
    }
    if index.is_node_ancestor(edge.target.as_str(), edge.source.as_str()) {
        return Some(edge.target.as_str());
    }

    None
}

fn nested_graph_options(parent_options: &LayeredOptions, node: &ElkInputNode) -> LayeredOptions {
    // ElkGraphImporter.createLGraph copies the container's own properties. Renderer-specific
    // presets and overrides must arrive on that node rather than leak in from the root graph.
    let mut options = node.nested_options.as_deref().cloned().unwrap_or_default();
    options.direction = parent_options.direction;
    options.hierarchy_handling =
        resolve_child_hierarchy_handling(node.hierarchy_handling, parent_options);
    options.port_constraints = node.port_constraints.unwrap_or(PortConstraints::Free);
    options
}

fn resolve_child_hierarchy_handling(
    node_hierarchy_handling: Option<HierarchyHandling>,
    parent_options: &LayeredOptions,
) -> HierarchyHandling {
    match node_hierarchy_handling.unwrap_or(HierarchyHandling::Inherit) {
        HierarchyHandling::Inherit => parent_options.hierarchy_handling,
        handling => handling,
    }
}

fn resolve_root_hierarchy_handling(handling: HierarchyHandling) -> HierarchyHandling {
    match handling {
        HierarchyHandling::Inherit => HierarchyHandling::SeparateChildren,
        handling => handling,
    }
}

fn apply_graph_padding_from_options(graph: &mut LGraph) {
    let ElkPadding {
        top,
        right,
        bottom,
        left,
    } = graph.options.padding;
    graph.padding.top += top;
    graph.padding.right += right;
    graph.padding.bottom += bottom;
    graph.padding.left += left;
}

fn add_padding(graph: &mut LGraph, padding: ElkPadding) {
    graph.padding.top += padding.top;
    graph.padding.right += padding.right;
    graph.padding.bottom += padding.bottom;
    graph.padding.left += padding.left;
}

/// What ELK's cell system reads from a node of the input graph. The label spacings come from
/// the options the node's parent sets.
pub(crate) fn cell_node(node: &ElkInputNode) -> CellNode {
    let parent = &node.parent_settings.options;
    CellNode {
        size: LSize {
            width: node.width,
            height: node.height,
        },
        sizing: node.node_size,
        placement: node.node_label_placement,
        labels: node
            .labels
            .iter()
            .map(|label| CellLabel {
                size: LSize {
                    width: label.width,
                    height: label.height,
                },
                placement: label.node_label_placement,
            })
            .collect(),
        node_labels_padding: parent.node_labels_padding,
        label_node_spacing: parent.spacing.label_node,
        label_label_spacing: parent.spacing.label_label,
        fixed_graph_size: false,
    }
}

/// Makes the graph inside a node large enough for the node's labels and minimum size.
///
/// Source: `ElkGraphImporter.calculateMinimumGraphSize`. It adds `MINIMUM_SIZE` to the
/// graph's size constraints and raises the graph's minimum size to what the node's cell
/// system asks for. `horizontal_layout_mode` is false only when the node's parent sets a
/// vertical direction itself.
pub(crate) fn include_minimum_graph_size(
    options: &mut LayeredOptions,
    node: &CellNode,
    horizontal_layout_mode: bool,
) {
    let minimum = cells::process(node, horizontal_layout_mode, false).size;
    options
        .node_size
        .constraints
        .insert(crate::options::SizeConstraints::MINIMUM_SIZE);
    let configured = &mut options.node_size.minimum;
    configured.width = f64::max(minimum.width, configured.width);
    configured.height = f64::max(minimum.height, configured.height);
}

/// Ports `ElkGraphImporter.transformNode`, for a node without children.
fn transform_node(node: &ElkInputNode, graph: &mut LGraph, model_order: Option<usize>) -> NodeId {
    let mut lnode = LNode::new(node.id.clone(), node.width, node.height, model_order);
    // Java reads the constraints before it replaces UNDEFINED, and hands that value on to the
    // ports. UNDEFINED is not side fixed, like FREE.
    let declared_constraints = node.port_constraints.unwrap_or(PortConstraints::Undefined);
    lnode.port_constraints = match declared_constraints {
        PortConstraints::Undefined => PortConstraints::Free,
        other => other,
    };
    if !matches!(
        declared_constraints,
        PortConstraints::Undefined | PortConstraints::Free
    ) {
        graph.graph_properties.non_free_ports = true;
    }
    lnode.port_alignment = node.port_alignment;
    if let Some(layer_constraint) = node.layer_constraint {
        lnode.layer_constraint = layer_constraint;
        lnode.layer_constraint_explicit = true;
    }
    // `ElkGraphImporter.transformNode` skips labels without text.
    for label in node.labels.iter().filter(|label| !label.text.is_empty()) {
        lnode.labels.push(label_to_lgraph(label));
    }
    lnode.node_label_placement = node.node_label_placement;
    lnode.node_size = node.node_size;
    lnode.self_loop_distribution = node.self_loop_distribution;
    lnode.self_loop_ordering = node.self_loop_ordering;
    lnode.node_flexibility = node.node_flexibility;
    lnode.ports_surrounding = node.ports_surrounding;
    let node_size = lnode.size;
    let node_index = graph.next_node_id();
    graph.layerless_nodes.push(lnode);

    // A port gets a model order when its node does: `needsModelOrder` asks about the node's
    // parent, which also decides the node's own order.
    for (position, port) in node.ports.iter().enumerate() {
        let port_model_order = model_order.map(|_| position);
        let lport = transform_port(
            port,
            position,
            node_index,
            node_size,
            declared_constraints,
            graph,
            port_model_order,
        );
        graph.layerless_nodes[node_index].ports.push(lport);
    }
    node_index
}

/// Ports `ElkGraphImporter.transformPort`.
fn transform_port(
    port: &ElkInputPort,
    position: usize,
    node: NodeId,
    node_size: LSize,
    constraints: PortConstraints,
    graph: &mut LGraph,
    model_order: Option<usize>,
) -> LPort {
    // Java's `LPort` has no type. This one only lets the importer find a port it made up again,
    // and a named port is never looked up that way.
    let mut lport = LPort::new(port.id.clone(), node, PortType::Output);
    lport.input_index = Some(position);
    lport.named = true;
    lport.hierarchy_net_flow = Some(port.net_flow as isize);
    lport.model_order = model_order;
    lport.side = port.side;
    lport.size = LSize {
        width: port.width,
        height: port.height,
    };
    lport.position = LPoint {
        x: port.x,
        y: port.y,
    };
    // `lport.copyProperties(elkport)` copies the three options. `containsKey` asks whether one is
    // set.
    lport.port_index = port.index.map(|index| index as isize);
    lport.border_offset = port.border_offset;
    lport.allow_non_flow_ports_to_switch_sides = port.allow_non_flow_ports_to_switch_sides;
    // The input has no edge to or from a descendant of its node, because the node has no children.
    // Inside self loops are refused, so `INSIDE_CONNECTIONS` stays false.
    initialize_port(&mut lport, constraints, node_size, graph.options.direction);

    if is_across_flow(graph.options.direction, lport.side) {
        graph.graph_properties.north_south_ports = true;
    }
    lport
}

/// Ports `LGraphUtil.initializePort`, without an explicit anchor: no option supplies one.
///
/// `ElkGraphImporter.transformPort` sets the side before it sets the port's size, so the anchor
/// that `LPort.setSide` computes there is the corner. `initializePort` then replaces it in every
/// case, so only the final anchor is computed here.
fn initialize_port(
    port: &mut LPort,
    constraints: PortConstraints,
    node_size: LSize,
    direction: ElkDirection,
) {
    let mut side = port.side;
    if side == PortSide::Undefined && constraints.is_side_fixed() {
        // The common case of a port at (0, 0) keeps border offset 0.
        side = crate::compound::calc_port_side(port, node_size, direction);
        port.side = side;
        if port.border_offset.is_none()
            && side != PortSide::Undefined
            && (port.position.x != 0.0 || port.position.y != 0.0)
        {
            port.border_offset = Some(calc_port_offset(port, node_size, side));
        }
    }

    if constraints.is_ratio_fixed() {
        let mut ratio = 0.0;
        match side {
            PortSide::North | PortSide::South => {
                if node_size.width > 0.0 {
                    ratio = port.position.x / node_size.width;
                }
            }
            PortSide::East | PortSide::West => {
                if node_size.height > 0.0 {
                    ratio = port.position.y / node_size.height;
                }
            }
            PortSide::Undefined => {}
        }
        port.ratio_or_position = ratio;
    }

    let (width, height) = (port.size.width, port.size.height);
    port.anchor = if constraints.is_side_fixed() {
        match side {
            PortSide::North => LPoint {
                x: width / 2.0,
                y: 0.0,
            },
            PortSide::East => LPoint {
                x: width,
                y: height / 2.0,
            },
            PortSide::South => LPoint {
                x: width / 2.0,
                y: height,
            },
            PortSide::West => LPoint {
                x: 0.0,
                y: height / 2.0,
            },
            // The side is decided later, so the anchor is the centre.
            PortSide::Undefined => LPoint {
                x: width / 2.0,
                y: height / 2.0,
            },
        }
    } else {
        LPoint {
            x: width / 2.0,
            y: height / 2.0,
        }
    };
}

/// Ports `LGraphUtil.calcPortOffset`.
fn calc_port_offset(port: &LPort, node_size: LSize, side: PortSide) -> f64 {
    match side {
        PortSide::North => -(port.position.y + port.size.height),
        PortSide::East => port.position.x - node_size.width,
        PortSide::South => port.position.y - node_size.height,
        PortSide::West => -(port.position.x + port.size.width),
        PortSide::Undefined => 0.0,
    }
}

fn transform_edge(
    edge: &ElkInputEdge,
    index: &InputIndex<'_>,
    graph: &mut LGraph,
    graph_parent: Option<&str>,
    model_order: Option<usize>,
    port_index: &mut ImportPortIndex,
) -> ImportResult<usize> {
    let source = edge_end_port(
        edge,
        &edge.source,
        edge.source_port.as_deref(),
        PortType::Output,
        index,
        graph,
        graph_parent,
        port_index,
    )?;
    let target = edge_end_port(
        edge,
        &edge.target,
        edge.target_port.as_deref(),
        PortType::Input,
        index,
        graph,
        graph_parent,
        port_index,
    )?;

    if source.node == target.node {
        graph.graph_properties.self_loops = true;
    }

    let mut labels = Vec::new();
    for label in &edge.labels {
        match label.placement {
            EdgeLabelPlacement::Center => graph.graph_properties.center_labels = true,
            EdgeLabelPlacement::Head | EdgeLabelPlacement::Tail => {
                graph.graph_properties.end_labels = true;
            }
        }
        labels.push(label_to_lgraph(label));
    }

    let edge_index = graph
        .add_edge(LayeredEdge {
            id: edge.id.clone(),
            source,
            target,
            source_node_id: edge.source.clone(),
            target_node_id: edge.target.clone(),
            labels,
            minlen: edge.minlen.max(1),
            reversed: false,
            bend_points: Vec::new(),
            junction_points: Vec::new(),
            model_order,
            priority_direction: edge.priority_direction,
            priority_shortness: edge.priority_shortness,
            priority_straightness: edge.priority_straightness,
            thickness: ELK_DEFAULT_EDGE_THICKNESS,
            original_opposite_port: None,
            compound_segment: None,
        })
        .expect("ports were created before adding edge");

    // `transformEdge`: a port with more than one incoming or more than one outgoing edge makes
    // the graph one with hyperedges. The edges that cross a level count too.
    let parallel_at = |graph: &LGraph, end: PortRef| {
        let node = &graph.layerless_nodes[end.node];
        let port = &node.ports[end.port];
        let (incoming, outgoing) = port_index.crossing_at(&node.id, &port.id, port.named);
        port.incoming_edges.len() + incoming > 1 || port.outgoing_edges.len() + outgoing > 1
    };
    if parallel_at(graph, source) || parallel_at(graph, target) {
        graph.graph_properties.hyperedges = true;
    }

    Ok(edge_index)
}

/// The port an edge starts or ends at: the named port, or one `LGraphUtil.createPort` makes.
///
/// Ports `ElkGraphImporter.transformEdge`, where it looks up `nodeAndPortMap` and creates the
/// ports that are missing.
#[allow(clippy::too_many_arguments)]
fn edge_end_port(
    edge: &ElkInputEdge,
    node_id: &str,
    port_id: Option<&str>,
    port_type: PortType,
    index: &InputIndex<'_>,
    graph: &mut LGraph,
    graph_parent: Option<&str>,
    port_index: &mut ImportPortIndex,
) -> ImportResult<PortRef> {
    let missing_node = || ImportError::MissingEndpoint {
        edge_id: edge.id.clone(),
        node_id: node_id.to_string(),
    };
    if node_id == index.scope_id && !index.nodes.contains_key(node_id) {
        return scope_port_end(edge, node_id, port_id, index);
    }
    let position = index.child_position(node_id);
    if let Some(port_id) = port_id {
        return named_port(edge, node_id, port_id, index, graph).map(|(port, _)| port);
    }
    ensure_port_at_node(
        graph,
        graph_parent,
        position,
        node_id,
        port_type,
        port_index,
    )
    .ok_or_else(missing_node)
}

/// The port a caller named at one end of an edge, in the graph the node sits in.
///
/// The port must belong to a node of that graph with that id, or the edge has no end there.
fn named_port<'g>(
    edge: &ElkInputEdge,
    node_id: &str,
    port_id: &str,
    index: &InputIndex<'_>,
    graph: &'g LGraph,
) -> ImportResult<(PortRef, &'g LPort)> {
    let missing_endpoint = || ImportError::MissingEndpoint {
        edge_id: edge.id.clone(),
        node_id: node_id.to_string(),
    };
    let port = index
        .port_ref(node_id, port_id)
        .ok_or_else(|| ImportError::MissingPort {
            edge_id: edge.id.clone(),
            node_id: node_id.to_string(),
            port_id: port_id.to_string(),
        })?;
    let node = graph
        .layerless_nodes
        .get_node(port.node)
        .ok_or_else(missing_endpoint)?;
    if node.id != node_id {
        return Err(missing_endpoint());
    }
    let found = node.ports.get(port.port).ok_or_else(missing_endpoint)?;
    Ok((port, found))
}

/// The port of the dummy that stands for a port of the node the run lays out.
///
/// Ports the lookup of `ElkGraphImporter.transformEdge`, where an external port maps to a node.
/// Without a dummy the node is unknown: `UnsupportedGraphException`.
fn scope_port_end(
    edge: &ElkInputEdge,
    node_id: &str,
    port_id: Option<&str>,
    index: &InputIndex<'_>,
) -> ImportResult<PortRef> {
    let port_id = port_id.ok_or_else(|| ImportError::MissingEndpoint {
        edge_id: edge.id.clone(),
        node_id: node_id.to_string(),
    })?;
    index
        .scope_dummies
        .get(port_id)
        .map(|&node| PortRef {
            node,
            port: PortId::new(0),
        })
        .ok_or_else(|| ImportError::ScopePortWithoutDummy {
            edge_id: edge.id.clone(),
        })
}

/// Preserve a hierarchy-crossing edge for ELK's compound preprocessor.
///
/// Source:
/// https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/graph/transform/ElkGraphImporter.java
fn transform_cross_hierarchy_edge(
    edge: &ElkInputEdge,
    arena: &mut HierarchicalImportArena,
    owner_slot: GraphId,
    index: &InputIndex<'_>,
    edge_order: usize,
    model_order: Option<usize>,
    port_index: &mut ImportPortIndex,
) -> ImportResult<()> {
    let merge_edges = arena.root().options.merge_edges;
    let source_parent = index.node_parent(edge.source.as_str());
    let target_parent = index.node_parent(edge.target.as_str());

    let source = hierarchy_edge_end(
        edge,
        edge.source.as_str(),
        edge.source_port.as_deref(),
        PortType::Output,
        "source",
        edge_order,
        merge_edges,
        index,
        arena,
        source_parent,
        port_index,
    )?;
    let target = hierarchy_edge_end(
        edge,
        edge.target.as_str(),
        edge.target_port.as_deref(),
        PortType::Input,
        "target",
        edge_order,
        merge_edges,
        index,
        arena,
        target_parent,
        port_index,
    )?;

    // `transformEdge`: a port with more than one incoming or more than one outgoing edge makes
    // the graph the edge sits in one with hyperedges. The graph is the one the edge belongs to,
    // not one of the levels its segments will sit in.
    let (source_incoming, source_outgoing) = {
        let entry = port_index
            .crossing_edges
            .entry(source.node_id.clone())
            .or_default()
            .of_mut(source.named)
            .entry(source.port_key.clone())
            .or_default();
        entry.1 += 1;
        *entry
    };
    let (target_incoming, target_outgoing) = {
        let entry = port_index
            .crossing_edges
            .entry(target.node_id.clone())
            .or_default()
            .of_mut(target.named)
            .entry(target.port_key.clone())
            .or_default();
        entry.0 += 1;
        *entry
    };
    let parallel = |attached: (usize, usize), incoming: usize, outgoing: usize| {
        attached.0 + incoming > 1 || attached.1 + outgoing > 1
    };
    if parallel(source.attached, source_incoming, source_outgoing)
        || parallel(target.attached, target_incoming, target_outgoing)
    {
        arena.graph_mut(owner_slot).graph_properties.hyperedges = true;
    }

    // The owned paths are part of ELK's compound-edge input, not graph locators. Build each once
    // and move it into the retained hierarchy edge so deep-chain cost tracks emitted path data.
    let steps = |path: Vec<String>| {
        path.into_iter()
            .map(|node| HierarchyStep {
                graph: arena.graph_slot_for_parent(index, Some(node.as_str())),
                node_id: node,
            })
            .collect::<Vec<_>>()
    };
    let source_path = steps(
        index
            .hierarchy_edge_output_path(edge.source.as_str())
            .unwrap_or_default(),
    );
    let target_path = steps(
        index
            .hierarchy_edge_output_path(edge.target.as_str())
            .unwrap_or_default(),
    );
    arena.root_mut().hierarchy_edges.push(HierarchyEdge {
        id: edge.id.clone(),
        source_node_id: source.node_id,
        target_node_id: target.node_id,
        source_port_key: source.port_key,
        target_port_key: target.port_key,
        source_port_named: source.named,
        target_port_named: target.named,
        source_path,
        target_path,
        labels: edge.labels.iter().map(label_to_lgraph).collect(),
        minlen: edge.minlen.max(1),
        model_order,
        priority_direction: edge.priority_direction,
        priority_shortness: edge.priority_shortness,
        priority_straightness: edge.priority_straightness,
    });

    Ok(())
}

/// How a hierarchy edge names one end: the node and the key of the port the end sits at.
struct HierarchyEdgeEnd {
    node_id: String,
    port_key: String,
    /// Whether the caller named the port. The key of a made-up port can equal the id of a named one.
    named: bool,
    /// The edges attached to the port by now, as (incoming, outgoing).
    attached: (usize, usize),
}

/// Finds or makes the port at one end of a hierarchy edge.
///
/// A port the caller named is the end as it is, in its own graph. A node without a named port gets
/// a port of its own, as `ElkGraphImporter.transformEdge` makes with `LGraphUtil.createPort`. A
/// port of the node the run lays out is the port of its external port dummy.
#[allow(clippy::too_many_arguments)]
fn hierarchy_edge_end(
    edge: &ElkInputEdge,
    node_id: &str,
    port_id: Option<&str>,
    port_type: PortType,
    role: &str,
    edge_order: usize,
    merge_edges: bool,
    index: &InputIndex<'_>,
    arena: &mut HierarchicalImportArena,
    graph_parent: Option<&str>,
    port_index: &mut ImportPortIndex,
) -> ImportResult<HierarchyEdgeEnd> {
    let missing_endpoint = || ImportError::MissingEndpoint {
        edge_id: edge.id.clone(),
        node_id: node_id.to_string(),
    };
    if node_id == index.scope_id && !index.nodes.contains_key(node_id) {
        // The dummy is a node of the root graph with the id of its port.
        let end = scope_port_end(edge, node_id, port_id, index)?;
        let dummy = &arena.root().layerless_nodes[end.node];
        let port = &dummy.ports[end.port];
        return Ok(HierarchyEdgeEnd {
            node_id: dummy.id.clone(),
            port_key: port.id.clone(),
            named: port.named,
            attached: (port.incoming_edges.len(), port.outgoing_edges.len()),
        });
    }
    let slot = arena.graph_slot_for_parent(index, graph_parent);
    if let Some(port_id) = port_id {
        let (_, attached) = named_port(edge, node_id, port_id, index, arena.graph(slot))?;
        return Ok(HierarchyEdgeEnd {
            node_id: node_id.to_string(),
            port_key: port_id.to_string(),
            named: true,
            attached: (attached.incoming_edges.len(), attached.outgoing_edges.len()),
        });
    }
    let port_key = hierarchy_port_key(node_id, edge_order, role, merge_edges, port_type);
    let end = ensure_hierarchy_endpoint_port(
        arena.graph_mut(slot),
        graph_parent,
        index.child_position(node_id),
        node_id,
        port_key.as_str(),
        port_type,
        port_index,
    )
    .ok_or_else(missing_endpoint)?;
    let port = &arena.graph(slot).layerless_nodes[end.node].ports[end.port];
    let attached = (port.incoming_edges.len(), port.outgoing_edges.len());
    Ok(HierarchyEdgeEnd {
        node_id: node_id.to_string(),
        port_key,
        named: false,
        attached,
    })
}

fn ensure_hierarchy_endpoint_port(
    graph: &mut LGraph,
    graph_parent: Option<&str>,
    node: NodeId,
    node_id: &str,
    port_key: &str,
    port_type: PortType,
    port_index: &mut ImportPortIndex,
) -> Option<PortRef> {
    if graph.options.merge_edges
        && graph.layerless_nodes.get_node(node).is_some_and(|candidate| {
            candidate.id == node_id && !candidate.port_constraints.is_side_fixed()
        })
    {
        return ensure_port_at_node(graph, graph_parent, node, node_id, port_type, port_index);
    }

    let candidate = graph.layerless_nodes.get_node(node)?;
    if candidate.id != node_id {
        return None;
    }

    let mut hierarchy_port = LPort::new(port_key.to_string(), node, port_type);
    // `LGraphUtil.createPort` gives the port the side of its type along the direction. A node
    // with fixed port sides keeps it. ELK makes the end of an edge into a descendant an input
    // port, but only a node with children has such an end, and its ports take the side of their
    // dummies in the end.
    let default_side = port_side_from_direction(graph.options.direction);
    hierarchy_port.set_side(match port_type {
        PortType::Output => default_side,
        PortType::Input => default_side.opposed(),
    });
    let port = graph.layerless_nodes[node].ports.push(hierarchy_port);
    // Dedicated hierarchy ports are never looked up again: only collector and inside-self-loop
    // identities participate in ELK's reuse rules. Indexing this edge-unique key would retain one
    // copied String and HashMap entry per endpoint without changing any later port selection.
    Some(PortRef { node, port })
}

fn hierarchy_port_key(
    node_id: &str,
    edge_order: usize,
    role: &str,
    merge_edges: bool,
    port_type: PortType,
) -> String {
    if merge_edges {
        // Collector identity is graph-local in ImportPortIndex, then keyed by node and direction.
        // Ignoring edge_order here is what lets every eligible parallel edge reuse that collector.
        format!("{node_id}:collector:{port_type:?}")
    } else {
        format!("{node_id}:{edge_order}:{role}")
    }
}

fn ensure_port_at_node(
    graph: &mut LGraph,
    graph_parent: Option<&str>,
    node: NodeId,
    node_id: &str,
    port_type: PortType,
    port_index: &mut ImportPortIndex,
) -> Option<PortRef> {
    let candidate = graph.layerless_nodes.get_node(node)?;
    if candidate.id != node_id {
        return None;
    }

    if graph.options.merge_edges && !graph.layerless_nodes[node].port_constraints.is_side_fixed() {
        let collector_key = hierarchy_port_key(node_id, 0, "collector", true, port_type);
        if let Some(port) = port_index.get(graph_parent, node, collector_key.as_str(), port_type) {
            return Some(port);
        }
        let default_side = port_side_from_direction(graph.options.direction);
        let side = match port_type {
            PortType::Output => default_side,
            PortType::Input => default_side.opposed(),
        };
        let port = graph.provide_collector_port(node, port_type, side);
        if let Some(port) = port {
            port_index.insert(graph_parent, node, collector_key.as_str(), port_type, port);
        }
        return port;
    }

    // `slot_count`, like `LGraph::add_port`: a port off the list still holds its name.
    let port_number = graph.layerless_nodes[node].ports.slot_count();
    let mut implicit_port = LPort::new(format!("{node_id}:{port_number:?}"), node, port_type);
    // Mirror Mermaid's ELK `LGraphUtil.createPort`: dedicated implicit ports receive
    // a direction-derived side before direction preprocessing/transposition runs.
    let default_side = port_side_from_direction(graph.options.direction);
    implicit_port.set_side(match port_type {
        PortType::Output => default_side,
        PortType::Input => default_side.opposed(),
    });
    let port = graph.layerless_nodes[node].ports.push(implicit_port);
    Some(PortRef { node, port })
}

/// `ElkGraphImporter.transformEdge` marks hyperedges when a port has more than one incoming
/// or more than one outgoing edge. One of each is not a hyperedge.
fn has_parallel_port_edges(port: &LPort) -> bool {
    port.incoming_edges.len() > 1 || port.outgoing_edges.len() > 1
}

fn label_to_lgraph(label: &ElkInputLabel) -> LLabel {
    let mut llabel = LLabel::new(label.text.clone(), label.width, label.height);
    llabel.source_index = label.source_index;
    llabel.placement = label.placement;
    llabel.inline = label.inline;
    llabel.label_side = None;
    llabel.node_label_placement = label.node_label_placement;
    llabel
}

fn node_materializes_children(
    index: &InputIndex<'_>,
    parent_options: &LayeredOptions,
    node: &ElkInputNode,
) -> bool {
    index.has_children(node.id.as_str())
        && resolve_child_hierarchy_handling(node.hierarchy_handling, parent_options)
            == HierarchyHandling::IncludeChildren
}

fn node_needs_nested_graph(
    index: &InputIndex<'_>,
    parent_options: &LayeredOptions,
    node: &ElkInputNode,
) -> bool {
    // SeparateChildren may still need a dummy-only nested graph for activated inside self-loops;
    // that must not be confused with permission to materialize the node's ordinary children.
    node_materializes_children(index, parent_options, node)
        || (parent_options.inside_self_loops_activate
            && index.inside_self_loop_nodes.contains(node.id.as_str()))
}

/// Source: `LGraphUtil.getDirection`, which `ElkGraphImporter.createLGraph` uses for a graph
/// without a direction.
pub(crate) fn resolve_direction(direction: ElkDirection, aspect_ratio: f64) -> ElkDirection {
    match direction {
        ElkDirection::Undefined if aspect_ratio >= 1.0 => ElkDirection::Right,
        ElkDirection::Undefined => ElkDirection::Down,
        direction => direction,
    }
}

/// Answers "is this node inside that node" in constant time, using each node's preorder position
/// and the end of its subtree's preorder range.
struct HierarchyQueryIndex {
    preorder_positions: Vec<usize>,
    subtree_ends: Vec<usize>,
}

impl HierarchyQueryIndex {
    fn new(
        node_order: &HashMap<&str, usize>,
        root_children: &[&ElkInputNode],
        children_by_parent: &HashMap<&str, Vec<&ElkInputNode>>,
    ) -> Self {
        let mut children = vec![Vec::new(); node_order.len()];
        for (parent, input_children) in children_by_parent {
            let parent = node_order[*parent];
            for child in input_children {
                children[parent].push(node_order[child.id.as_str()]);
            }
        }

        let mut preorder_positions = vec![0usize; node_order.len()];
        let mut preorder = Vec::with_capacity(node_order.len());
        let mut stack = root_children
            .iter()
            .rev()
            .map(|node| node_order[node.id.as_str()])
            .collect::<Vec<_>>();
        while let Some(node) = stack.pop() {
            preorder_positions[node] = preorder.len();
            preorder.push(node);
            stack.extend(children[node].iter().rev().copied());
        }

        let mut subtree_sizes = vec![1usize; node_order.len()];
        for &node in preorder.iter().rev() {
            for &child in &children[node] {
                subtree_sizes[node] += subtree_sizes[child];
            }
        }
        let subtree_ends = (0..node_order.len())
            .map(|node| preorder_positions[node] + subtree_sizes[node])
            .collect();

        Self {
            preorder_positions,
            subtree_ends,
        }
    }

    fn is_ancestor(&self, ancestor: usize, node: usize) -> bool {
        let position = self.preorder_positions[node];
        self.preorder_positions[ancestor] <= position && position < self.subtree_ends[ancestor]
    }
}

struct InputIndex<'a> {
    nodes: HashMap<&'a str, &'a ElkInputNode>,
    node_order: HashMap<&'a str, usize>,
    node_ids: Vec<&'a str>,
    root_children: Vec<&'a ElkInputNode>,
    children_by_parent: HashMap<&'a str, Vec<&'a ElkInputNode>>,
    child_positions: HashMap<&'a str, usize>,
    /// For each port id, the id of its node and its position among that node's ports.
    ports: HashMap<&'a str, (&'a str, usize)>,
    edges_by_parent: HashMap<Option<&'a str>, Vec<&'a ElkInputEdge>>,
    inside_self_loop_nodes: HashSet<&'a str>,
    root_options: LayeredOptions,
    effective_options: HashMap<&'a str, LayeredOptions>,
    hierarchy_queries: HierarchyQueryIndex,
    container_depths: Vec<Option<usize>>,
    first_unmaterialized_graph_parents: Vec<Option<&'a str>>,
    /// The id of the node the run lays out, whose ports are the graph's external ports.
    scope_id: &'a str,
    /// The node index of the dummy of each port of the scope, by the port's id.
    scope_dummies: HashMap<String, NodeId>,
}

impl<'a> InputIndex<'a> {
    fn new(input: &'a ElkInputGraph, scope_ports: &'a [ElkInputPort]) -> ImportResult<Self> {
        let mut nodes = HashMap::new();
        let mut node_order = HashMap::new();
        for (index, node) in input.nodes.iter().enumerate() {
            if nodes.insert(node.id.as_str(), node).is_some() {
                return Err(ImportError::DuplicateNode {
                    id: node.id.clone(),
                });
            }
            node_order.insert(node.id.as_str(), index);
        }

        // elkjs keeps its node and port ids apart, so a port may have the id of a node. The
        // edge's `source` names the node, and `source_port` the port.
        let mut ports: HashMap<&str, (&str, usize)> = HashMap::new();
        for node in &input.nodes {
            for (position, port) in node.ports.iter().enumerate() {
                if ports
                    .insert(port.id.as_str(), (node.id.as_str(), position))
                    .is_some()
                {
                    return Err(ImportError::DuplicatePort {
                        id: port.id.clone(),
                    });
                }
            }
        }

        for (position, port) in scope_ports.iter().enumerate() {
            if ports
                .insert(port.id.as_str(), (input.id.as_str(), position))
                .is_some()
            {
                return Err(ImportError::DuplicatePort {
                    id: port.id.clone(),
                });
            }
        }

        let mut root_children = Vec::new();
        let mut children_by_parent: HashMap<&str, Vec<&ElkInputNode>> = HashMap::new();
        let mut child_positions = HashMap::with_capacity(input.nodes.len());
        for node in &input.nodes {
            if let Some(parent) = node.parent.as_deref()
                && !nodes.contains_key(parent)
            {
                return Err(ImportError::MissingParent {
                    node_id: node.id.clone(),
                    parent_id: parent.to_string(),
                });
            }
            let children = match node.parent.as_deref() {
                Some(parent) => children_by_parent.entry(parent).or_default(),
                None => &mut root_children,
            };
            child_positions.insert(node.id.as_str(), children.len());
            children.push(node);
        }

        let mut edge_ids = HashSet::with_capacity(input.edges.len());
        for edge in &input.edges {
            if !edge_ids.insert(edge.id.as_str()) {
                return Err(ImportError::DuplicateEdge {
                    id: edge.id.clone(),
                });
            }
        }

        // An edge may end at a port of the node the run lays out, which is no node of the graph.
        let is_scope_port = |node: &str, port: &Option<String>| {
            node == input.id && port.is_some() && !nodes.contains_key(node)
        };
        for edge in &input.edges {
            if !nodes.contains_key(edge.source.as_str())
                && !is_scope_port(edge.source.as_str(), &edge.source_port)
            {
                return Err(ImportError::MissingEndpoint {
                    edge_id: edge.id.clone(),
                    node_id: edge.source.clone(),
                });
            }
            if !nodes.contains_key(edge.target.as_str())
                && !is_scope_port(edge.target.as_str(), &edge.target_port)
            {
                return Err(ImportError::MissingEndpoint {
                    edge_id: edge.id.clone(),
                    node_id: edge.target.clone(),
                });
            }
            for (node, port) in [
                (&edge.source, &edge.source_port),
                (&edge.target, &edge.target_port),
            ] {
                if let Some(port) = port
                    && ports.get(port.as_str()).map(|(owner, _)| *owner) != Some(node.as_str())
                {
                    return Err(ImportError::MissingPort {
                        edge_id: edge.id.clone(),
                        node_id: node.clone(),
                        port_id: port.clone(),
                    });
                }
            }
        }

        detect_parent_cycles(input, &node_order)?;
        let hierarchy_queries =
            HierarchyQueryIndex::new(&node_order, &root_children, &children_by_parent);
        let node_ids = input
            .nodes
            .iter()
            .map(|node| node.id.as_str())
            .collect::<Vec<_>>();

        let mut root_options = input.options.clone();
        root_options.direction =
            resolve_direction(root_options.direction, root_options.aspect_ratio);
        root_options.hierarchy_handling =
            resolve_root_hierarchy_handling(root_options.hierarchy_handling);
        let mut effective_options = HashMap::with_capacity(input.nodes.len());
        let mut option_search = root_children
            .iter()
            .rev()
            .copied()
            .map(|node| (node, root_options.clone()))
            .collect::<Vec<_>>();
        while let Some((node, parent_options)) = option_search.pop() {
            let options = nested_graph_options(&parent_options, node);
            effective_options.insert(node.id.as_str(), options.clone());
            option_search.extend(
                children_by_parent
                    .get(node.id.as_str())
                    .into_iter()
                    .flat_map(|children| children.iter().rev().copied())
                    .map(|child| (child, options.clone())),
            );
        }

        let inside_self_loop_nodes = input
            .edges
            .iter()
            .filter(|edge| edge.inside_self_loops_yo && edge.source == edge.target)
            .map(|edge| edge.source.as_str())
            .collect::<HashSet<_>>();
        let mut result = Self {
            nodes,
            node_order,
            node_ids,
            root_children,
            children_by_parent,
            child_positions,
            ports,
            edges_by_parent: HashMap::new(),
            inside_self_loop_nodes,
            root_options,
            effective_options,
            hierarchy_queries,
            container_depths: vec![None; input.nodes.len()],
            first_unmaterialized_graph_parents: vec![None; input.nodes.len()],
            scope_id: input.id.as_str(),
            scope_dummies: HashMap::new(),
        };
        result.index_materialized_reachability();
        for edge in &input.edges {
            let parent = input_edge_containing_parent(&result, edge);
            result.edges_by_parent.entry(parent).or_default().push(edge);
        }
        Ok(result)
    }

    /// Returns the stable input-order child slice without cloning the parent's membership vector.
    fn children(&self, parent: Option<&str>) -> &[&'a ElkInputNode] {
        match parent {
            Some(parent) => self
                .children_by_parent
                .get(parent)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            None => &self.root_children,
        }
    }

    fn node_parent(&self, id: &str) -> Option<&'a str> {
        self.nodes.get(id).and_then(|node| node.parent.as_deref())
    }

    fn edges<'index>(&'index self, parent: Option<&'a str>) -> &'index [&'a ElkInputEdge] {
        self.edges_by_parent
            .get(&parent)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn has_children(&self, node: &str) -> bool {
        self.children_by_parent
            .get(node)
            .is_some_and(|children| !children.is_empty())
    }

    /// The index of a node in its graph. The dummies of the external ports come first in the
    /// graph of the run, ahead of its children.
    fn child_position(&self, node: &str) -> NodeId {
        let position = *self
            .child_positions
            .get(node)
            .expect("validated input nodes have a stable sibling position");
        NodeId::new(if self.node_parent(node).is_none() {
            position + self.scope_dummies.len()
        } else {
            position
        })
    }

    /// Where a named port sits in its node, once `transform_node` has made the node's ports.
    fn port_ref(&self, node: &str, port: &str) -> Option<PortRef> {
        let &(owner, position) = self.ports.get(port)?;
        (owner == node).then(|| PortRef {
            node: self.child_position(node),
            port: PortId::new(position),
        })
    }

    fn options_for_parent(&self, parent: Option<&str>) -> &LayeredOptions {
        parent
            .and_then(|parent| self.effective_options.get(parent))
            .unwrap_or(&self.root_options)
    }

    fn materialized_graph_path(&self, node: &str) -> Option<Vec<&'a str>> {
        // This path is consumed component-by-component by compound segment materialization. Graph
        // lookup itself uses HierarchicalImportArena's constant-time slot locator.
        let depth = self.container_depth(node)?;
        let mut path = Vec::with_capacity(depth);
        let mut current = self.node_parent(node);
        while let Some(parent) = current {
            path.push(parent);
            current = self.node_parent(parent);
        }
        path.reverse();
        debug_assert_eq!(path.len(), depth);
        Some(path)
    }

    fn hierarchy_edge_output_path(&self, node: &str) -> Option<Vec<String>> {
        let depth = self.container_depth(node)?;
        let mut path = Vec::with_capacity(depth);
        let mut current = self.node_parent(node);
        while let Some(parent) = current {
            path.push(parent.to_string());
            current = self.node_parent(parent);
        }
        path.reverse();
        debug_assert_eq!(path.len(), depth);
        Some(path)
    }

    fn node_index(&self, node: &str) -> Option<usize> {
        self.node_order.get(node).copied()
    }

    fn is_node_ancestor(&self, ancestor: &str, node: &str) -> bool {
        let ancestor = self
            .node_index(ancestor)
            .expect("validated edge ancestor endpoint is indexed");
        let node = self
            .node_index(node)
            .expect("validated edge descendant endpoint is indexed");
        self.hierarchy_queries.is_ancestor(ancestor, node)
    }

    fn container_depth(&self, node: &str) -> Option<usize> {
        self.node_index(node)
            .and_then(|index| self.container_depths[index])
    }

    fn index_materialized_reachability(&mut self) {
        let include_children =
            self.root_options.hierarchy_handling == HierarchyHandling::IncludeChildren;
        let mut stack = self
            .root_children
            .iter()
            .rev()
            .copied()
            .map(|node| (node, Some(0usize), None))
            .collect::<Vec<_>>();
        while let Some((node, container_depth, first_unmaterialized_parent)) = stack.pop() {
            let node_index = self
                .node_index(node.id.as_str())
                .expect("indexed node order exists for every input node");
            self.container_depths[node_index] = container_depth;
            self.first_unmaterialized_graph_parents[node_index] = first_unmaterialized_parent;
            let materializes_children = include_children
                && container_depth.is_some()
                && node_materializes_children(
                    self,
                    self.options_for_parent(node.parent.as_deref()),
                    node,
                );
            let graph_depth = if include_children
                && container_depth.is_some()
                && node_needs_nested_graph(
                    self,
                    self.options_for_parent(node.parent.as_deref()),
                    node,
                ) {
                Some(container_depth.unwrap_or(0) + 1)
            } else {
                None
            };
            let child_container_depth = if materializes_children {
                graph_depth
            } else {
                None
            };
            let child_unmaterialized_parent = first_unmaterialized_parent
                .or_else(|| (!materializes_children).then_some(node.id.as_str()));
            stack.extend(
                self.children(Some(node.id.as_str()))
                    .iter()
                    .rev()
                    .copied()
                    .map(|child| (child, child_container_depth, child_unmaterialized_parent)),
            );
        }
    }

    fn scoped_endpoint_depth(
        &self,
        edge: &ElkInputEdge,
        endpoint: &ElkInputEdgeSegmentEndpoint,
    ) -> ImportResult<usize> {
        let ElkInputEdgeSegmentEndpoint::Node { id } = endpoint else {
            return Ok(0);
        };
        self.container_depth(id).ok_or_else(|| {
            let graph_parent = self
                .first_unmaterialized_graph_parent(id)
                .unwrap_or(id.as_str());
            ImportError::UnavailableSegmentScope {
                edge_id: edge.id.clone(),
                graph_parent: graph_parent.to_string(),
            }
        })
    }

    fn first_unmaterialized_graph_parent(&self, node: &str) -> Option<&'a str> {
        self.node_index(node)
            .and_then(|node| self.first_unmaterialized_graph_parents[node])
    }
}

fn detect_parent_cycles<'a>(
    input: &'a ElkInputGraph,
    node_order: &HashMap<&'a str, usize>,
) -> ImportResult<()> {
    let mut state = vec![0u8; input.nodes.len()];
    for start in 0..input.nodes.len() {
        if state[start] == 2 {
            continue;
        }
        let mut path = Vec::new();
        let mut current = Some(start);
        while let Some(index) = current {
            match state[index] {
                2 => break,
                1 => {
                    return Err(ImportError::ParentCycle {
                        node_id: input.nodes[index].id.clone(),
                    });
                }
                _ => {
                    state[index] = 1;
                    path.push(index);
                    current = input.nodes[index]
                        .parent
                        .as_deref()
                        .and_then(|parent| node_order.get(parent).copied());
                }
            }
        }
        for index in path {
            state[index] = 2;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compound::preprocess_source_ported_compound_graph;
    use crate::graph::LNodeKind;
    use crate::options::OrderingStrategy;
    use crate::options::SpacingOptions;

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

    #[test]
    fn parent_cycle_reports_the_revisited_cycle_member_after_a_stem() {
        let mut stem = node("stem");
        stem.parent = Some("A".to_string());
        let mut a = node("A");
        a.parent = Some("B".to_string());
        let mut b = node("B");
        b.parent = Some("A".to_string());
        let input = graph(vec![stem, a, b], vec![]);
        let node_order = input
            .nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.id.as_str(), index))
            .collect();

        assert!(matches!(
            detect_parent_cycles(&input, &node_order),
            Err(ImportError::ParentCycle { node_id }) if node_id == "A"
        ));
    }

    #[test]
    fn import_rejects_duplicate_input_edge_ids_before_endpoint_validation() {
        let input = graph(
            vec![node("A"), node("B")],
            vec![
                edge("duplicate", "A", "B"),
                edge("duplicate", "missing", "B"),
            ],
        );

        let error = import_graph(&input).unwrap_err();

        assert_eq!(
            error,
            ImportError::DuplicateEdge {
                id: "duplicate".to_string(),
            }
        );
    }

    #[test]
    fn hierarchical_import_uses_linear_graph_slots_for_deep_chains() {
        for node_count in [1usize, 4, 16, 64, 256] {
            let mut nodes = Vec::with_capacity(node_count);
            for index in 0..node_count {
                let mut current = node(format!("node-{index}").as_str());
                current.parent = index.checked_sub(1).map(|parent| format!("node-{parent}"));
                current.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
                nodes.push(current);
            }
            let mut input = graph(nodes, Vec::new());
            input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
            let imported = import_graph(&input).unwrap();

            let mut graph: &LGraph = &imported;
            for index in 0..node_count {
                assert_eq!(graph.layerless_nodes.len(), 1);
                assert_eq!(graph.layerless_nodes[0].id, format!("node-{index}"));
                if index + 1 < node_count {
                    graph = graph.layerless_nodes[0]
                        .nested_graph
                        .map(|id| &imported[id])
                        .expect("every non-leaf chain node owns the next graph slot");
                } else {
                    assert!(graph.layerless_nodes[0].nested_graph.is_none());
                }
            }
        }
    }

    #[test]
    fn hierarchical_import_keeps_the_full_path_of_each_cross_edge() {
        for node_count in [2usize, 8, 32] {
            for edge_count in [1usize, 4, 16] {
                let mut nodes = Vec::with_capacity(node_count);
                for index in 0..node_count {
                    let mut current = node(format!("node-{index}").as_str());
                    current.parent = index.checked_sub(1).map(|parent| format!("node-{parent}"));
                    current.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
                    nodes.push(current);
                }
                let edges = (0..edge_count)
                    .map(|index| {
                        edge(
                            format!("edge-{index}").as_str(),
                            "node-0",
                            format!("node-{}", node_count - 1).as_str(),
                        )
                    })
                    .collect::<Vec<_>>();
                let mut input = graph(nodes, edges);
                input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;

                let imported = import_graph(&input).unwrap();

                assert_eq!(imported.hierarchy_edges.len(), edge_count);
                assert!(imported.hierarchy_edges.iter().all(|edge| {
                    edge.source_path.is_empty() && edge.target_path.len() == node_count - 1
                }));
                assert_eq!(
                    imported
                        .hierarchy_edges
                        .iter()
                        .map(|edge| edge.id.clone())
                        .collect::<Vec<_>>(),
                    (0..edge_count)
                        .map(|index| format!("edge-{index}"))
                        .collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn hierarchy_query_index_answers_ancestry_across_branches_and_forests() {
        let mut left = node("left");
        left.parent = Some("root-a".to_string());
        let mut left_leaf = node("left-leaf");
        left_leaf.parent = Some("left".to_string());
        let mut right = node("right");
        right.parent = Some("root-a".to_string());
        let mut right_leaf = node("right-leaf");
        right_leaf.parent = Some("right".to_string());
        let input = graph(
            vec![
                node("root-a"),
                left,
                left_leaf,
                right,
                right_leaf,
                node("root-b"),
            ],
            Vec::new(),
        );
        let index = InputIndex::new(&input, &[]).unwrap();
        let root_a = index.node_index("root-a").unwrap();
        let left_leaf = index.node_index("left-leaf").unwrap();
        let right_leaf = index.node_index("right-leaf").unwrap();
        let root_b = index.node_index("root-b").unwrap();

        let queries = &index.hierarchy_queries;

        assert!(queries.is_ancestor(root_a, left_leaf));
        assert!(queries.is_ancestor(root_a, right_leaf));
        assert!(queries.is_ancestor(root_a, root_a));
        assert!(!queries.is_ancestor(left_leaf, root_a));
        assert!(!queries.is_ancestor(root_a, root_b));
        assert!(!queries.is_ancestor(root_b, left_leaf));
    }

    #[test]
    fn separate_children_self_loop_graph_does_not_materialize_regular_children() {
        let mut group = node("group");
        group.hierarchy_handling = Some(HierarchyHandling::SeparateChildren);
        let mut child = node("child");
        child.parent = Some("group".to_string());
        let loop_edge = ElkInputEdge {
            source_port: None,
            target_port: None,
            inside_self_loops_yo: true,
            ..edge("group-loop", "group", "group")
        };
        let mut input = graph(vec![group, child], vec![loop_edge]);
        input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
        input.options.inside_self_loops_activate = true;

        let imported = import_graph(&input).unwrap();
        let nested = imported.layerless_nodes[0]
            .nested_graph
            .map(|id| &imported[id])
            .expect("inside self-loop creates a nested graph");
        assert!(nested.layerless_nodes.iter().all(|node| node.id != "child"));
    }

    #[test]
    fn scoped_pieces_may_reference_the_same_logical_edge_id() {
        let input = graph(vec![node("A"), node("B")], Vec::new());
        let segment = |edge_order| ElkInputEdgeSegment {
            edge: edge("logical-edge", "A", "B"),
            source: ElkInputEdgeSegmentEndpoint::Node {
                id: "A".to_string(),
            },
            target: ElkInputEdgeSegmentEndpoint::Node {
                id: "B".to_string(),
            },
            segment: CompoundEdgeSegment::Output { depth: 0 },
            edge_order,
        };

        let imported = import_graph_with_segments(&input, &[segment(0), segment(1)]).unwrap();

        assert_eq!(imported.edges.len(), 2);
        assert_eq!(imported.cross_hierarchy_edges.len(), 2);
        assert!(
            imported
                .cross_hierarchy_edges
                .iter()
                .all(|segment| segment.original_edge_id == "logical-edge")
        );
    }

    fn cross_edge_import_input(merge_edges: bool, edge_count: usize) -> ElkInputGraph {
        let mut group = node("group");
        group.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        group.nested_options = Some(Box::new(LayeredOptions {
            merge_edges,
            ..Default::default()
        }));
        let mut child = node("child");
        child.parent = Some("group".to_string());
        let edges = (0..edge_count)
            .map(|index| edge(format!("edge-{index}").as_str(), "child", "outer"))
            .collect::<Vec<_>>();
        let mut input = graph(vec![group, child, node("outer")], edges);
        input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
        input.options.merge_edges = merge_edges;
        input
    }

    #[test]
    fn merged_cross_edges_share_one_port_per_endpoint() {
        let edge_count = 32usize;

        let imported = import_graph(&cross_edge_import_input(true, edge_count)).unwrap();

        let outer = imported
            .layerless_nodes
            .iter()
            .find(|node| node.id == "outer")
            .unwrap();
        assert_eq!(outer.ports.len(), 1);
        let nested = &imported[imported.layerless_nodes[0].nested_graph.unwrap()];
        let child = nested
            .layerless_nodes
            .iter()
            .find(|node| node.id == "child")
            .unwrap();
        assert_eq!(child.ports.len(), 1);
        assert_eq!(imported.hierarchy_edges.len(), edge_count);
    }

    #[test]
    fn dedicated_cross_edge_ports_are_not_reused() {
        let edge_count = 32usize;

        let imported = import_graph(&cross_edge_import_input(false, edge_count)).unwrap();

        let outer = imported
            .layerless_nodes
            .iter()
            .find(|node| node.id == "outer")
            .unwrap();
        assert_eq!(outer.ports.len(), edge_count);
        assert_eq!(imported.hierarchy_edges.len(), edge_count);
    }

    #[test]
    fn inside_self_loops_share_one_dummy_per_node_and_role() {
        let edge_count = 32usize;
        let edges = (0..edge_count)
            .map(|index| ElkInputEdge {
                source_port: None,
                target_port: None,
                inside_self_loops_yo: true,
                ..edge(format!("loop-{index}").as_str(), "A", "A")
            })
            .collect::<Vec<_>>();
        let mut input = graph(vec![node("A")], edges);
        input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
        input.options.inside_self_loops_activate = true;

        let imported = import_graph(&input).unwrap();

        let nested = &imported[imported.layerless_nodes[0].nested_graph.unwrap()];
        assert_eq!(nested.layerless_nodes.len(), 2);
        assert_eq!(nested.edges.len(), edge_count);
    }

    #[test]
    fn edge_labels_are_imported_with_their_edge() {
        let mut labeled_edge = edge("ab", "A", "B");
        labeled_edge.labels = (0..128)
            .map(|_| ElkInputLabel::center("terminal", 18.0, 16.5))
            .collect();
        let input = graph(vec![node("A"), node("B")], vec![labeled_edge]);

        let imported = import_graph(&input).unwrap();

        assert_eq!(imported.edges[0].labels.len(), 128);
    }

    #[test]
    fn scoped_segment_depth_that_overflows_when_rebased_is_an_error() {
        let mut nested_input = graph(vec![node("outer"), node("inner")], Vec::new());
        nested_input.nodes[1].parent = Some("outer".to_string());
        nested_input.nodes[0].hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        nested_input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
        let deep = ElkInputEdgeSegment {
            edge: edge("deep", "inner", "outer"),
            source: ElkInputEdgeSegmentEndpoint::Node {
                id: "inner".to_string(),
            },
            target: ElkInputEdgeSegmentEndpoint::Node {
                id: "outer".to_string(),
            },
            segment: CompoundEdgeSegment::Output { depth: usize::MAX },
            edge_order: 0,
        };

        assert_eq!(
            import_graph_with_segments(&nested_input, &[deep]).unwrap_err(),
            ImportError::SegmentDepthOverflow {
                edge_id: "deep".to_string(),
            }
        );
    }

    #[test]
    fn imports_mermaid_flowchart_nodes_edges_labels_and_model_order() {
        let mut a = node("A");
        a.labels = vec![ElkInputLabel::center("Alpha", 42.0, 18.0)];
        let mut ab = edge("A-B", "A", "B");
        ab.labels = vec![ElkInputLabel::center("go", 20.0, 12.0)];

        let lgraph = import_graph(&graph(vec![a, node("B")], vec![ab])).unwrap();

        assert_eq!(lgraph.layerless_nodes.len(), 2);
        assert_eq!(lgraph.layerless_nodes[0].id, "A");
        assert_eq!(lgraph.layerless_nodes[0].model_order, Some(0));
        assert_eq!(lgraph.layerless_nodes[0].labels[0].text, "Alpha");
        assert_eq!(lgraph.edges.len(), 1);
        assert_eq!(lgraph.edges[0].model_order, Some(0));
        assert_eq!(lgraph.edges[0].thickness, 1.0);
        assert_eq!(
            lgraph.edges[0].labels[0].placement,
            EdgeLabelPlacement::Center
        );
        assert!(lgraph.graph_properties.center_labels);
        assert!(lgraph.options.graph_has_center_labels);
    }

    #[test]
    fn importer_creates_nested_graph_for_inside_self_loop() {
        let mut graph = graph(
            vec![node("A")],
            vec![ElkInputEdge {
                source_port: None,
                target_port: None,
                inside_self_loops_yo: true,
                ..edge("A-A", "A", "A")
            }],
        );
        graph.options.inside_self_loops_activate = true;

        let lgraph = import_graph(&graph).unwrap();
        let node = lgraph
            .layerless_nodes
            .iter()
            .find(|node| node.id == "A")
            .unwrap();
        assert!(node.nested_graph.is_some());
    }

    #[test]
    fn importer_reuses_inside_self_loop_dummies_per_node_role() {
        let mut graph = graph(
            vec![node("A")],
            vec![
                ElkInputEdge {
                    source_port: None,
                    target_port: None,
                    inside_self_loops_yo: true,
                    ..edge("A-A-1", "A", "A")
                },
                ElkInputEdge {
                    source_port: None,
                    target_port: None,
                    inside_self_loops_yo: true,
                    ..edge("A-A-2", "A", "A")
                },
            ],
        );
        graph.options.inside_self_loops_activate = true;

        let lgraph = import_graph(&graph).unwrap();
        let node = lgraph
            .layerless_nodes
            .iter()
            .find(|node| node.id == "A")
            .unwrap();
        let nested = &lgraph[node.nested_graph.unwrap()];
        let external_dummies = nested
            .layerless_nodes
            .iter()
            .filter(|candidate| candidate.kind == LNodeKind::ExternalPort)
            .collect::<Vec<_>>();

        assert_eq!(external_dummies.len(), 2);
        let input_dummy = external_dummies
            .iter()
            .find(|dummy| dummy.parent_port_type == Some(PortType::Input))
            .expect("inside self-loop input dummy should exist");
        let output_dummy = external_dummies
            .iter()
            .find(|dummy| dummy.parent_port_type == Some(PortType::Output))
            .expect("inside self-loop output dummy should exist");
        assert_eq!(input_dummy.ports[PortId::new(0)].incoming_edges.len(), 2);
        assert_eq!(output_dummy.ports[PortId::new(0)].outgoing_edges.len(), 2);
    }

    #[test]
    fn importer_marks_inside_self_loops_as_graph_self_loops() {
        let mut graph = graph(
            vec![node("A")],
            vec![ElkInputEdge {
                source_port: None,
                target_port: None,
                inside_self_loops_yo: true,
                ..edge("A-A", "A", "A")
            }],
        );
        graph.options.inside_self_loops_activate = true;

        let lgraph = import_graph(&graph).unwrap();
        let nested = lgraph
            .layerless_nodes
            .iter()
            .find(|node| node.id == "A")
            .and_then(|node| node.nested_graph.map(|id| &lgraph[id]))
            .expect("inside self-loop should create a nested graph");

        assert!(nested.graph_properties.self_loops);
        assert!(nested.options.graph_has_self_loops);
    }

    #[test]
    fn importer_applies_layered_padding_option_to_lgraph_padding() {
        let mut input = graph(vec![node("A")], vec![]);
        input.options.padding = ElkPadding {
            top: 7.0,
            right: 8.0,
            bottom: 9.0,
            left: 10.0,
        };

        let lgraph = import_graph(&input).unwrap();

        assert_eq!(lgraph.padding.top, 7.0);
        assert_eq!(lgraph.padding.right, 8.0);
        assert_eq!(lgraph.padding.bottom, 9.0);
        assert_eq!(lgraph.padding.left, 10.0);
    }

    #[test]
    fn importer_applies_layered_padding_option_to_nested_graphs() {
        let mut cluster = node("cluster");
        cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut child = node("A");
        child.parent = Some("cluster".to_string());

        let lgraph = import_graph(&graph(vec![cluster, child], vec![])).unwrap();
        let nested = &lgraph[lgraph.layerless_nodes[0].nested_graph.unwrap()];

        assert_eq!(lgraph.padding.top, 12.0);
        assert_eq!(lgraph.padding.left, 12.0);
        assert_eq!(nested.padding.top, 12.0);
        assert_eq!(nested.padding.left, 12.0);
    }

    #[test]
    fn importer_adds_inside_top_node_label_padding_to_nested_graphs() {
        let mut cluster = node("cluster");
        cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        cluster.node_label_placement =
            NodeLabelPlacement::from_location(crate::options::NodeLabelLocation::InsideTopCenter);
        cluster.labels = vec![ElkInputLabel::center("Cluster", 64.0, 22.0)];
        cluster.nested_options = Some(Box::new(LayeredOptions {
            spacing: SpacingOptions::layered_base_value(30.0),
            ..Default::default()
        }));
        let mut child = node("A");
        child.parent = Some("cluster".to_string());

        let lgraph = import_graph(&graph(vec![cluster, child], vec![])).unwrap();
        let nested = &lgraph[lgraph.layerless_nodes[0].nested_graph.unwrap()];

        assert_eq!(nested.options.spacing.node_node, 30.0);
        assert_eq!(nested.padding.top, 39.0);
        assert_eq!(nested.padding.right, 12.0);
        assert_eq!(nested.padding.bottom, 12.0);
        assert_eq!(nested.padding.left, 12.0);
    }

    #[test]
    fn importer_applies_nested_hierarchy_merge_without_cloning_root_options() {
        let mut cluster = node("cluster");
        cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        cluster.nested_options = Some(Box::new(LayeredOptions {
            spacing: SpacingOptions::layered_base_value(30.0),
            layering_strategy: crate::options::LayeringStrategy::LongestPath,
            random_seed: 17,
            ..Default::default()
        }));
        let mut child = node("A");
        child.parent = Some("cluster".to_string());
        let mut input = graph(vec![cluster, child], vec![]);
        input.options.merge_hierarchy_edges = false;
        input.options.consider_model_order_strategy = OrderingStrategy::NodesAndEdges;
        input.options.spacing = SpacingOptions::layered_base_value(40.0);
        input.options.layering_strategy = crate::options::LayeringStrategy::CoffmanGraham;
        input.options.random_seed = 42;

        let lgraph = import_graph(&input).unwrap();
        let nested = &lgraph[lgraph.layerless_nodes[0].nested_graph.unwrap()];

        assert_eq!(lgraph.layerless_nodes[0].model_order, Some(0));
        assert_eq!(nested.layerless_nodes[0].model_order, None);
        assert!(nested.options.merge_hierarchy_edges);
        assert_eq!(
            nested.options.consider_model_order_strategy,
            OrderingStrategy::None
        );
        assert_eq!(nested.options.spacing.node_node, 30.0);
        assert_eq!(
            nested.options.layering_strategy,
            crate::options::LayeringStrategy::LongestPath
        );
        assert_eq!(nested.options.random_seed, 17);
    }

    #[test]
    fn importer_copies_node_port_constraints_to_nested_graph_options() {
        let mut cluster = node("cluster");
        cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        cluster.port_constraints = Some(PortConstraints::FixedSide);
        let mut child = node("A");
        child.parent = Some("cluster".to_string());

        let lgraph = import_graph(&graph(vec![cluster, child], vec![])).unwrap();
        let nested = &lgraph[lgraph.layerless_nodes[0].nested_graph.unwrap()];

        assert_eq!(nested.options.port_constraints, PortConstraints::FixedSide);
    }

    #[test]
    fn importer_forces_nested_direction_to_parent_direction() {
        let mut cluster = node("cluster");
        cluster.direction = Some(ElkDirection::Left);
        cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut child = node("A");
        child.parent = Some("cluster".to_string());
        let mut input = graph(vec![cluster, child], vec![]);
        input.options.direction = ElkDirection::Down;

        let lgraph = import_graph(&input).unwrap();
        let nested = &lgraph[lgraph.layerless_nodes[0].nested_graph.unwrap()];

        assert_eq!(nested.options.direction, ElkDirection::Down);
    }

    #[test]
    fn importer_resolves_root_hierarchy_inherit_to_separate_children() {
        let mut input = graph(vec![node("cluster"), node("A")], vec![]);
        input.options.hierarchy_handling = HierarchyHandling::Inherit;
        input.nodes[1].parent = Some("cluster".to_string());

        let lgraph = import_graph(&input).unwrap();

        assert_eq!(
            lgraph.options.hierarchy_handling,
            HierarchyHandling::SeparateChildren
        );
        assert!(lgraph.layerless_nodes.iter().all(|node| !node.compound));
    }

    #[test]
    fn imports_include_children_hierarchy_into_nested_graphs() {
        let mut cluster = node("cluster");
        cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut child = node("A");
        child.parent = Some("cluster".to_string());
        let lgraph = import_graph(&graph(vec![cluster, child, node("B")], vec![])).unwrap();

        let cluster = lgraph
            .layerless_nodes
            .iter()
            .find(|node| node.id == "cluster")
            .unwrap();
        let nested = &lgraph[cluster.nested_graph.unwrap()];
        assert_eq!(
            nested.parent,
            Some(crate::graph::GraphParent {
                graph: GraphId::ROOT,
                node: NodeId::new(lgraph
                    .layerless_nodes
                    .iter()
                    .position(|n| n.id == "cluster")
                    .unwrap()),
            })
        );
        assert_eq!(nested.layerless_nodes[0].id, "A");
    }

    #[test]
    fn importer_preserves_descendant_edge_for_compound_preprocessor() {
        let mut cluster = node("cluster");
        cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut child = node("A");
        child.parent = Some("cluster".to_string());

        let lgraph = import_graph(&graph(
            vec![cluster, child],
            vec![edge("cluster-A", "cluster", "A")],
        ))
        .unwrap();
        let cluster = lgraph
            .layerless_nodes
            .iter()
            .find(|node| node.id == "cluster")
            .unwrap();
        let nested = &lgraph[cluster.nested_graph.unwrap()];
        assert!(nested.edges.is_empty());
        assert_eq!(lgraph.hierarchy_edges.len(), 1);
        assert_eq!(lgraph.hierarchy_edges[0].id, "cluster-A");
        assert_eq!(lgraph.hierarchy_edges[0].source_node_id, "cluster");
        assert_eq!(lgraph.hierarchy_edges[0].target_node_id, "A");
        assert_eq!(lgraph.hierarchy_edges[0].source_path, Vec::new());
        assert_eq!(
            lgraph.hierarchy_edges[0].target_path,
            vec![HierarchyStep {
                node_id: "cluster".to_string(),
                graph: nested.id,
            }]
        );
    }

    #[test]
    fn cross_hierarchy_import_creates_endpoint_ports_in_input_order() {
        let mut cluster = node("cluster");
        cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut child = node("A");
        child.parent = Some("cluster".to_string());
        let mut sibling = node("C");
        sibling.parent = Some("cluster".to_string());

        let lgraph = import_graph(&graph(
            vec![cluster, child, sibling, node("B")],
            vec![edge("A-B", "A", "B"), edge("A-C", "A", "C")],
        ))
        .unwrap();

        let nested = lgraph
            .layerless_nodes
            .iter()
            .find(|node| node.id == "cluster")
            .unwrap()
            .nested_graph
            .map(|id| &lgraph[id])
            .unwrap();
        let child = nested
            .layerless_nodes
            .iter()
            .find(|node| node.id == "A")
            .unwrap();

        assert_eq!(
            child
                .ports
                .iter()
                .map(|port| port.id.as_str())
                .collect::<Vec<_>>(),
            vec!["A:0:source", "A:1"]
        );
        assert_eq!(nested.edges[0].source.port, PortId::new(1));
        assert_eq!(lgraph.hierarchy_edges[0].source_port_key, "A:0:source");
    }

    #[test]
    fn source_ported_compound_metadata_links_parent_port_and_external_dummy() {
        let mut cluster = node("cluster");
        cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut child = node("A");
        child.parent = Some("cluster".to_string());

        let mut lgraph = import_graph(&graph(
            vec![cluster, child, node("B")],
            vec![edge("A-B", "A", "B")],
        ))
        .unwrap();
        preprocess_source_ported_compound_graph(&mut lgraph);

        let cluster_index = NodeId::new(lgraph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "cluster")
            .unwrap());
        let cluster = &lgraph.layerless_nodes[cluster_index];
        let parent_port = &cluster.ports[PortId::new(0)];
        let port_dummy = parent_port
            .port_dummy
            .as_ref()
            .expect("compound port should point to nested external dummy");
        assert!(parent_port.inside_connections);
        let nested = &lgraph[cluster.nested_graph.unwrap()];
        assert_eq!(port_dummy.graph_id, nested.id);
        let external = &nested.layerless_nodes[port_dummy.node];
        assert_eq!(external.external_port_side, PortSide::South);
        assert_eq!(
            parent_port.border_offset,
            external.ports[PortId::new(0)].border_offset
        );
        assert_eq!(
            parent_port.border_offset,
            Some(nested.options.spacing.edge_edge / 2.0)
        );
        let origin = external
            .origin_port
            .as_ref()
            .expect("external dummy should point back to parent port");
        assert_eq!(origin.graph_id, GraphId::ROOT);
        assert_eq!(origin.port.node, cluster_index);
        assert_eq!(origin.port.port, PortId::new(0));
    }

    #[test]
    fn source_ported_compound_does_not_duplicate_existing_parent_dummy() {
        let mut cluster = node("cluster");
        cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut child = node("A");
        child.parent = Some("cluster".to_string());

        let mut lgraph = import_graph(&graph(
            vec![node("B"), cluster, child],
            vec![edge("B-A", "B", "A")],
        ))
        .unwrap();
        preprocess_source_ported_compound_graph(&mut lgraph);

        let cluster = lgraph
            .layerless_nodes
            .iter()
            .find(|node| node.id == "cluster")
            .unwrap();
        let nested = &lgraph[cluster.nested_graph.unwrap()];
        let external_dummies = nested
            .nodes_with_ids()
            .filter(|(_, node)| {
                node.kind == LNodeKind::ExternalPort && node.id == "external:cluster"
            })
            .collect::<Vec<_>>();

        assert_eq!(external_dummies.len(), 1);
        let (dummy_node, external_dummy) = external_dummies[0];
        let dummy_port = external_dummy.ports.first().unwrap();
        assert_eq!(
            dummy_port.incoming_edges.len() + dummy_port.outgoing_edges.len(),
            1
        );
        assert_eq!(cluster.ports.len(), 1);
        assert_eq!(
            cluster.ports[PortId::new(0)]
                .port_dummy
                .as_ref()
                .unwrap()
                .node,
            dummy_node
        );
    }

    #[test]
    fn source_ported_compound_metadata_links_parent_to_child_external_dummy() {
        let mut cluster = node("cluster");
        cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut child = node("A");
        child.parent = Some("cluster".to_string());

        let mut lgraph = import_graph(&graph(
            vec![cluster, child],
            vec![edge("cluster-A", "cluster", "A")],
        ))
        .unwrap();
        preprocess_source_ported_compound_graph(&mut lgraph);

        let cluster_index = NodeId::new(lgraph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "cluster")
            .unwrap());
        let cluster = &lgraph.layerless_nodes[cluster_index];
        let parent_port = cluster
            .ports
            .iter()
            .find(|port| port.port_dummy.is_some())
            .expect("parent-to-child edge should create a parent external port");
        assert_eq!(cluster.ports.len(), 1);
        assert_eq!(parent_port.id, "cluster:0:source");
        assert_eq!(parent_port.port_type, PortType::Output);
        let port_dummy = parent_port.port_dummy.as_ref().unwrap();
        assert!(parent_port.inside_connections);
        let nested = &lgraph[cluster.nested_graph.unwrap()];
        assert_eq!(port_dummy.graph_id, nested.id);
        let external = &nested.layerless_nodes[port_dummy.node];
        let origin = external
            .origin_port
            .as_ref()
            .expect("external dummy should point back to parent port");
        assert_eq!(origin.graph_id, GraphId::ROOT);
        assert_eq!(origin.port.node, cluster_index);
        assert_eq!(origin.port.port, PortId::new(0));
    }

    #[test]
    fn source_ported_compound_parent_boundary_segments_use_external_port_dummies() {
        let mut cluster = node("cluster");
        cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut child = node("A");
        child.parent = Some("cluster".to_string());

        let mut lgraph = import_graph(&graph(
            vec![cluster, child],
            vec![edge("cluster-A", "cluster", "A")],
        ))
        .unwrap();
        preprocess_source_ported_compound_graph(&mut lgraph);
        let cluster = lgraph
            .layerless_nodes
            .iter()
            .find(|node| node.id == "cluster")
            .unwrap();
        let nested = &lgraph[cluster.nested_graph.unwrap()];
        let segment = nested
            .edges
            .iter()
            .find(|edge| edge.id == "cluster-A")
            .unwrap();

        assert_eq!(
            nested.layerless_nodes[segment.source.node].kind,
            LNodeKind::ExternalPort
        );
        assert_eq!(nested.layerless_nodes[segment.target.node].id, "A");
    }

    #[test]
    fn source_ported_compound_import_records_cross_hierarchy_segments() {
        let mut outer = node("outer");
        outer.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut inner = node("inner");
        inner.parent = Some("outer".to_string());
        inner.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut child = node("A");
        child.parent = Some("inner".to_string());

        let mut lgraph = import_graph(&graph(
            vec![outer, inner, child, node("B")],
            vec![edge("A-B", "A", "B")],
        ))
        .unwrap();
        preprocess_source_ported_compound_graph(&mut lgraph);

        let outer = lgraph
            .layerless_nodes
            .iter()
            .find(|node| node.id == "outer")
            .unwrap();
        let inner_graph = outer
            .nested_graph
            .map(|id| &lgraph[id])
            .unwrap()
            .layerless_nodes
            .iter()
            .find(|node| node.id == "inner")
            .unwrap()
            .nested_graph
            .map(|id| &lgraph[id])
            .unwrap();
        let outer_graph = &lgraph[outer.nested_graph.unwrap()];
        assert_eq!(
            inner_graph.cross_hierarchy_edges[0].segment,
            CompoundEdgeSegment::Output { depth: 2 }
        );
        assert_eq!(
            outer_graph.cross_hierarchy_edges[0].segment,
            CompoundEdgeSegment::Output { depth: 1 }
        );
        assert_eq!(
            lgraph.cross_hierarchy_edges[0].segment,
            CompoundEdgeSegment::Output { depth: 0 }
        );
    }

    #[test]
    fn source_ported_compound_reuses_exported_external_port_when_hierarchy_edges_merge() {
        let mut cluster = node("cluster");
        cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut child = node("A");
        child.parent = Some("cluster".to_string());
        let mut first = edge("A-B", "A", "B");
        let mut first_label = ElkInputLabel::center("first", 12.0, 6.0);
        first_label.placement = EdgeLabelPlacement::Tail;
        first.labels = vec![first_label];
        let mut second = edge("A-C", "A", "C");
        let mut second_label = ElkInputLabel::center("second", 18.0, 6.0);
        second_label.placement = EdgeLabelPlacement::Tail;
        second.labels = vec![second_label];
        let mut input = graph(
            vec![cluster, child, node("B"), node("C")],
            vec![first, second],
        );
        input.options.merge_edges = true;
        input.options.merge_hierarchy_edges = true;

        let mut lgraph = import_graph(&input).unwrap();
        preprocess_source_ported_compound_graph(&mut lgraph);

        let nested = lgraph
            .layerless_nodes
            .iter()
            .find(|node| node.id == "cluster")
            .unwrap()
            .nested_graph
            .map(|id| &lgraph[id])
            .unwrap();
        assert_eq!(nested.edges.len(), 1);
        assert_eq!(nested.cross_hierarchy_edges.len(), 2);
        assert!(
            nested
                .cross_hierarchy_edges
                .iter()
                .all(|segment| segment.edge == 0)
        );
        assert_eq!(nested.edges[0].labels.len(), 2);
        assert_eq!(
            nested.edges[0]
                .labels
                .iter()
                .filter_map(|label| label.original_label_edge.as_deref())
                .collect::<Vec<_>>(),
            vec!["A-B", "A-C"]
        );
        assert!(nested.graph_properties.end_labels);
    }

    #[test]
    fn source_ported_compound_keeps_nested_hierarchy_merge_default_when_root_disables_merge() {
        let mut cluster = node("cluster");
        cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut child = node("A");
        child.parent = Some("cluster".to_string());
        let mut input = graph(
            vec![cluster, child, node("B"), node("C")],
            vec![edge("A-B", "A", "B"), edge("A-C", "A", "C")],
        );
        input.options.merge_edges = true;
        input.options.merge_hierarchy_edges = false;

        let mut lgraph = import_graph(&input).unwrap();
        preprocess_source_ported_compound_graph(&mut lgraph);

        let nested = lgraph
            .layerless_nodes
            .iter()
            .find(|node| node.id == "cluster")
            .unwrap()
            .nested_graph
            .map(|id| &lgraph[id])
            .unwrap();
        assert_eq!(nested.edges.len(), 1);
        assert_eq!(
            nested
                .cross_hierarchy_edges
                .iter()
                .map(|segment| segment.edge)
                .collect::<Vec<_>>(),
            vec![0, 0]
        );
    }

    #[test]
    fn source_ported_compound_parent_end_segments_are_not_reused() {
        let mut cluster = node("cluster");
        cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut child = node("A");
        child.parent = Some("cluster".to_string());
        let mut input = graph(
            vec![cluster, child],
            vec![
                edge("cluster-A-1", "cluster", "A"),
                edge("cluster-A-2", "cluster", "A"),
            ],
        );
        input.options.merge_edges = true;
        input.options.merge_hierarchy_edges = true;

        let mut lgraph = import_graph(&input).unwrap();
        preprocess_source_ported_compound_graph(&mut lgraph);

        let nested = lgraph
            .layerless_nodes
            .iter()
            .find(|node| node.id == "cluster")
            .unwrap()
            .nested_graph
            .map(|id| &lgraph[id])
            .unwrap();
        assert_eq!(nested.edges.len(), 2);
        assert_eq!(
            nested
                .cross_hierarchy_edges
                .iter()
                .map(|segment| segment.edge)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert!(
            nested.edges.iter().all(
                |edge| nested.layerless_nodes[edge.source.node].kind == LNodeKind::ExternalPort
            )
        );
    }

    #[test]
    fn importer_reuses_collector_ports_when_edges_are_merged() {
        let mut input = graph(
            vec![node("A"), node("B"), node("C")],
            vec![edge("A-B", "A", "B"), edge("A-C", "A", "C")],
        );
        input.options.merge_edges = true;

        let lgraph = import_graph(&input).unwrap();

        let a = NodeId::new(lgraph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "A")
            .unwrap());
        assert_eq!(lgraph.layerless_nodes[a].ports.len(), 1);
        assert_eq!(
            lgraph.layerless_nodes[a].ports[PortId::new(0)].collector_type,
            Some(PortType::Output)
        );
        assert_eq!(
            lgraph.layerless_nodes[a].ports[PortId::new(0)].outgoing_edges,
            vec![0, 1]
        );
        assert!(lgraph.graph_properties.hyperedges);
        assert!(lgraph.options.graph_has_hyperedges);
    }

    #[test]
    fn importer_keeps_dedicated_ports_when_edge_merge_is_disabled() {
        let lgraph = import_graph(&graph(
            vec![node("A"), node("B"), node("C")],
            vec![edge("A-B", "A", "B"), edge("A-C", "A", "C")],
        ))
        .unwrap();

        let a = NodeId::new(lgraph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "A")
            .unwrap());
        assert_eq!(lgraph.layerless_nodes[a].ports.len(), 2);
        assert!(
            lgraph.layerless_nodes[a]
                .ports
                .iter()
                .all(|port| port.collector_type.is_none())
        );
        assert!(!lgraph.graph_properties.hyperedges);
    }

    // Mutation: name a made-up port after `ports.len()` instead of `ports.slot_count()`.
    #[test]
    fn a_made_up_port_gets_a_name_no_unlisted_port_has() {
        let mut lgraph = LGraph::new("root", LayeredOptions::default());
        lgraph.options.merge_edges = false;
        lgraph
            .layerless_nodes
            .push(LNode::new("A", 10.0, 10.0, None));
        let first = lgraph.add_port(NodeId::new(0), PortType::Output, PortSide::East, Default::default());
        lgraph.add_port(NodeId::new(0), PortType::Output, PortSide::East, Default::default());
        lgraph.layerless_nodes[0]
            .ports
            .unlist(&[first.unwrap().port]);

        let made_up = ensure_port_at_node(
            &mut lgraph,
            None,
            NodeId::new(0),
            "A",
            PortType::Output,
            &mut ImportPortIndex::new(),
        )
        .unwrap();

        let ports = &lgraph.layerless_nodes[0].ports;
        assert_eq!(ports.slot_count(), 3);
        let made_up_name = &ports[made_up.port].id;
        let same_name = (0..ports.slot_count())
            .filter(|index| ports[PortId::new(*index)].id == *made_up_name)
            .count();
        assert_eq!(same_name, 1);
    }

    #[test]
    fn implicit_ports_have_directional_sides_before_preprocessing() {
        for (direction, output_side, input_side) in [
            (ElkDirection::Right, PortSide::East, PortSide::West),
            (ElkDirection::Left, PortSide::West, PortSide::East),
            (ElkDirection::Down, PortSide::South, PortSide::North),
            (ElkDirection::Up, PortSide::North, PortSide::South),
        ] {
            let mut input = graph(
                vec![node("A"), node("B")],
                vec![edge("first", "A", "B"), edge("second", "A", "B")],
            );
            input.options.direction = direction;
            let imported = import_graph(&input).unwrap();
            let source = imported
                .layerless_nodes
                .iter()
                .find(|node| node.id == "A")
                .unwrap();
            let target = imported
                .layerless_nodes
                .iter()
                .find(|node| node.id == "B")
                .unwrap();
            assert_eq!(source.ports.len(), 2);
            assert_eq!(target.ports.len(), 2);
            for index in 0..2 {
                let port = PortId::new(index);
                assert_eq!(source.ports[port].side, output_side, "{direction:?}");
                assert_eq!(target.ports[port].side, input_side, "{direction:?}");
                assert_eq!(source.ports[port].outgoing_edges, vec![index]);
                assert_eq!(target.ports[port].incoming_edges, vec![index]);
            }
        }
    }

    #[test]
    fn importer_keeps_dedicated_ports_when_node_port_constraints_are_side_fixed() {
        let mut a = node("A");
        a.port_constraints = Some(PortConstraints::FixedSide);
        let mut input = graph(
            vec![a, node("B"), node("C")],
            vec![edge("A-B", "A", "B"), edge("A-C", "A", "C")],
        );
        input.options.merge_edges = true;

        let lgraph = import_graph(&input).unwrap();

        let a = NodeId::new(lgraph
            .layerless_nodes
            .iter()
            .position(|node| node.id == "A")
            .unwrap());
        assert_eq!(lgraph.layerless_nodes[a].ports.len(), 2);
        assert!(
            lgraph.layerless_nodes[a]
                .ports
                .iter()
                .all(|port| port.collector_type.is_none())
        );
    }

    #[test]
    fn import_rejects_invalid_parent_and_endpoints() {
        let mut child = node("A");
        child.parent = Some("missing".to_string());
        assert!(matches!(
            import_graph(&graph(vec![child], vec![])),
            Err(ImportError::MissingParent { .. })
        ));

        assert!(matches!(
            import_graph(&graph(vec![node("A")], vec![edge("A-B", "A", "B")])),
            Err(ImportError::MissingEndpoint { .. })
        ));
    }

    #[test]
    fn scoped_segment_import_rejects_an_unmaterialized_owner_scope() {
        let group = node("group");
        let mut child = node("child");
        child.parent = Some("group".to_string());
        let mut input = graph(vec![group, child, node("outer")], vec![]);
        input.options.hierarchy_handling = HierarchyHandling::SeparateChildren;
        let segment = ElkInputEdgeSegment {
            edge: edge("child-outer", "child", "outer"),
            source: ElkInputEdgeSegmentEndpoint::Node {
                id: "child".to_string(),
            },
            target: ElkInputEdgeSegmentEndpoint::Node {
                id: "outer".to_string(),
            },
            segment: CompoundEdgeSegment::Output { depth: 0 },
            edge_order: 0,
        };

        let error = import_graph_with_segments(&input, &[segment])
            .expect_err("a flat import must reject a segment owned by a nested scope");

        assert_eq!(
            error,
            ImportError::UnavailableSegmentScope {
                edge_id: "child-outer".to_string(),
                graph_parent: "group".to_string(),
            }
        );
    }

    #[test]
    fn scoped_segment_import_rejects_a_scope_below_an_unmaterialized_ancestor() {
        let mut outer = node("outer-group");
        outer.hierarchy_handling = Some(HierarchyHandling::SeparateChildren);
        let mut inner = node("inner-group");
        inner.parent = Some("outer-group".to_string());
        inner.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut child = node("child");
        child.parent = Some("inner-group".to_string());
        let mut input = graph(vec![outer, inner, child, node("root-node")], vec![]);
        input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
        let segment = ElkInputEdgeSegment {
            edge: edge("child-root", "child", "root-node"),
            source: ElkInputEdgeSegmentEndpoint::Node {
                id: "child".to_string(),
            },
            target: ElkInputEdgeSegmentEndpoint::Node {
                id: "root-node".to_string(),
            },
            segment: CompoundEdgeSegment::Output { depth: 0 },
            edge_order: 0,
        };

        let error = import_graph_with_segments(&input, &[segment]).expect_err(
            "a locally includable scope below a SeparateChildren ancestor is unreachable",
        );

        assert_eq!(
            error,
            ImportError::UnavailableSegmentScope {
                edge_id: "child-root".to_string(),
                graph_parent: "outer-group".to_string(),
            }
        );
    }

    #[test]
    fn scoped_segment_import_accepts_a_materialized_owner_scope() {
        let mut group = node("group");
        group.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut child = node("child");
        child.parent = Some("group".to_string());
        let mut input = graph(vec![group, child, node("outer")], vec![]);
        input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
        let mut segment_edge = edge("child-outer", "child", "outer");
        segment_edge.model_order = Some(7);
        segment_edge.labels = vec![ElkInputLabel::center("identity", 40.0, 12.0)];
        let segment = ElkInputEdgeSegment {
            edge: segment_edge,
            source: ElkInputEdgeSegmentEndpoint::Node {
                id: "child".to_string(),
            },
            target: ElkInputEdgeSegmentEndpoint::Node {
                id: "outer".to_string(),
            },
            segment: CompoundEdgeSegment::Output { depth: 2 },
            edge_order: 0,
        };

        let imported = import_graph_with_segments(&input, &[segment])
            .expect("the nested owner scope is materialized");

        assert_eq!(imported.cross_hierarchy_edges.len(), 1);
        let nested = imported
            .layerless_nodes
            .iter()
            .find(|node| node.id == "group")
            .and_then(|node| node.nested_graph.map(|id| &imported[id]))
            .expect("group scope should be materialized");
        assert_eq!(nested.cross_hierarchy_edges.len(), 1);
        assert_eq!(
            imported.cross_hierarchy_edges[0].original_model_order,
            Some(7)
        );
        assert_eq!(
            nested.cross_hierarchy_edges[0].original_model_order,
            Some(7)
        );
        assert_eq!(
            imported.cross_hierarchy_edges[0].segment,
            CompoundEdgeSegment::Output { depth: 2 }
        );
        assert_eq!(
            nested.cross_hierarchy_edges[0].segment,
            CompoundEdgeSegment::Output { depth: 3 }
        );
        let labels = imported
            .edges
            .iter()
            .chain(nested.edges.iter())
            .flat_map(|edge| edge.labels.iter())
            .collect::<Vec<_>>();
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].text, "identity");
        assert_eq!(
            labels[0].original_label_edge.as_deref(),
            Some("child-outer")
        );
    }

    #[test]
    fn scoped_input_segment_rebases_every_materialized_hierarchy_level() {
        let mut group = node("group");
        group.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut child = node("child");
        child.parent = Some("group".to_string());
        let mut input = graph(vec![group, child, node("outer")], vec![]);
        input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
        let segment = ElkInputEdgeSegment {
            edge: edge("outer-child", "outer", "child"),
            source: ElkInputEdgeSegmentEndpoint::Node {
                id: "outer".to_string(),
            },
            target: ElkInputEdgeSegmentEndpoint::Node {
                id: "child".to_string(),
            },
            segment: CompoundEdgeSegment::Input { depth: 4 },
            edge_order: 0,
        };

        let imported = import_graph_with_segments(&input, &[segment])
            .expect("the nested owner scope is materialized");
        let nested = imported
            .layerless_nodes
            .iter()
            .find(|node| node.id == "group")
            .and_then(|node| node.nested_graph.map(|id| &imported[id]))
            .expect("group scope should be materialized");

        assert_eq!(
            imported.cross_hierarchy_edges[0].segment,
            CompoundEdgeSegment::Input { depth: 4 }
        );
        assert_eq!(
            nested.cross_hierarchy_edges[0].segment,
            CompoundEdgeSegment::Input { depth: 5 }
        );
    }

    #[test]
    fn import_preserves_model_order_strategy_without_enabling_wrapping() {
        let mut input = graph(vec![node("A"), node("B")], vec![edge("A-B", "A", "B")]);
        input.options.consider_model_order_strategy = OrderingStrategy::NodesAndEdges;

        let lgraph = import_graph(&input).unwrap();

        assert_eq!(
            lgraph.options.consider_model_order_strategy,
            OrderingStrategy::NodesAndEdges
        );
        assert!(!lgraph.options.graph_has_hyperedges);
    }

    #[test]
    fn import_keeps_elk_zero_seed_and_the_pipeline_rejects_it() {
        let mut input = graph(vec![node("A")], vec![]);
        input.options.random_seed = 0;
        let mut imported = import_graph(&input).expect("zero is a valid upstream input value");

        assert!(matches!(
            crate::pipeline::execute_ported_processors(&mut imported),
            Err(crate::pipeline::PipelineError::RandomSeed(crate::random::RandomSeedError::ZeroSeed {
                graph_id
            })) if graph_id == "root"
        ));
        assert_eq!(imported.options.random_seed, 0);
    }

    // --- Ports on nodes without children. ---

    fn input_port(id: &str, side: PortSide) -> ElkInputPort {
        ElkInputPort {
            id: id.to_string(),
            x: 0.0,
            y: 0.0,
            width: 6.0,
            height: 4.0,
            side,
            index: None,
            border_offset: None,
            allow_non_flow_ports_to_switch_sides: false,
            net_flow: 0,
            child_edges: 0,
            connected_to_external_nodes: false,
        }
    }

    /// A node of 80 by 40 with the given ports and port constraints.
    fn ported(
        id: &str,
        constraints: Option<PortConstraints>,
        ports: Vec<ElkInputPort>,
    ) -> ElkInputNode {
        let mut node = node(id);
        node.port_constraints = constraints;
        node.ports = ports;
        node
    }

    fn flat_graph(
        direction: ElkDirection,
        nodes: Vec<ElkInputNode>,
        edges: Vec<ElkInputEdge>,
    ) -> ElkInputGraph {
        let mut input = graph(nodes, edges);
        input.options.direction = direction;
        input.options.hierarchy_handling = HierarchyHandling::SeparateChildren;
        input
    }

    fn layered_node<'a>(graph: &'a LGraph, id: &str) -> &'a LNode {
        graph
            .layerless_nodes
            .iter()
            .find(|node| node.id == id)
            .unwrap()
    }

    // Mutation: skip the `non_free_ports` property in `transform_node`, or compute the anchor
    // as the centre for a fixed-side port.
    #[test]
    fn a_fixed_side_port_keeps_its_size_position_side_and_side_anchor() {
        let mut east = input_port("p", PortSide::East);
        east.x = 80.0;
        east.y = 10.0;
        let west = input_port("q", PortSide::West);
        let input = flat_graph(
            ElkDirection::Right,
            vec![ported(
                "A",
                Some(PortConstraints::FixedSide),
                vec![east, west],
            )],
            vec![],
        );
        let lgraph = import_graph(&input).unwrap();
        let node = layered_node(&lgraph, "A");
        assert_eq!(node.port_constraints, PortConstraints::FixedSide);
        assert!(lgraph.graph_properties.non_free_ports);
        assert!(!lgraph.graph_properties.north_south_ports);
        let ids: Vec<_> = node.ports.iter().map(|port| port.id.as_str()).collect();
        assert_eq!(ids, ["p", "q"]);
        let (p, q) = (&node.ports[PortId::new(0)], &node.ports[PortId::new(1)]);
        assert_eq!(p.side, PortSide::East);
        assert_eq!((p.size.width, p.size.height), (6.0, 4.0));
        assert_eq!((p.position.x, p.position.y), (80.0, 10.0));
        assert_eq!((p.anchor.x, p.anchor.y), (6.0, 2.0));
        assert_eq!((q.anchor.x, q.anchor.y), (0.0, 2.0));
        assert_eq!((p.input_index, q.input_index), (Some(0), Some(1)));
    }

    // Mutation: give a port that is not side fixed the anchor of its side.
    #[test]
    fn a_free_port_keeps_its_side_but_has_the_centre_as_anchor() {
        let input = flat_graph(
            ElkDirection::Right,
            vec![ported("A", None, vec![input_port("p", PortSide::East)])],
            vec![],
        );
        let lgraph = import_graph(&input).unwrap();
        let port = &layered_node(&lgraph, "A").ports[PortId::new(0)];
        assert_eq!(port.side, PortSide::East);
        assert_eq!((port.anchor.x, port.anchor.y), (3.0, 2.0));
        assert!(!lgraph.graph_properties.non_free_ports);
    }

    // Mutation: keep `UNDEFINED` on the layered node, or let it set `non_free_ports`.
    #[test]
    fn undefined_port_constraints_become_free() {
        let input = flat_graph(
            ElkDirection::Right,
            vec![ported(
                "A",
                Some(PortConstraints::Undefined),
                vec![input_port("p", PortSide::West)],
            )],
            vec![],
        );
        let lgraph = import_graph(&input).unwrap();
        assert_eq!(
            layered_node(&lgraph, "A").port_constraints,
            PortConstraints::Free
        );
        assert!(!lgraph.graph_properties.non_free_ports);
    }

    // Mutation: set `non_free_ports` only for a node that has ports.
    #[test]
    fn a_node_without_ports_still_marks_the_graph_as_not_free() {
        let input = flat_graph(
            ElkDirection::Right,
            vec![ported("A", Some(PortConstraints::FixedOrder), vec![])],
            vec![],
        );
        assert!(
            import_graph(&input)
                .unwrap()
                .graph_properties
                .non_free_ports
        );
    }

    // Mutation: drop the call to `calc_port_side`, or the border offset next to it.
    #[test]
    fn a_fixed_side_port_without_a_side_takes_it_from_its_position() {
        // Node of 80 by 40, port of 6 by 4. (x, y, expected side, expected border offset)
        let cases = [
            (-8.0, 10.0, PortSide::West, Some(2.0)),
            (84.0, 10.0, PortSide::East, Some(4.0)),
            // At the corner the offset stays unset, so the port sits on the border.
            (0.0, 0.0, PortSide::West, None),
        ];
        for (x, y, side, offset) in cases {
            let mut port = input_port("p", PortSide::Undefined);
            port.x = x;
            port.y = y;
            let input = flat_graph(
                ElkDirection::Right,
                vec![ported("A", Some(PortConstraints::FixedSide), vec![port])],
                vec![],
            );
            let lgraph = import_graph(&input).unwrap();
            let port = &layered_node(&lgraph, "A").ports[PortId::new(0)];
            assert_eq!((port.side, port.border_offset), (side, offset), "x = {x}");
        }
    }

    // Mutation: overwrite a border offset the input sets with the computed one.
    #[test]
    fn a_border_offset_from_the_input_wins() {
        let mut port = input_port("p", PortSide::Undefined);
        port.x = -8.0;
        port.y = 10.0;
        port.border_offset = Some(7.0);
        let input = flat_graph(
            ElkDirection::Right,
            vec![ported("A", Some(PortConstraints::FixedSide), vec![port])],
            vec![],
        );
        let lgraph = import_graph(&input).unwrap();
        assert_eq!(
            layered_node(&lgraph, "A").ports[PortId::new(0)].border_offset,
            Some(7.0)
        );
    }

    // Mutation: remember the ratio for FIXED_POS too, or divide by the width on the east side.
    #[test]
    fn only_fixed_ratio_remembers_the_ratio_of_a_port() {
        for (constraints, ratio) in [
            (PortConstraints::FixedRatio, 0.25),
            (PortConstraints::FixedPos, 0.0),
            (PortConstraints::FixedOrder, 0.0),
        ] {
            let mut port = input_port("p", PortSide::East);
            port.x = 80.0;
            port.y = 10.0;
            let input = flat_graph(
                ElkDirection::Right,
                vec![ported("A", Some(constraints), vec![port])],
                vec![],
            );
            let lgraph = import_graph(&input).unwrap();
            assert_eq!(
                layered_node(&lgraph, "A").ports[PortId::new(0)].ratio_or_position,
                ratio
            );
        }
    }

    // Mutation: ignore the `elk.port.index` of the input port.
    #[test]
    fn the_port_index_reaches_the_layered_port() {
        let mut indexed = input_port("p", PortSide::East);
        indexed.index = Some(3);
        let input = flat_graph(
            ElkDirection::Right,
            vec![ported(
                "A",
                Some(PortConstraints::FixedOrder),
                vec![indexed, input_port("q", PortSide::East)],
            )],
            vec![],
        );
        let lgraph = import_graph(&input).unwrap();
        let ports = &layered_node(&lgraph, "A").ports;
        assert_eq!(
            (
                ports[PortId::new(0)].port_index,
                ports[PortId::new(1)].port_index
            ),
            (Some(3), None)
        );
    }

    // Mutation: attach the edge to a new port instead of the named one.
    #[test]
    fn an_edge_attaches_to_its_named_ports() {
        let mut named = edge("e", "A", "B");
        named.source_port = Some("p".to_string());
        named.target_port = Some("q".to_string());
        let input = flat_graph(
            ElkDirection::Right,
            vec![
                ported(
                    "A",
                    Some(PortConstraints::FixedSide),
                    vec![
                        input_port("x", PortSide::East),
                        input_port("p", PortSide::East),
                    ],
                ),
                ported(
                    "B",
                    Some(PortConstraints::FixedSide),
                    vec![input_port("q", PortSide::West)],
                ),
            ],
            vec![named],
        );
        let lgraph = import_graph(&input).unwrap();
        // No port was added, and the second port of A holds the edge.
        assert_eq!(layered_node(&lgraph, "A").ports.len(), 2);
        assert_eq!(layered_node(&lgraph, "B").ports.len(), 1);
        assert!(
            layered_node(&lgraph, "A").ports[PortId::new(0)]
                .outgoing_edges
                .is_empty()
        );
        assert_eq!(
            layered_node(&lgraph, "A").ports[PortId::new(1)].outgoing_edges,
            vec![0]
        );
        assert_eq!(
            layered_node(&lgraph, "B").ports[PortId::new(0)].incoming_edges,
            vec![0]
        );
    }

    // Mutation: count the incoming and outgoing edges of a port together, as one number.
    #[test]
    fn a_port_with_two_edges_in_one_direction_makes_a_hyperedge_but_one_each_does_not() {
        let from_port = |id: &str, target: &str| {
            let mut e = edge(id, "A", target);
            e.source_port = Some("p".to_string());
            e
        };
        let into_port = |id: &str, source: &str| {
            let mut e = edge(id, source, "A");
            e.target_port = Some("p".to_string());
            e
        };
        let build = |edges: Vec<ElkInputEdge>| {
            let input = flat_graph(
                ElkDirection::Right,
                vec![
                    ported(
                        "A",
                        Some(PortConstraints::FixedSide),
                        vec![input_port("p", PortSide::East)],
                    ),
                    node("B"),
                ],
                edges,
            );
            import_graph(&input).unwrap().graph_properties.hyperedges
        };
        assert!(!build(vec![from_port("e1", "B")]));
        assert!(!build(vec![from_port("e1", "B"), into_port("e2", "B")]));
        assert!(build(vec![from_port("e1", "B"), from_port("e2", "B")]));
        assert!(build(vec![into_port("e1", "B"), into_port("e2", "B")]));
    }

    // Mutation: look up the crossing edges of a port by its id alone (`crossing_at` and the
    // writes in `transform_cross_hierarchy_edge` ignore `named`). The implicit port `G:2` then
    // reads the edge into the named port `G:2` and the graph becomes one with hyperedges.
    #[test]
    fn a_made_up_port_does_not_share_crossing_edges_with_a_named_port_of_the_same_id() {
        let build = |flat_target_port: Option<&str>| {
            let mut child = node("c");
            child.parent = Some("G".to_string());
            let mut into_port = edge("e1", "c", "G");
            into_port.target_port = Some("G:2".to_string());
            // The root owns the edge, so it is imported before the flat edge, as the bug needs.
            into_port.container = Some("root".to_string());
            let mut flat = edge("e2", "s", "G");
            flat.target_port = flat_target_port.map(str::to_string);
            let mut input = flat_graph(
                ElkDirection::Right,
                vec![
                    node("s"),
                    ported(
                        "G",
                        None,
                        vec![
                            input_port("G:1", PortSide::West),
                            input_port("G:2", PortSide::West),
                        ],
                    ),
                    child,
                ],
                vec![into_port, flat],
            );
            input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
            import_graph(&input).unwrap().graph_properties.hyperedges
        };
        // G has two ports, so the edge from `s` gets a made-up port that is also called `G:2`.
        assert!(!build(None));
        // Control: two edges into the named port do make a hyperedge.
        assert!(build(Some("G:2")));
    }

    // Mutation: reject a port that has the id of a node, or let two ports share an id.
    #[test]
    fn a_port_id_may_repeat_a_node_id_but_not_a_port_id() {
        let two_with = |first: &str, second: &str| {
            flat_graph(
                ElkDirection::Right,
                vec![
                    ported("A", None, vec![input_port(first, PortSide::East)]),
                    ported("B", None, vec![input_port(second, PortSide::West)]),
                ],
                vec![],
            )
        };
        assert!(matches!(
            import_graph(&two_with("p", "p")),
            Err(ImportError::DuplicatePort { id }) if id == "p"
        ));
        // elkjs keeps node and port ids apart.
        assert!(import_graph(&two_with("B", "A")).is_ok());
        assert!(import_graph(&two_with("p", "q")).is_ok());
    }

    // Mutation: accept a port of another node.
    #[test]
    fn an_edge_may_only_name_a_port_of_its_own_node() {
        let build = |source_port: &str| {
            let mut e = edge("e", "A", "B");
            e.source_port = Some(source_port.to_string());
            flat_graph(
                ElkDirection::Right,
                vec![
                    ported("A", None, vec![input_port("p", PortSide::East)]),
                    ported("B", None, vec![input_port("q", PortSide::West)]),
                ],
                vec![e],
            )
        };
        assert!(import_graph(&build("p")).is_ok());
        assert!(matches!(
            import_graph(&build("q")),
            Err(ImportError::MissingPort { port_id, node_id, .. }) if port_id == "q" && node_id == "A"
        ));
        assert!(matches!(
            import_graph(&build("nowhere")),
            Err(ImportError::MissingPort { .. })
        ));

        // An edge the flat import leaves out is checked too: both its ends sit inside `G`.
        let mut inside = edge("e", "X", "X");
        inside.source_port = Some("q".to_string());
        let mut x = ported("X", None, vec![input_port("p", PortSide::East)]);
        x.parent = Some("G".to_string());
        let input = flat_graph(
            ElkDirection::Right,
            vec![
                node("G"),
                x,
                ported("B", None, vec![input_port("q", PortSide::West)]),
            ],
            vec![inside],
        );
        assert!(matches!(
            import_graph(&input),
            Err(ImportError::MissingPort { port_id, .. }) if port_id == "q"
        ));
    }

    // Mutation: drop the direction from the test, so that a port across the flow is one that is
    // north or south in every direction.
    #[test]
    fn a_port_across_the_flow_of_a_node_without_children_is_imported_and_marks_the_graph() {
        // (direction, side, across the flow)
        let cases = [
            (ElkDirection::Right, PortSide::North, true),
            (ElkDirection::Left, PortSide::South, true),
            (ElkDirection::Right, PortSide::East, false),
            (ElkDirection::Down, PortSide::East, true),
            (ElkDirection::Up, PortSide::West, true),
            (ElkDirection::Down, PortSide::North, false),
            (ElkDirection::Up, PortSide::South, false),
        ];
        for (direction, side, across_flow) in cases {
            let input = flat_graph(
                direction,
                vec![ported(
                    "A",
                    Some(PortConstraints::FixedSide),
                    vec![input_port("p", side)],
                )],
                vec![],
            );
            let lgraph = import_graph(&input).unwrap();
            assert_eq!(
                lgraph.graph_properties.north_south_ports, across_flow,
                "{direction:?} {side:?}"
            );
        }
    }

    // Mutation: refuse every node with fixed ports across the flow, not only a node with
    // children, accept the port of a node with children, or ignore the direction.
    #[test]
    fn a_port_across_the_flow_of_a_node_with_children_is_refused() {
        let hierarchical = |direction: ElkDirection, side: PortSide| {
            let mut child = node("X");
            child.parent = Some("G".to_string());
            let mut input = flat_graph(
                direction,
                vec![
                    ported(
                        "G",
                        Some(PortConstraints::FixedSide),
                        vec![input_port("p", side)],
                    ),
                    child,
                    // A node without children has a port across the flow in every case.
                    ported(
                        "L",
                        Some(PortConstraints::FixedSide),
                        vec![
                            input_port("q", PortSide::North),
                            input_port("r", PortSide::East),
                        ],
                    ),
                ],
                vec![],
            );
            input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
            input
        };

        // (direction, side of the port of `G`, refused)
        let cases = [
            (ElkDirection::Right, PortSide::North, true),
            (ElkDirection::Down, PortSide::East, true),
            (ElkDirection::Right, PortSide::East, false),
            (ElkDirection::Down, PortSide::North, false),
        ];
        for (direction, side, refused) in cases {
            let result = import_graph(&hierarchical(direction, side));
            if refused {
                assert!(
                    matches!(
                        result,
                        Err(ImportError::NorthSouthPort { ref node_id, ref port_id })
                            if node_id == "G" && port_id == "p"
                    ),
                    "{direction:?} {side:?}"
                );
            } else {
                assert!(result.is_ok(), "{direction:?} {side:?}");
            }
        }
    }

    // Mutation: test `nested_graph.is_some()` instead of children in `check_north_south_ports`,
    // so the dummy-only nested graph of an inside self loop makes the node count as one with
    // children.
    #[test]
    fn a_node_with_only_an_inside_self_loop_has_no_children_for_its_ports() {
        let mut input = graph(
            vec![ported(
                "A",
                Some(PortConstraints::FixedSide),
                vec![input_port("p", PortSide::North)],
            )],
            vec![ElkInputEdge {
                source_port: None,
                target_port: None,
                inside_self_loops_yo: true,
                ..edge("A-A", "A", "A")
            }],
        );
        input.options.direction = ElkDirection::Right;
        input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
        input.options.inside_self_loops_activate = true;
        let lgraph = import_graph(&input).unwrap();
        // The node has a nested graph, so only the test for children lets the port pass.
        assert!(layered_node(&lgraph, "A").nested_graph.is_some());
    }

    // Mutation: refuse a port across the flow although every node has free ports.
    #[test]
    fn a_port_across_the_flow_is_fine_while_all_ports_are_free() {
        let input = flat_graph(
            ElkDirection::Right,
            vec![ported("A", None, vec![input_port("p", PortSide::North)])],
            vec![],
        );
        let lgraph = import_graph(&input).unwrap();
        // The graph still has the property. `PortSideProcessor` moves the port later.
        assert!(lgraph.graph_properties.north_south_ports);
        // A fixed node anywhere else in the graph makes the graph need the north and south
        // port steps, which a node without children does not stop.
        let mixed = flat_graph(
            ElkDirection::Right,
            vec![
                ported("A", None, vec![input_port("p", PortSide::North)]),
                ported("B", Some(PortConstraints::FixedOrder), vec![]),
            ],
            vec![],
        );
        assert!(import_graph(&mixed).is_ok());
    }

    // Mutation: hand out the model order of ports even when the graph does not ask for it.
    #[test]
    fn ports_get_a_model_order_when_the_graph_asks_for_one() {
        let ports = || {
            vec![
                input_port("p", PortSide::East),
                input_port("q", PortSide::East),
            ]
        };
        let mut asking = flat_graph(
            ElkDirection::Right,
            vec![ported("A", None, ports())],
            vec![],
        );
        asking.options.consider_model_order_strategy = OrderingStrategy::NodesAndEdges;
        let lgraph = import_graph(&asking).unwrap();
        let orders: Vec<_> = layered_node(&lgraph, "A")
            .ports
            .iter()
            .map(|p| p.model_order)
            .collect();
        assert_eq!(orders, [Some(0), Some(1)]);

        let mut silent = flat_graph(
            ElkDirection::Right,
            vec![ported("A", None, ports())],
            vec![],
        );
        silent.options.consider_model_order_strategy = OrderingStrategy::None;
        let lgraph = import_graph(&silent).unwrap();
        let orders: Vec<_> = layered_node(&lgraph, "A")
            .ports
            .iter()
            .map(|p| p.model_order)
            .collect();
        assert_eq!(orders, [None, None]);
    }
}
