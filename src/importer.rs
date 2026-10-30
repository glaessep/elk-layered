//! ELK graph importer scaffold.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/graph/transform/ElkGraphImporter.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/LayeredLayoutProvider.java
//! - https://github.com/mermaid-js/mermaid/blob/7c0cafcf42e76bfaf79d0cbbd12edb986612f014/packages/mermaid-layout-elk/src/render.ts

use std::collections::{HashMap, HashSet, VecDeque};

use crate::common::cells::{self, CellLabel, CellNode, CellPort, CellPortLabel, CellPorts};
use crate::compound::{
    PendingCompoundSegment, PendingSegmentEndpoint, ScopedHierarchySegment,
    compound_label_segment_index, introduce_source_ported_scoped_edge_segments,
    source_ported_cross_hierarchy_segments,
};
use crate::graph::{
    CompoundEdgeSegment, DummyLabelSpacings, EdgeId, EdgeLabelPlacement, ExternalPortData,
    GraphArena, GraphId, HierarchyEdge, HierarchyStep, LGraph, LLabel, LNode, LPoint, LPort, LSize,
    LayeredEdge, NodeId, PlacementOptions, PortId, PortRef, PortSide, PortType, SelfLoopOptions,
    apply_port_index_to_dummy, create_external_port_dummy, is_across_flow,
    outside_dummy_label_size, port_box, port_side_from_direction,
};
use crate::options::{
    CycleBreakingStrategy, ElkDirection, ElkPadding, HierarchyHandling, LayerConstraint,
    LayeredOptions, LayeringStrategy, NodeLabelPlacement, OrderingStrategy, PortConstraints,
    PortLabelPlacement,
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
    /// `InternalProperties.COMPOUND_NODE`: the node has children, or activates inside self
    /// loops. It holds also when a run lays the node out without its children.
    pub compound_node: bool,
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
    /// The node's own `portLabels.placement`, with the deprecated options folded in.
    pub port_label_placement: crate::options::PortLabelPlacement,
    /// The node's own `portLabels.treatAsGroup`.
    pub port_labels_treat_as_group: bool,
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
    /// Whether an edge at the port leads to or comes from a descendant of the port's node.
    /// Ports the `connectionsToDescendants` of `ElkGraphImporter.transformPort`.
    pub inside_connections: bool,
    /// Whether any edge of the graph starts or ends at the port. The cells read it from
    /// `ElkGraphAdapters.ElkPortAdapter` to place port labels next to the port.
    pub has_edges: bool,
    /// The port's labels, in input order. ELK drops a label with empty text.
    pub labels: Vec<ElkInputLabel>,
}

impl ElkInputPort {
    /// Where the port's top left corner lies, relative to its node, and the port's width and
    /// height.
    pub(crate) fn bounds(&self) -> (LPoint, LSize) {
        port_box(self.x, self.y, self.width, self.height)
    }
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
    /// Whether the edge is an inside self loop. The caller has already combined the edge's
    /// `insideSelfLoops.yo` with the `insideSelfLoops.activate` of its node, so the importer
    /// reads this alone.
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
    /// Left side, relative to the label's owner. A port label whose node's port label
    /// placement is fixed keeps it.
    pub x: f64,
    /// Top side, relative to the label's owner.
    pub y: f64,
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
            x: 0.0,
            y: 0.0,
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
    /// An edge names a container node that holds no graph, so the edge has no graph to belong to.
    #[error("ELK edge `{edge_id}` is contained by node `{container}`, which holds no graph")]
    ContainerWithoutGraph { edge_id: String, container: String },
    #[error("ELK edge `{edge_id}` names port `{port_id}`, which is no port of node `{node_id}`")]
    MissingPort {
        edge_id: String,
        node_id: String,
        port_id: String,
    },
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
    import_scope_with(
        input,
        segments,
        ElkPadding::uniform(0.0),
        ScopePorts::none(),
        false,
    )
    .map(|imported| imported.graphs)
}

/// What importing a scope makes.
pub(crate) struct ImportedScope {
    /// Every graph of the scope.
    pub(crate) graphs: GraphArena,
    /// The graph each input edge belongs to, by the edge's id: the root, or the graph inside the
    /// node that contains the edge. A later step never has to look a graph up by its name.
    /// Empty when the caller did not ask for it.
    pub(crate) edge_containers: HashMap<String, GraphId>,
}

/// The ports of the node a run lays out, which ELK sees as the external ports of the graph.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ScopePorts<'a> {
    /// In input order. Each port has its side defined: `ElkGraphImporter.ensureDefinedPortSide`.
    pub(crate) ports: &'a [ElkInputPort],
    /// The size of the node, which the dummies of ports with fixed ratios read.
    pub(crate) node_size: LSize,
    /// The label spacings the parent of the node sets: `transformExternalPort` hands them to each
    /// dummy.
    pub(crate) label_spacings: DummyLabelSpacings,
}

impl ScopePorts<'_> {
    /// A graph whose node has no ports.
    #[cfg(test)]
    pub(crate) fn none() -> ScopePorts<'static> {
        ScopePorts {
            ports: &[],
            node_size: LSize {
                width: 0.0,
                height: 0.0,
            },
            label_spacings: DummyLabelSpacings::default(),
        }
    }
}

/// Imports a graph whose root has inside node labels. `root_label_padding` is the padding
/// `NodeLabelAndSizeCalculator.computeInsideNodeLabelPadding` gives the root node.
pub(crate) fn import_scope(
    input: &ElkInputGraph,
    segments: &[ElkInputEdgeSegment],
    root_label_padding: ElkPadding,
    scope: ScopePorts<'_>,
) -> ImportResult<ImportedScope> {
    import_scope_with(input, segments, root_label_padding, scope, true)
}

/// [`import_scope`], with a choice about the edge containers. Only `layout` reads them, so the
/// other callers pass `false` and skip building the map.
fn import_scope_with(
    input: &ElkInputGraph,
    segments: &[ElkInputEdgeSegment],
    root_label_padding: ElkPadding,
    scope: ScopePorts<'_>,
    with_edge_containers: bool,
) -> ImportResult<ImportedScope> {
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

    // The graph inside a node, if it has one.
    let nested_of = |node: &str| index.node_index(node).and_then(|node| nested_graphs[node]);
    // A node without a graph reads as the root, as `graph_slot_for_parent` does.
    let graph_of = |node: &str| nested_of(node).unwrap_or(GraphId::ROOT);
    let prepared_segments =
        materialize_scoped_edge_segments(&index, segments, input.options.merge_edges, &graph_of);
    introduce_source_ported_scoped_edge_segments(
        &mut graphs,
        GraphId::ROOT,
        prepared_segments.segments,
    );

    graphs.sync_graph_properties_to_options();
    // The same container the edges were grouped by above, now as the graph that holds it.
    let mut edge_containers = HashMap::new();
    if with_edge_containers {
        for edge in &input.edges {
            let graph = match input_edge_containing_parent(&index, edge) {
                // The scope itself holds the edge.
                None => GraphId::ROOT,
                // A named container without a graph is not the root. Reading it as the root would
                // give the edge the wrong coordinates.
                Some(container) => {
                    nested_of(container).ok_or_else(|| ImportError::ContainerWithoutGraph {
                        edge_id: edge.id.clone(),
                        container: container.to_string(),
                    })?
                }
            };
            edge_containers.insert(edge.id.clone(), graph);
        }
    }
    Ok(ImportedScope {
        graphs,
        edge_containers,
    })
}

/// Makes the external port dummy of each port of the graph's node, if the graph has external
/// ports. Gives the dummy of each port by the port's id.
///
/// Ports `ElkGraphImporter.checkExternalPorts` and `transformExternalPort`. A port is external when
/// an edge connects it to a child, or when the node places its port labels `INSIDE` and the port
/// has a label. An inside self loop at a port is refused.
fn import_external_ports(
    root: &mut LGraph,
    scope: &ScopePorts<'_>,
    scope_id: &str,
) -> HashMap<String, NodeId> {
    let mut dummies = HashMap::new();
    let placement = root.options.port_label_placement;
    let inside_port_labels = placement.contains(PortLabelPlacement::INSIDE);
    if scope.ports.iter().any(|port| port.child_edges > 0)
        // `portLabelPlacement.contains(INSIDE) && elkport.getLabels().size() > 0`: a label without
        // text counts, because the check looks at the list.
        || (inside_port_labels && scope.ports.iter().any(|port| !port.labels.is_empty()))
    {
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
        let (port_position, port_size) = port.bounds();
        let border_offset = port.border_offset.unwrap_or_else(|| {
            if port.x == 0.0 && port.y == 0.0 {
                0.0
            } else {
                calc_port_offset(port_position, port_size, scope.node_size, port.side)
            }
        });
        let direction = root.options.direction;
        let node = root.push_node_with(|node| {
            let mut dummy = create_external_port_dummy(
                node,
                format!("{scope_id}:external:{}", port.id),
                port.id.clone(),
                PortType::Output,
                constraints,
                port.side,
                port.net_flow as isize,
                LPoint {
                    x: port_position.x + port_size.width / 2.0,
                    y: port_position.y + port_size.height / 2.0,
                },
                port_size,
                scope.node_size,
                border_offset,
                direction,
                ExternalPortData {
                    scope_port: Some(position),
                    label_spacings: scope.label_spacings,
                    ..ExternalPortData::default()
                },
            );
            apply_port_index_to_dummy(&mut dummy, constraints, port.index.map(|i| i as isize));
            let dummy_port = &mut dummy.ports[PortId::new(0)];
            dummy_port.named = true;
            dummy_port.connected_to_external_nodes = port.connected_to_external_nodes;
            // If the node places its port labels inside, the graph needs room for them. If it
            // places them outside, the graph still needs room, so the ports do not clash outside;
            // but inside it needs none on the axis along the port's side. So the label keeps only
            // the part that lies inside the node: `insidePart`, which only fixed labels have.
            for mut llabel in port_labels_to_lgraph(port) {
                if !inside_port_labels && port.side != PortSide::Undefined {
                    llabel.size = outside_dummy_label_size(
                        llabel.position,
                        llabel.size,
                        port_size,
                        port.side,
                        placement.is_fixed(),
                        matches!(port.side, PortSide::East | PortSide::West),
                    );
                }
                dummy_port.labels.push(llabel);
            }
            dummy
        });
        dummies.insert(port.id.clone(), node);
    }
    dummies
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
                lands_in: None,
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
            let parent_graph = &mut arena.graphs[parent_slot];
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
            let edge_model_order = needs_model_order_based_on_parent(&arena.graphs.root().options)
                .then(|| edge.model_order.unwrap_or(edge_order));
            let source_parent = index.node_parent(edge.source.as_str());
            let target_parent = index.node_parent(edge.target.as_str());
            if is_inside_self_loop(edge) {
                let graph_slot = arena.graph_slot_for_parent(index, Some(edge.source.as_str()));
                let graph = &mut arena.graphs[graph_slot];
                transform_inside_self_loop(edge, graph, edge_model_order);
            } else if source_parent == target_parent {
                let graph = &mut arena.graphs[owner_slot];
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

/// Ports `ElkGraphImporter.importHierarchicalGraph` and `transformEdge` for an inside self loop.
///
/// `transformEdge` makes a new port for each end of an edge that has none, so each loop gets a
/// source dummy and a target dummy of its own. No port holds more than one edge, and the loop is
/// no hyperedge.
fn transform_inside_self_loop(
    edge: &ElkInputEdge,
    graph: &mut LGraph,
    model_order: Option<usize>,
) -> EdgeId {
    let source = make_inside_self_loop_dummy(graph, edge.source.as_str(), PortType::Output);
    let target = make_inside_self_loop_dummy(graph, edge.target.as_str(), PortType::Input);

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
        .add_edge({
            let mut built_edge = LayeredEdge::new(edge.id.clone(), source, target);
            built_edge.source_node_id = edge.source.clone();
            built_edge.target_node_id = edge.target.clone();
            built_edge.labels = labels;
            built_edge.minlen = edge.minlen.max(1);
            built_edge.model_order = model_order;
            built_edge.priority_direction = edge.priority_direction;
            built_edge.priority_shortness = edge.priority_shortness;
            built_edge.priority_straightness = edge.priority_straightness;
            built_edge.thickness = ELK_DEFAULT_EDGE_THICKNESS;
            built_edge.inside_self_loop = true;
            built_edge
        })
        .expect("inside self-loop dummies were created before adding edge");

    graph.graph_properties.self_loops = true;

    edge_index
}

/// Makes the external port dummy for one end of an inside self loop.
///
/// The dummy has no key of its own. The compound preprocessor then makes a new port on the node
/// for it, as `LGraphUtil.createPort` does in ELK, and never matches a port the caller named.
fn make_inside_self_loop_dummy(
    graph: &mut LGraph,
    parent_node_id: &str,
    port_type: PortType,
) -> PortRef {
    graph.graph_properties.external_ports = true;
    graph.graph_properties.non_free_ports = true;

    let dummy_id = format!("external:{parent_node_id}");
    let direction = graph.options.direction;
    let node = graph.push_node_with(|node| {
        create_external_port_dummy(
            node,
            dummy_id,
            String::new(),
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
            direction,
            ExternalPortData {
                parent_port_type: Some(port_type),
                ..ExternalPortData::default()
            },
        )
    });
    PortRef {
        node,
        port: PortId::new(0),
    }
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
        ports: cell_ports(node),
    }
}

/// The ports of a node of the input graph, as `ElkGraphAdapters.ElkPortAdapter` shows them to
/// the cells.
///
/// `ratio_or_position` stays 0, as on an `ElkPort`: only `LGraphUtil.initializePort` sets that
/// property, on the layered port.
fn cell_ports(node: &ElkInputNode) -> CellPorts {
    let parent = &node.parent_settings.options;
    CellPorts {
        ports: node
            .ports
            .iter()
            // `ensureDefinedPortSide` gives every port of a graph's node a side before ELK sizes
            // the node. A port the caller left without one has no part in the size here.
            .filter(|port| port.side != PortSide::Undefined)
            .map(|port| CellPort {
                side: port.side,
                size: LSize {
                    width: port.width,
                    height: port.height,
                },
                position: LPoint {
                    x: port.x,
                    y: port.y,
                },
                border_offset: port.border_offset,
                ratio_or_position: 0.0,
                // The adapter shows every label, those without text too.
                labels: port
                    .labels
                    .iter()
                    .map(|label| CellPortLabel {
                        size: LSize {
                            width: label.width,
                            height: label.height,
                        },
                        position: LPoint {
                            x: label.x,
                            y: label.y,
                        },
                    })
                    .collect(),
                has_edges: port.has_edges,
                has_compound_connections: port.inside_connections,
            })
            .collect(),
        constraints: match node.port_constraints {
            None | Some(PortConstraints::Undefined) => PortConstraints::Free,
            Some(other) => other,
        },
        label_placement: node.port_label_placement,
        treat_as_group: node.port_labels_treat_as_group,
        compound: true,
        port_port_spacing: parent.spacing.port_port,
        label_port_horizontal: parent.spacing.label_port_horizontal,
        label_port_vertical: parent.spacing.label_port_vertical,
        surrounding: parent.spacing.ports_surrounding.into(),
        alignment: node.port_alignment.unwrap_or(parent.port_alignment_default),
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
    let minimum = cells::process(node, horizontal_layout_mode, false, true).size;
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
    if let Some(layer_constraint) = node.layer_constraint {
        lnode.set_layer_constraint(layer_constraint);
    }
    // `ElkGraphImporter.transformNode` skips labels without text.
    for label in node.labels.iter().filter(|label| !label.text.is_empty()) {
        lnode.labels.push(label_to_lgraph(label));
    }
    lnode.node_label_placement = node.node_label_placement;
    lnode.node_size = node.node_size;
    lnode.port_label_placement = node.port_label_placement;
    lnode.port_labels_treat_as_group = node.port_labels_treat_as_group;
    lnode.compound_node = node.compound_node;
    let node_size = lnode.size;
    let node_index = graph.push_node_with(|_| lnode);
    if let Some(alignment) = node.port_alignment {
        *graph.tables.port_alignment.row_mut(node_index) = alignment;
    }
    *graph.tables.placement_options.row_mut(node_index) = PlacementOptions {
        flexibility: node.node_flexibility,
        ports_surrounding: node.ports_surrounding,
    };
    *graph.tables.self_loop_options.row_mut(node_index) = SelfLoopOptions {
        distribution: node.self_loop_distribution,
        ordering: node.self_loop_ordering,
    };

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
    (lport.position, lport.size) = port.bounds();
    // `lport.copyProperties(elkport)` copies the three options. `containsKey` asks whether one is
    // set.
    lport.port_index = port.index.map(|index| index as isize);
    lport.border_offset = port.border_offset;
    lport.allow_non_flow_ports_to_switch_sides = port.allow_non_flow_ports_to_switch_sides;
    // An inside self loop at a port is refused, so only an edge to or from a descendant counts.
    lport.inside_connections = port.inside_connections;
    initialize_port(&mut lport, constraints, node_size, graph.options.direction);
    lport.labels.extend(port_labels_to_lgraph(port));

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
        side = crate::compound::calc_port_side(port.position, port.size, node_size, direction);
        port.side = side;
        if port.border_offset.is_none()
            && side != PortSide::Undefined
            && (port.position.x != 0.0 || port.position.y != 0.0)
        {
            port.border_offset = Some(calc_port_offset(port.position, port.size, node_size, side));
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
fn calc_port_offset(position: LPoint, size: LSize, node_size: LSize, side: PortSide) -> f64 {
    match side {
        PortSide::North => -(position.y + size.height),
        PortSide::East => position.x - node_size.width,
        PortSide::South => position.y - node_size.height,
        PortSide::West => -(position.x + size.width),
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
) -> ImportResult<EdgeId> {
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
        .add_edge({
            let mut built_edge = LayeredEdge::new(edge.id.clone(), source, target);
            built_edge.source_node_id = edge.source.clone();
            built_edge.target_node_id = edge.target.clone();
            built_edge.labels = labels;
            built_edge.minlen = edge.minlen.max(1);
            built_edge.model_order = model_order;
            built_edge.priority_direction = edge.priority_direction;
            built_edge.priority_shortness = edge.priority_shortness;
            built_edge.priority_straightness = edge.priority_straightness;
            built_edge.thickness = ELK_DEFAULT_EDGE_THICKNESS;
            built_edge
        })
        .expect("ports were created before adding edge");

    // `transformEdge`: a port with more than one incoming or more than one outgoing edge makes
    // the graph one with hyperedges. The edges that cross a level count too.
    let parallel_at = |graph: &LGraph, end: PortRef| {
        let node = &graph.layerless_nodes[end.node];
        let port = &node.ports[end.port];
        let (incoming, outgoing) = port_index.crossing_at(&node.id, &port.id, port.named);
        port.incoming_edges().len() + incoming > 1 || port.outgoing_edges().len() + outgoing > 1
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
        .get(port.node)
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
    let merge_edges = arena.graphs.root().options.merge_edges;
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
        arena.graphs[owner_slot].graph_properties.hyperedges = true;
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
    arena.graphs.root_mut().hierarchy_edges.push(HierarchyEdge {
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
        let dummy = &arena.graphs.root().layerless_nodes[end.node];
        let port = &dummy.ports[end.port];
        return Ok(HierarchyEdgeEnd {
            node_id: dummy.id.clone(),
            port_key: port.id.clone(),
            named: port.named,
            attached: (port.incoming_edges().len(), port.outgoing_edges().len()),
        });
    }
    let slot = arena.graph_slot_for_parent(index, graph_parent);
    if let Some(port_id) = port_id {
        let (_, attached) = named_port(edge, node_id, port_id, index, &arena.graphs[slot])?;
        return Ok(HierarchyEdgeEnd {
            node_id: node_id.to_string(),
            port_key: port_id.to_string(),
            named: true,
            attached: (
                attached.incoming_edges().len(),
                attached.outgoing_edges().len(),
            ),
        });
    }
    let port_key = hierarchy_port_key(node_id, edge_order, role, merge_edges, port_type);
    let end = ensure_hierarchy_endpoint_port(
        &mut arena.graphs[slot],
        graph_parent,
        index.child_position(node_id),
        node_id,
        port_key.as_str(),
        port_type,
        port_index,
    )
    .ok_or_else(missing_endpoint)?;
    let port = &arena.graphs[slot].layerless_nodes[end.node].ports[end.port];
    let attached = (port.incoming_edges().len(), port.outgoing_edges().len());
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
        && graph.layerless_nodes.get(node).is_some_and(|candidate| {
            candidate.id == node_id && !candidate.port_constraints.is_side_fixed()
        })
    {
        return ensure_port_at_node(graph, graph_parent, node, node_id, port_type, port_index);
    }

    let candidate = graph.layerless_nodes.get(node)?;
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
    let candidate = graph.layerless_nodes.get(node)?;
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

/// The labels of a port that ELK keeps: those with text, each with the position it was given.
///
/// Ports the label loops of `ElkGraphImporter.transformPort` and `transformExternalPort`, with
/// `transformLabel`. Only a port label reads the position: one whose placement is fixed keeps it,
/// and its bounds set the margins of its port.
fn port_labels_to_lgraph(port: &ElkInputPort) -> impl Iterator<Item = LLabel> + '_ {
    port.labels
        .iter()
        .filter(|label| !label.text.is_empty())
        .map(|label| {
            let mut llabel = label_to_lgraph(label);
            llabel.position = LPoint {
                x: label.x,
                y: label.y,
            };
            llabel
        })
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
        || index.inside_self_loop_nodes.contains(node.id.as_str())
}

/// Whether the edge is an inside self loop.
///
/// Ports the `isInsideSelfLoop` tests in `ElkGraphImporter.importFlatGraph`,
/// `importHierarchicalGraph` and `hasInsideSelfLoops`. Java reads `isSelfloop()`, the node's
/// `INSIDE_SELF_LOOPS_ACTIVATE` and the edge's `INSIDE_SELF_LOOPS_YO` here. The caller has already
/// read all three (`is_inside_self_loop` in `layout.rs`) and sets `inside_self_loops_yo` to the
/// answer, so the importer reads that field alone.
fn is_inside_self_loop(edge: &ElkInputEdge) -> bool {
    // A caller bug, not caller input: only `layout.rs` sets the field.
    debug_assert!(
        !edge.inside_self_loops_yo || edge.source == edge.target,
        "inside self loop \"{}\" joins two nodes",
        edge.id
    );
    edge.inside_self_loops_yo
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
        // The map's order does not matter: each parent's list is filled from its own entry only.
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
            .filter(|edge| is_inside_self_loop(edge))
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
mod tests;
