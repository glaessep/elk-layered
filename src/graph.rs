//! Internal layered graph model.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/graph/LGraph.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/graph/LNode.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/graph/LEdge.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/graph/LPort.java

use super::options::{
    Alignment, ElkDirection, LayerConstraint, LayeredOptions, NodeLabelPlacement, NodeSizing,
    PortAlignment, PortConstraints, PortLabelPlacement, SpacingOptions,
};
use crate::random::JavaRandom;

mod arena;
mod id_vec;
mod ids;
mod node_table;
mod ports;
pub use arena::{GraphArena, GraphId, GraphParent, OtherGraphs};
pub use id_vec::{IdVec, VecId};
pub use ids::{EdgeId, NodeId};
pub use node_table::NodeTable;
#[cfg(test)]
pub(crate) use ports::port_ids;
pub use ports::{NodePorts, PortId};

#[derive(Debug, Clone, PartialEq)]
pub struct LGraph {
    /// Names this graph in its hierarchy.
    ///
    /// Crate-visible: The arena sets it once; every phase names the graph by it.
    pub(crate) id: GraphId,
    /// The caller's id of the node this graph belongs to. It only labels errors and results;
    /// nothing looks a graph up by it.
    ///
    /// Crate-visible: It only labels errors and results; no invariant ties it to another field.
    pub(crate) name: String,
    /// Crate-visible: Every phase reads the configured options, and the importer and configurator
    /// fill them before the run.
    pub(crate) options: LayeredOptions,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) size: LSize,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) padding: LPadding,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) offset: LPoint,
    /// Crate-visible: Every phase indexes the nodes by `NodeId`. The `IdVec` keeps the ids typed,
    /// and the private fields of `LNode` keep the node invariants.
    pub(crate) layerless_nodes: IdVec<NodeId, LNode>,
    /// Private: only the methods below add, remove or reorder layers, so `LNode::layer_index`
    /// and each layer's list stay in step.
    layers: Vec<Layer>,
    /// Crate-visible: Every phase indexes the edges by `EdgeId`. The `IdVec` keeps the ids typed,
    /// and the private fields of `LayeredEdge` keep the edge invariants.
    pub(crate) edges: IdVec<EdgeId, LayeredEdge>,
    /// Crate-visible: The importer and the configurator write it and later phases read it; no
    /// invariant ties it to another field.
    pub(crate) graph_properties: GraphProperties,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) cyclic: bool,
    /// Crate-visible: The configurator seeds it, and the processors that draw random numbers
    /// advance it in place. `JavaRandom` keeps its own state.
    pub(crate) random: JavaRandom,
    /// The node this graph lies in, set when the graph joins a [`GraphArena`] with `nest`.
    ///
    /// Crate-visible: `GraphArena::nest` sets it and the phases read it to reach the node the graph
    /// lies in.
    pub(crate) parent: Option<GraphParent>,
    /// Crate-visible: The layer constraint processors fill and empty it, and a component split and
    /// combine carry it; `hidden_mask` is the reader.
    pub(crate) hidden_nodes: Vec<NodeId>,
    /// Crate-visible: The hierarchical port processors fill it, and a component split and combine
    /// carry it.
    pub(crate) replaced_external_port_dummies: Vec<NodeId>,
    /// Crate-visible: Records of the compound preprocessing; each record holds its own ids.
    pub(crate) hierarchy_edges: Vec<HierarchyEdge>,
    /// Crate-visible: A record list that the compound preprocessing writes and the export reads;
    /// each record holds its own ids.
    pub(crate) cross_hierarchy_edges: Vec<CrossHierarchyEdge>,
    /// Crate-visible: The self loop processors install and read it, and a component split and
    /// combine carry it.
    pub(crate) self_loop_holders: Vec<SelfLoopHolder>,
    /// Crate-visible: The crossing minimization reads it. Nothing sets it yet.
    pub(crate) in_layer_successor_constraints_between_non_dummies: bool,
    /// The side tables of the nodes: the data that one processor owns.
    ///
    /// Crate-visible: the processors that own a table read and write it directly, and a component
    /// split and combine carry it as a whole.
    pub(crate) tables: NodeTables,
}

/// The side tables of the nodes of one graph. Each table belongs to one processor, as its doc
/// says. A component split and combine move all of them through `subset` and `absorb`, so a new
/// table needs its edits only here: the field, `subset`, `absorb`, and `map_node_ids` if its rows
/// hold node ids.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct NodeTables {
    /// What `NorthSouthPortPreprocessor` tells the crossing minimization about a node. Only the
    /// nodes with fixed port sides and their dummies have a row.
    pub(crate) in_layer: NodeTable<InLayerLayout>,
    /// `layered.edgeRouting.selfLoopDistribution` and `selfLoopOrdering` of the nodes the importer
    /// read them for. The self loop processors are the only readers.
    pub(crate) self_loop_options: NodeTable<SelfLoopOptions>,
    /// `portAlignment` of the nodes that set it. The node size and port placement is the only
    /// reader. A node without a row follows the graph's default.
    pub(crate) port_alignment: NodeTable<PortAlignment>,
    /// The options only the node placement reads, of the nodes the importer read them for.
    pub(crate) placement_options: NodeTable<PlacementOptions>,
    /// A node has a row when its layer constraint was set by the caller or by a hierarchical
    /// port dummy, not derived. Only the layer constraint preprocessing reads it.
    pub(crate) layer_constraint_explicit: NodeTable<()>,
}

impl NodeTables {
    /// The tables of a graph made from just `nodes`, in this order.
    pub(crate) fn subset(&self, nodes: &IdVec<NodeId, NodeId>) -> Self {
        Self {
            in_layer: self.in_layer.subset(nodes),
            self_loop_options: self.self_loop_options.subset(nodes),
            port_alignment: self.port_alignment.subset(nodes),
            placement_options: self.placement_options.subset(nodes),
            layer_constraint_explicit: self.layer_constraint_explicit.subset(nodes),
        }
    }

    /// Renames the node ids that rows hold, `kind` naming the id for an error. Used when graphs
    /// split or merge and their node ids change.
    pub(crate) fn map_node_ids<E>(
        &mut self,
        mut map: impl FnMut(NodeId, &'static str) -> Result<NodeId, E>,
    ) -> Result<(), E> {
        for row in self.in_layer.rows_mut() {
            if let Some(unit) = &mut row.layout_unit {
                *unit = map(*unit, "layout unit")?;
            }
            for node in &mut row.successor_constraints {
                *node = map(*node, "successor")?;
            }
            for node in &mut row.barycenter_associates {
                *node = map(*node, "barycenter associate")?;
            }
        }
        Ok(())
    }

    /// Moves every row of the tables `other` of a component into these tables: row `local` lands
    /// at `node_map[local]`.
    pub(crate) fn absorb(&mut self, node_map: &IdVec<NodeId, NodeId>, other: NodeTables) {
        self.in_layer.absorb(node_map, other.in_layer);
        self.self_loop_options
            .absorb(node_map, other.self_loop_options);
        self.port_alignment.absorb(node_map, other.port_alignment);
        self.placement_options
            .absorb(node_map, other.placement_options);
        self.layer_constraint_explicit
            .absorb(node_map, other.layer_constraint_explicit);
    }
}

/// The node options that only the node placement (`NetworkSimplexPlacer`) reads.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlacementOptions {
    /// `nodeFlexibility`.
    pub(crate) flexibility: super::options::NodeFlexibility,
    /// `portsSurrounding`: the margin the ports leave around a node, if the node sets it.
    pub(crate) ports_surrounding: Option<super::options::SpacingMargin>,
}

/// What `NorthSouthPortPreprocessor` records for one node, for the crossing minimization.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InLayerLayout {
    /// `InternalProperties.IN_LAYER_LAYOUT_UNIT`: the node a dummy and the node itself form a
    /// layout unit under.
    pub(crate) layout_unit: Option<NodeId>,
    /// `InternalProperties.IN_LAYER_SUCCESSOR_CONSTRAINTS`.
    pub(crate) successor_constraints: Vec<NodeId>,
    /// `InternalProperties.BARYCENTER_ASSOCIATES`.
    pub(crate) barycenter_associates: Vec<NodeId>,
}

/// The self loop options of one node.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SelfLoopOptions {
    /// `layered.edgeRouting.selfLoopDistribution`.
    pub(crate) distribution: super::options::SelfLoopDistributionStrategy,
    /// `layered.edgeRouting.selfLoopOrdering`.
    pub(crate) ordering: super::options::SelfLoopOrderingStrategy,
}

impl LGraph {
    /// Size returned for a completed provider invocation's root by
    /// `ElkGraphLayoutTransferrer.applyParentNodeLayout` and `ElkUtil.resizeNode`.
    /// This final minimum-size clamp does not realign the graph contents a second time.
    #[cfg(test)]
    pub fn exported_root_size(&self) -> LSize {
        let mut size = LSize {
            width: self.size.width + self.padding.left + self.padding.right,
            height: self.size.height + self.padding.top + self.padding.bottom,
        };
        if let Some(minimum) = self.options.effective_node_size_minimum() {
            size.width = size.width.max(minimum.width);
            size.height = size.height.max(minimum.height);
        }
        size
    }

    pub fn new(name: impl Into<String>, options: LayeredOptions) -> Self {
        Self {
            id: GraphId::ROOT,
            name: name.into(),
            options,
            size: LSize::default(),
            padding: LPadding::default(),
            offset: LPoint::default(),
            layerless_nodes: IdVec::new(),
            layers: Vec::new(),
            edges: IdVec::new(),
            graph_properties: GraphProperties::default(),
            cyclic: false,
            // `configure_graph_properties` replaces this with the stream seeded from
            // `options.random_seed`. Every public pipeline entry configures first.
            random: JavaRandom::default(),
            parent: None,
            hidden_nodes: Vec::new(),
            replaced_external_port_dummies: Vec::new(),
            hierarchy_edges: Vec::new(),
            cross_hierarchy_edges: Vec::new(),
            self_loop_holders: Vec::new(),
            in_layer_successor_constraints_between_non_dummies: false,
            tables: NodeTables::default(),
        }
    }

    /// Copies configured properties without copying the graph's dense arenas.
    /// Components use the already-resolved random stream through their serial executor.
    pub(crate) fn component_shell(&self) -> Self {
        let mut graph = Self::new(self.name.clone(), self.options.clone());
        graph.id = self.id;
        graph.options.node_size.constraints = crate::options::SizeConstraints::FIXED;
        graph.padding = self.padding;
        graph.graph_properties = self.graph_properties.clone();
        graph.parent = self.parent;
        graph.random = self.random.clone();
        graph.in_layer_successor_constraints_between_non_dummies =
            self.in_layer_successor_constraints_between_non_dummies;
        graph
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// The layer's nodes in order. Private so a node cannot join or leave a layer without its
    /// `layer_index` following: `LGraph` owns every change, as `LNode#setLayer` does in ELK.
    nodes: Vec<NodeId>,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) size: LSize,
}

impl AsRef<[NodeId]> for Layer {
    fn as_ref(&self) -> &[NodeId] {
        &self.nodes
    }
}

impl Layer {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            size: LSize::default(),
        }
    }

    /// The layer's nodes, top to bottom.
    pub fn nodes(&self) -> &[NodeId] {
        &self.nodes
    }
}

impl Default for Layer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LNode {
    /// Crate-visible: The caller's id of the node; it labels results and no invariant ties it to
    /// another field.
    pub(crate) id: String,
    /// What the node is, and the data only that kind of node has. `None` for a node of the
    /// input. Private so the data always fits the kind: see [`Self::kind`] and the accessors.
    dummy: Option<DummyData>,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) size: LSize,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) position: LPoint,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) margin: LMargin,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) padding: LPadding,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) labels: Vec<LLabel>,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) node_label_placement: NodeLabelPlacement,
    /// The node's own size options. ELK's `LabelAndNodeSizeProcessor` sizes the node by them.
    ///
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) node_size: NodeSizing,
    /// The node's own `portLabels.placement`, with the deprecated options folded in.
    ///
    /// Crate-visible: Plain data: the importer sets it, and the label and node size step and the
    /// export read it.
    pub(crate) port_label_placement: PortLabelPlacement,
    /// The node's own `portLabels.treatAsGroup`.
    ///
    /// Crate-visible: Plain data: the importer sets it, and the label and node size step reads it.
    pub(crate) port_labels_treat_as_group: bool,
    /// Crate-visible: `NodePorts` keeps the port order and the port ids in step; the node reaches
    /// it directly.
    pub(crate) ports: NodePorts,
    /// The graph inside this node, which the layout's [`GraphArena`] holds.
    ///
    /// Crate-visible: `GraphArena::nest` sets it, and the compound phases and the pipeline read it
    /// to reach the inner graph.
    pub(crate) nested_graph: Option<GraphId>,
    /// `InternalProperties.COMPOUND_NODE`: the node has children in the input, or activates inside
    /// self loops. Unlike `nested_graph`, it holds when a run lays out the node without its
    /// children.
    ///
    /// Crate-visible: Plain data: the importer sets it, and the label and node size step reads it.
    pub(crate) compound_node: bool,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) model_order: Option<usize>,
    /// The layer the node sits in. Private: `LGraph` sets it together with the layer's list.
    layer_index: Option<usize>,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) layer_constraint: super::options::LayerConstraint,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) node_alignment: Alignment,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) port_constraints: PortConstraints,
}

/// The data that only one kind of dummy node has. ELK keeps these as properties that are set
/// where they apply; here the node's kind and its data are one value, so a long edge dummy
/// cannot carry a label side and a reader of the wrong kind gets `None`.
#[derive(Debug, Clone, PartialEq)]
pub enum DummyData {
    /// `NodeType.LONG_EDGE`.
    LongEdge(LongEdgeData),
    /// `NodeType.LABEL`.
    Label(LabelData),
    /// `NodeType.EXTERNAL_PORT`.
    ExternalPort(ExternalPortData),
    /// `NodeType.NORTH_SOUTH_PORT`. It has no data of its own: its ports carry the origins.
    NorthSouthPort,
    /// `NodeType.BREAKING_POINT`. ELK's graph wrapping makes these; wrapping is not ported, so no
    /// processor builds one yet.
    BreakingPoint,
}

impl DummyData {
    fn kind(&self) -> LNodeKind {
        match self {
            Self::LongEdge(_) => LNodeKind::LongEdge,
            Self::Label(_) => LNodeKind::Label,
            Self::ExternalPort(_) => LNodeKind::ExternalPort,
            Self::NorthSouthPort => LNodeKind::NorthSouthPort,
            Self::BreakingPoint => LNodeKind::BreakingPoint,
        }
    }
}

/// The ports at the two ends of the long edge that a long edge dummy or a label dummy belongs
/// to. ELK sets both properties on both kinds of dummy.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LongEdgeEnds {
    /// `InternalProperties.LONG_EDGE_SOURCE`.
    pub source: Option<PortRef>,
    /// `InternalProperties.LONG_EDGE_TARGET`.
    pub target: Option<PortRef>,
}

/// Data of a long edge dummy.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LongEdgeData {
    /// The ports at the ends of the long edge.
    pub ends: LongEdgeEnds,
    /// `InternalProperties.LONG_EDGE_HAS_LABEL_DUMMIES`.
    pub has_label_dummies: bool,
    /// The edge the dummy was made from: ELK keeps it in `InternalProperties.ORIGIN`.
    pub origin_edge: Option<EdgeId>,
}

/// Data of a label dummy.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelData {
    /// The ports at the ends of the long edge the label sits on.
    pub ends: LongEdgeEnds,
    /// `InternalProperties.LABEL_SIDE`.
    pub side: LabelSide,
    /// The edge the labels belong to: ELK keeps it in `InternalProperties.ORIGIN`.
    pub origin_edge: Option<EdgeId>,
    /// The labels the dummy stands for, as `InternalProperties.REPRESENTED_LABELS`.
    pub represented_labels: Vec<LLabel>,
}

impl Default for LabelData {
    fn default() -> Self {
        Self {
            ends: LongEdgeEnds::default(),
            side: LabelSide::Below,
            origin_edge: None,
            represented_labels: Vec::new(),
        }
    }
}

/// Data of an external port dummy.
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalPortData {
    /// `InternalProperties.EXT_PORT_SIDE`.
    pub side: PortSide,
    /// `InternalProperties.EXT_PORT_SIZE`.
    pub size: LSize,
    /// `InternalProperties.PORT_RATIO_OR_POSITION`.
    pub ratio_or_position: f64,
    /// For the dummy of a port of the graph's own node: the position of that port among the
    /// node's input ports. ELK keeps the port in `InternalProperties.ORIGIN`.
    pub scope_port: Option<usize>,
    /// The dummy that an `HierarchicalPortOrthogonalEdgeRouter` replacement stands in for.
    pub replaced_dummy: Option<NodeId>,
    /// The id of the parent node's port the dummy was made for.
    pub parent_port_key: Option<String>,
    /// The type of that port.
    pub parent_port_type: Option<PortType>,
    /// The port of the parent graph's node, as `InternalProperties.ORIGIN`.
    pub origin_port: Option<GraphPortRef>,
    /// `InternalProperties.IN_LAYER_CONSTRAINT`.
    pub in_layer_constraint: InLayerConstraint,
    /// Whether ELK sets the layer constraint of the dummy on purpose, as it does for a west or
    /// east dummy, and for a north or south dummy once the graph is transposed. Such a constraint
    /// counts as explicit.
    pub layer_constraint_explicit: bool,
    /// `LayeredOptions.PORT_ANCHOR` of the dummy: where its port's anchor sat when the dummy was
    /// made. `GraphTransformer` turns the port but not this property, so in a vertical layout it
    /// keeps the coordinates from before the turn.
    pub anchor: LPoint,
    /// The label spacings the dummy carries: `SPACING_LABEL_PORT_HORIZONTAL`,
    /// `SPACING_LABEL_PORT_VERTICAL` and `SPACING_LABEL_LABEL`. `ElkGraphImporter` copies them from
    /// the parent of the node. A dummy made by `CompoundGraphPreprocessor` keeps the defaults.
    pub label_spacings: DummyLabelSpacings,
}

/// The label spacings of an external port dummy, which
/// `LabelAndNodeSizeProcessor.placeExternalPortDummyLabels` and
/// `HierarchicalPortOrthogonalEdgeRouter.restoreDummy` read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DummyLabelSpacings {
    /// `SPACING_LABEL_PORT_HORIZONTAL`.
    pub label_port_horizontal: f64,
    /// `SPACING_LABEL_PORT_VERTICAL`.
    pub label_port_vertical: f64,
    /// `SPACING_LABEL_LABEL`.
    pub label_label: f64,
}

impl DummyLabelSpacings {
    /// The defaults of the three options. A test holds them to those of [`SpacingOptions`].
    pub(crate) const DEFAULT: Self = Self {
        label_port_horizontal: 1.0,
        label_port_vertical: 1.0,
        label_label: 0.0,
    };
}

impl Default for DummyLabelSpacings {
    /// The defaults of the three options, for a dummy that was given none.
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl From<&SpacingOptions> for DummyLabelSpacings {
    /// The spacings a node's parent sets: what `transformExternalPort` copies to the dummy.
    fn from(spacing: &SpacingOptions) -> Self {
        Self {
            label_port_horizontal: spacing.label_port_horizontal,
            label_port_vertical: spacing.label_port_vertical,
            label_label: spacing.label_label,
        }
    }
}

/// Ports `ElkUtil.computeInsidePart(KVector, KVector, KVector, double, PortSide)`: how far a port
/// label reaches into the node behind its port. `label_position` is relative to the port.
pub(crate) fn compute_inside_part(
    label_position: LPoint,
    label_size: LSize,
    port_size: LSize,
    port_border_offset: f64,
    side: PortSide,
) -> f64 {
    match side {
        PortSide::North => f64::max(
            0.0,
            label_size.height + label_position.y - (port_size.height + port_border_offset),
        ),
        PortSide::South => f64::max(0.0, -label_position.y - port_border_offset),
        PortSide::East => f64::max(0.0, -label_position.x - port_border_offset),
        PortSide::West => f64::max(
            0.0,
            label_size.width + label_position.x - (port_size.width + port_border_offset),
        ),
        PortSide::Undefined => 0.0,
    }
}

/// The size of the label a dummy gets for a port label that sits outside the node: it keeps the
/// size along the side's axis only as far as it reaches into the node.
///
/// Ports the label loops of `ElkGraphImporter.transformExternalPort` and
/// `CompoundGraphPreprocessor.transformHierarchyEdges`. `fixed` says the port labels have a fixed
/// placement: then the label keeps the part that lies inside the node "after" the port, with
/// border offset 0. `reduce_width` says which axis goes: the width for a port on an east or west
/// side, the height otherwise.
pub(crate) fn outside_dummy_label_size(
    label_position: LPoint,
    label_size: LSize,
    port_size: LSize,
    side: PortSide,
    fixed: bool,
    reduce_width: bool,
) -> LSize {
    let inside_part = if fixed {
        compute_inside_part(label_position, label_size, port_size, 0.0, side)
    } else {
        0.0
    };
    if reduce_width {
        LSize {
            width: inside_part,
            height: label_size.height,
        }
    } else {
        LSize {
            width: label_size.width,
            height: inside_part,
        }
    }
}

impl Default for ExternalPortData {
    fn default() -> Self {
        Self {
            side: PortSide::Undefined,
            size: LSize::default(),
            ratio_or_position: 0.0,
            scope_port: None,
            replaced_dummy: None,
            parent_port_key: None,
            parent_port_type: None,
            origin_port: None,
            in_layer_constraint: InLayerConstraint::None,
            layer_constraint_explicit: false,
            anchor: LPoint::default(),
            label_spacings: DummyLabelSpacings::default(),
        }
    }
}

impl LNode {
    /// The index of the layer the node sits in, or `None` while it is layerless.
    pub fn layer_index(&self) -> Option<usize> {
        self.layer_index
    }

    /// What the node is.
    pub fn kind(&self) -> LNodeKind {
        self.dummy
            .as_ref()
            .map_or(LNodeKind::Normal, DummyData::kind)
    }

    /// Makes the node a dummy of the kind `kind`, with the data of a new dummy of that kind.
    /// [`LNodeKind::Normal`] makes it a node of the input again.
    pub fn set_kind(&mut self, kind: LNodeKind) {
        self.dummy = match kind {
            LNodeKind::Normal => None,
            LNodeKind::LongEdge => Some(DummyData::LongEdge(LongEdgeData::default())),
            LNodeKind::Label => Some(DummyData::Label(LabelData::default())),
            LNodeKind::ExternalPort => Some(DummyData::ExternalPort(ExternalPortData::default())),
            LNodeKind::NorthSouthPort => Some(DummyData::NorthSouthPort),
            LNodeKind::BreakingPoint => Some(DummyData::BreakingPoint),
        };
    }

    /// Makes the node a dummy that holds `data`.
    pub fn set_dummy(&mut self, data: DummyData) {
        self.dummy = Some(data);
    }

    /// The data of a long edge dummy, or `None` for any other node.
    pub fn long_edge(&self) -> Option<&LongEdgeData> {
        match &self.dummy {
            Some(DummyData::LongEdge(data)) => Some(data),
            _ => None,
        }
    }

    /// The data of a long edge dummy for change, or `None` for any other node.
    pub fn long_edge_mut(&mut self) -> Option<&mut LongEdgeData> {
        match &mut self.dummy {
            Some(DummyData::LongEdge(data)) => Some(data),
            _ => None,
        }
    }

    /// The data of a label dummy, or `None` for any other node.
    pub fn label_dummy(&self) -> Option<&LabelData> {
        match &self.dummy {
            Some(DummyData::Label(data)) => Some(data),
            _ => None,
        }
    }

    /// The data of a label dummy for change, or `None` for any other node.
    pub fn label_dummy_mut(&mut self) -> Option<&mut LabelData> {
        match &mut self.dummy {
            Some(DummyData::Label(data)) => Some(data),
            _ => None,
        }
    }

    /// The side of the node the external port dummy stands for. A node that is no external port
    /// dummy stands for none, so it has no side: [`PortSide::Undefined`].
    pub fn external_port_side(&self) -> PortSide {
        self.external_port()
            .map_or(PortSide::Undefined, |external| external.side)
    }

    /// The in-layer constraint of an external port dummy. A node that is no external port dummy
    /// has none: [`InLayerConstraint::None`].
    pub fn in_layer_constraint(&self) -> InLayerConstraint {
        self.external_port()
            .map_or(InLayerConstraint::None, |external| {
                external.in_layer_constraint
            })
    }

    /// The data of an external port dummy together with the layer constraint of the node, both
    /// for change, or `None` for any other node. A layer constraint and an external port's side
    /// change together in the graph transformations.
    pub fn external_port_and_layer_constraint_mut(
        &mut self,
    ) -> Option<(&mut ExternalPortData, &mut super::options::LayerConstraint)> {
        match &mut self.dummy {
            Some(DummyData::ExternalPort(data)) => Some((data, &mut self.layer_constraint)),
            _ => None,
        }
    }

    /// The ports at the ends of the long edge that a long edge dummy or a label dummy belongs
    /// to, or `None` for any other node.
    pub fn long_edge_ends(&self) -> Option<LongEdgeEnds> {
        match &self.dummy {
            Some(DummyData::LongEdge(data)) => Some(data.ends),
            Some(DummyData::Label(data)) => Some(data.ends),
            _ => None,
        }
    }

    /// The same for change.
    pub fn long_edge_ends_mut(&mut self) -> Option<&mut LongEdgeEnds> {
        match &mut self.dummy {
            Some(DummyData::LongEdge(data)) => Some(&mut data.ends),
            Some(DummyData::Label(data)) => Some(&mut data.ends),
            _ => None,
        }
    }

    /// The labels a label dummy stands for. A node that is no label dummy stands for none.
    pub fn represented_labels(&self) -> &[LLabel] {
        match &self.dummy {
            Some(DummyData::Label(data)) => &data.represented_labels,
            _ => &[],
        }
    }

    /// The edge a long edge dummy or a label dummy was made from, or `None` for any other node
    /// and for a dummy without one.
    pub fn origin_edge(&self) -> Option<EdgeId> {
        match &self.dummy {
            Some(DummyData::LongEdge(data)) => data.origin_edge,
            Some(DummyData::Label(data)) => data.origin_edge,
            _ => None,
        }
    }

    /// The data of an external port dummy, or `None` for any other node.
    pub fn external_port(&self) -> Option<&ExternalPortData> {
        match &self.dummy {
            Some(DummyData::ExternalPort(data)) => Some(data),
            _ => None,
        }
    }

    /// The data of an external port dummy for change, or `None` for any other node.
    pub fn external_port_mut(&mut self) -> Option<&mut ExternalPortData> {
        match &mut self.dummy {
            Some(DummyData::ExternalPort(data)) => Some(data),
            _ => None,
        }
    }

    pub fn new(id: impl Into<String>, width: f64, height: f64, model_order: Option<usize>) -> Self {
        Self {
            id: id.into(),
            dummy: None,
            size: LSize { width, height },
            position: LPoint::default(),
            margin: LMargin::default(),
            padding: LPadding::default(),
            labels: Vec::new(),
            node_label_placement: NodeLabelPlacement::FIXED,
            node_size: NodeSizing::default(),
            port_label_placement: PortLabelPlacement::OUTSIDE,
            port_labels_treat_as_group: true,
            ports: NodePorts::default(),
            nested_graph: None,
            compound_node: false,
            model_order,
            layer_index: None,
            layer_constraint: super::options::LayerConstraint::None,
            node_alignment: Alignment::Automatic,
            port_constraints: PortConstraints::Undefined,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LNodeKind {
    #[default]
    Normal,
    LongEdge,
    ExternalPort,
    Label,
    NorthSouthPort,
    // ELK's graph wrapping inserts these; wrapping is not ported, so none is built yet.
    #[allow(dead_code)]
    BreakingPoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InLayerConstraint {
    #[default]
    None,
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LPort {
    /// Crate-visible: The caller's id of the port; it labels results and no invariant ties it to
    /// another field.
    pub(crate) id: String,
    /// Crate-visible: The crossing minimization numbers the ports and is the only writer and
    /// reader.
    pub(crate) crossing_minimization_id: Option<usize>,
    /// The node that owns the port. Private: `LGraph::push_node_with` sets it.
    node: NodeId,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) port_type: PortType,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) side: PortSide,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) size: LSize,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) position: LPoint,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) anchor: LPoint,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) margin: LMargin,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) labels: Vec<LLabel>,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) model_order: Option<usize>,
    /// Crate-visible: The crossing minimization sets and reads it, and a component split and combine
    /// remap it.
    pub(crate) long_edge_target_node: Option<NodeId>,
    /// The position of the input port this port was made from, among its node's ports. `None` for
    /// a port ELK makes up. ELK keeps the input port in `InternalProperties.ORIGIN`.
    ///
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) input_index: Option<usize>,
    /// Whether the caller named this port, so an edge finds it by its id alone. A port that
    /// ELK makes up is found by its id and type. An external port dummy's port counts as named.
    ///
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) named: bool,
    /// `CompoundGraphPreprocessor.calculateNetFlow` for a port the caller named: the votes of
    /// every edge at the port, those outside its node too. `None` for a port ELK makes up.
    ///
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) hierarchy_net_flow: Option<isize>,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) port_index: Option<isize>,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) border_offset: Option<f64>,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) ratio_or_position: f64,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) connected_to_external_nodes: bool,
    /// Crate-visible: The hierarchical port processors set it, and a component split and combine
    /// remap it.
    pub(crate) port_dummy: Option<GraphNodeRef>,
    /// `InternalProperties.ORIGIN` of a port on a north or south port dummy: the port of the
    /// node whose edges `NorthSouthPortPreprocessor` moved here.
    ///
    /// Crate-visible: The north/south port processors set it, and a component split and combine
    /// remap it.
    pub(crate) origin_port: Option<PortRef>,
    /// `LayeredOptions.ALLOW_NON_FLOW_PORTS_TO_SWITCH_SIDES`, which only a port can set.
    ///
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) allow_non_flow_ports_to_switch_sides: bool,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) inside_connections: bool,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) collector_type: Option<PortType>,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) end_label_cell: Option<LabelCellLayout>,
    /// The end labels of the edges at the port, from `EndLabelPreprocessor` to
    /// `EndLabelPostprocessor`. Java leaves them on their edges and keeps a label cell; this
    /// crate holds them here, apart from the port's own labels.
    ///
    /// Crate-visible: the two end label steps and the end label sorter move them. The list is
    /// empty again before the components are combined, so the combine leaves it alone.
    pub(crate) end_labels: Vec<LLabel>,
    /// The edges that enter the port. Private: it must match each edge's `target`, so
    /// `LGraph` changes it together with the edge.
    incoming_edges: Vec<EdgeId>,
    /// The edges that leave the port. Private for the same reason, against `source`.
    outgoing_edges: Vec<EdgeId>,
}

impl LPort {
    pub fn new(id: impl Into<String>, node: NodeId, port_type: PortType) -> Self {
        Self {
            id: id.into(),
            crossing_minimization_id: None,
            node,
            port_type,
            side: PortSide::Undefined,
            size: LSize::default(),
            position: LPoint::default(),
            anchor: LPoint::default(),
            margin: LMargin::default(),
            labels: Vec::new(),
            model_order: None,
            long_edge_target_node: None,
            input_index: None,
            named: false,
            hierarchy_net_flow: None,
            port_index: None,
            border_offset: None,
            ratio_or_position: 0.0,
            connected_to_external_nodes: true,
            port_dummy: None,
            origin_port: None,
            allow_non_flow_ports_to_switch_sides: false,
            inside_connections: false,
            collector_type: None,
            end_label_cell: None,
            end_labels: Vec::new(),
            incoming_edges: Vec::new(),
            outgoing_edges: Vec::new(),
        }
    }

    pub fn set_side(&mut self, side: PortSide) {
        self.side = side;
        self.anchor = match side {
            PortSide::North => LPoint {
                x: self.size.width / 2.0,
                y: 0.0,
            },
            PortSide::East => LPoint {
                x: self.size.width,
                y: self.size.height / 2.0,
            },
            PortSide::South => LPoint {
                x: self.size.width / 2.0,
                y: self.size.height,
            },
            PortSide::West => LPoint {
                x: 0.0,
                y: self.size.height / 2.0,
            },
            PortSide::Undefined => self.anchor,
        };
    }

    /// A copy of the port that no edge is attached to.
    #[cfg(test)]
    pub(crate) fn clone_without_edges(&self) -> Self {
        Self {
            incoming_edges: Vec::new(),
            outgoing_edges: Vec::new(),
            ..self.clone()
        }
    }

    /// Renames the port's owner and its edges in place, keeping the order. Used when graphs
    /// merge and their ids change.
    pub(crate) fn map_ids<E>(
        &mut self,
        map_node: impl FnOnce(NodeId) -> Result<NodeId, E>,
        mut map_incoming: impl FnMut(EdgeId) -> Result<EdgeId, E>,
        mut map_outgoing: impl FnMut(EdgeId) -> Result<EdgeId, E>,
    ) -> Result<(), E> {
        self.node = map_node(self.node)?;
        for edge in &mut self.incoming_edges {
            *edge = map_incoming(*edge)?;
        }
        for edge in &mut self.outgoing_edges {
            *edge = map_outgoing(*edge)?;
        }
        Ok(())
    }

    /// The node that owns the port.
    pub fn node(&self) -> NodeId {
        self.node
    }

    /// The edges that enter the port, in the port's order.
    pub fn incoming_edges(&self) -> &[EdgeId] {
        &self.incoming_edges
    }

    /// The edges that leave the port, in the port's order.
    pub fn outgoing_edges(&self) -> &[EdgeId] {
        &self.outgoing_edges
    }

    pub fn net_flow(&self) -> isize {
        self.incoming_edges.len() as isize - self.outgoing_edges.len() as isize
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayeredEdge {
    /// Crate-visible: The caller's id of the edge; it labels results and no invariant ties it to
    /// another field.
    pub(crate) id: String,
    /// The source port. Private: it must match the `outgoing_edges` of that port, so
    /// `LGraph::set_edge_source` is the only way to change it, as `LEdge#setSource` is in ELK.
    source: PortRef,
    /// The target port. Private for the same reason, against `incoming_edges`.
    target: PortRef,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) source_node_id: String,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) target_node_id: String,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) labels: Vec<LLabel>,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) minlen: usize,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) reversed: bool,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) bend_points: Vec<LPoint>,
    /// `LayeredOptions.JUNCTION_POINTS` of the edge. ELK keeps `null` until a router adds one, so
    /// an empty list stands for `null`.
    ///
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) junction_points: Vec<LPoint>,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) model_order: Option<usize>,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) priority_direction: i32,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) priority_shortness: i32,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) priority_straightness: i32,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) thickness: f64,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) original_opposite_port: Option<PortRef>,
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) compound_segment: Option<CompoundEdgeSegment>,
    /// Whether the importer made the edge from an inside self loop of the input. Its ends are
    /// external port dummies that stand for no port of the input, so the export needs this mark to
    /// tell it from an edge the compound steps made. ELK keeps the input edge in `ORIGIN`.
    ///
    /// Crate-visible: Plain data: no other field depends on it, and several phases read or write
    /// it.
    pub(crate) inside_self_loop: bool,
}

impl LayeredEdge {
    /// An edge between two ports with no labels, no bend points and a thickness of 0. ELK's
    /// default thickness is 1, as the importer sets it: a caller that wants it sets it on the
    /// edge. `LGraph::add_edge` attaches the edge to its ports.
    pub fn new(id: impl Into<String>, source: PortRef, target: PortRef) -> Self {
        Self {
            id: id.into(),
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
            thickness: 0.0,
            original_opposite_port: None,
            compound_segment: None,
            inside_self_loop: false,
        }
    }

    /// Renames the ports' nodes in place. Used when graphs merge and their ids change.
    pub(crate) fn map_ports<E>(
        &mut self,
        mut map: impl FnMut(&mut PortRef) -> Result<(), E>,
    ) -> Result<(), E> {
        map(&mut self.source)?;
        map(&mut self.target)
    }

    /// The next piece of an edge that is split or copied between two new ports: it keeps the
    /// id, the node ids, the minimum length, the direction, the model order, the priorities, the
    /// thickness, the opposite port and the compound segment of `old`. It has no labels, bend
    /// points or junction points.
    pub fn segment_of(old: &LayeredEdge, source: PortRef, target: PortRef) -> Self {
        let mut segment = Self::new(old.id.clone(), source, target);
        segment.source_node_id = old.source_node_id.clone();
        segment.target_node_id = old.target_node_id.clone();
        segment.minlen = old.minlen;
        segment.reversed = old.reversed;
        segment.model_order = old.model_order;
        segment.priority_direction = old.priority_direction;
        segment.priority_shortness = old.priority_shortness;
        segment.priority_straightness = old.priority_straightness;
        segment.thickness = old.thickness;
        segment.original_opposite_port = old.original_opposite_port;
        segment.compound_segment = old.compound_segment;
        segment
    }

    /// The port the edge leaves.
    pub fn source(&self) -> PortRef {
        self.source
    }

    /// The port the edge enters.
    pub fn target(&self) -> PortRef {
        self.target
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HierarchyEdge {
    pub id: String,
    pub source_node_id: String,
    pub target_node_id: String,
    pub source_port_key: String,
    pub target_port_key: String,
    /// Whether the caller named the source port. A made-up port can have the id of a named port
    /// of the same node, so the key alone does not say which of the two an end sits at.
    pub source_port_named: bool,
    /// The same for the target port.
    pub target_port_named: bool,
    /// The nodes above the source, from the hierarchy edge's own graph down.
    pub source_path: Vec<HierarchyStep>,
    /// The nodes above the target, from the hierarchy edge's own graph down.
    pub target_path: Vec<HierarchyStep>,
    pub labels: Vec<LLabel>,
    pub minlen: usize,
    pub model_order: Option<usize>,
    pub priority_direction: i32,
    pub priority_shortness: i32,
    pub priority_straightness: i32,
}

/// One node above the end of a hierarchy edge, and the graph inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HierarchyStep {
    /// The caller's id of the node.
    pub node_id: String,
    /// The graph inside the node.
    pub graph: GraphId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PortRef {
    pub node: NodeId,
    pub port: PortId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphNodeRef {
    /// The graph that holds the node.
    pub graph_id: GraphId,
    pub node: NodeId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphPortRef {
    /// The graph that holds the port's node.
    pub graph_id: GraphId,
    pub port: PortRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompoundEdgeSegment {
    Output { depth: usize },
    Input { depth: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossHierarchyEdge {
    pub original_edge_id: String,
    pub original_model_order: Option<usize>,
    /// The graph that holds the edge `edge` names.
    pub graph_id: GraphId,
    pub edge: EdgeId,
    pub segment: CompoundEdgeSegment,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelfLoopHolder {
    pub node: NodeId,
    pub hyper_loops: Vec<SelfHyperLoop>,
    pub ports_hidden: bool,
    /// Ports taken off the node's list while they are hidden, in the order they stood in the list.
    /// They keep their slots, so the holder's ports and edges still name them.
    pub hidden_ports: Vec<PortId>,
    pub original_port_constraints: PortConstraints,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelfHyperLoop {
    pub ports: Vec<SelfLoopPort>,
    pub edges: Vec<SelfLoopEdge>,
    pub labels: Option<SelfHyperLoopLabels>,
    pub self_loop_type: Option<SelfLoopType>,
    pub leftmost_port: Option<PortId>,
    pub rightmost_port: Option<PortId>,
    pub occupied_sides: Vec<PortSide>,
    pub routing_slots: [usize; 5],
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelfHyperLoopLabels {
    pub id: Option<usize>,
    pub label_refs: Vec<SelfLoopLabelRef>,
    pub size: LSize,
    pub position: LPoint,
    pub side: Option<PortSide>,
    pub alignment: Option<SelfLoopLabelAlignment>,
    pub alignment_reference_port: Option<PortId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelfLoopLabelRef {
    pub edge: EdgeId,
    pub label: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfLoopLabelAlignment {
    Center,
    Left,
    Right,
    Top,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelfLoopPort {
    pub port: PortId,
    pub had_only_self_loops: bool,
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelfLoopEdge {
    pub edge: EdgeId,
    pub source_port: PortId,
    pub target_port: PortId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfLoopType {
    OneSide,
    TwoSidesCorner,
    TwoSidesOpposing,
    ThreeSides,
    FourSides,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LLabel {
    pub text: String,
    pub size: LSize,
    pub position: LPoint,
    pub placement: EdgeLabelPlacement,
    pub inline: bool,
    pub label_side: Option<LabelSide>,
    pub end_label_edge: Option<EdgeId>,
    pub original_label_edge: Option<String>,
    /// Opaque caller-owned label identity, preserved through layout transformations.
    pub source_index: Option<usize>,
    /// The label's own `nodeLabels.placement`, which wins over its node's.
    pub node_label_placement: Option<NodeLabelPlacement>,
}

impl LLabel {
    pub fn new(text: impl Into<String>, width: f64, height: f64) -> Self {
        Self {
            text: text.into(),
            size: LSize { width, height },
            position: LPoint::default(),
            placement: EdgeLabelPlacement::Center,
            inline: false,
            label_side: None,
            end_label_edge: None,
            original_label_edge: None,
            source_index: None,
            node_label_placement: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LSize {
    pub width: f64,
    pub height: f64,
}

/// The top left corner and the size of a port, from the four numbers a port carries in the input.
/// `ElkPort` and `ElkInputPort` both read their geometry through it.
pub(crate) fn port_box(x: f64, y: f64, width: f64, height: f64) -> (LPoint, LSize) {
    (LPoint { x, y }, LSize { width, height })
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LPadding {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LMargin {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PortType {
    #[default]
    Input,
    Output,
}

pub(crate) const fn collector_port_id_suffix(port_type: PortType) -> &'static str {
    match port_type {
        PortType::Input => ":collector:Input",
        PortType::Output => ":collector:Output",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PortSide {
    #[default]
    Undefined,
    North,
    East,
    South,
    West,
}

impl PortSide {
    pub fn opposed(self) -> Self {
        match self {
            Self::North => Self::South,
            Self::East => Self::West,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::Undefined => Self::Undefined,
        }
    }

    pub fn right(self) -> Self {
        match self {
            Self::North => Self::East,
            Self::East => Self::South,
            Self::South => Self::West,
            Self::West => Self::North,
            Self::Undefined => Self::Undefined,
        }
    }

    pub fn left(self) -> Self {
        match self {
            Self::North => Self::West,
            Self::East => Self::North,
            Self::South => Self::East,
            Self::West => Self::South,
            Self::Undefined => Self::Undefined,
        }
    }

    pub fn ordinal(self) -> usize {
        match self {
            Self::Undefined => 0,
            Self::North => 1,
            Self::East => 2,
            Self::South => 3,
            Self::West => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabelCellLayout {
    pub position: LPoint,
    pub size: LSize,
    pub horizontal_layout: bool,
    pub horizontal_alignment: HorizontalLabelAlignment,
    pub vertical_alignment: VerticalLabelAlignment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HorizontalLabelAlignment {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VerticalLabelAlignment {
    Top,
    #[default]
    Center,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EdgeLabelPlacement {
    Head,
    Tail,
    #[default]
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LabelSide {
    Above,
    #[default]
    Below,
    Inline,
}

impl LabelSide {
    pub fn opposite(self) -> Self {
        match self {
            Self::Above => Self::Below,
            Self::Below => Self::Above,
            Self::Inline => Self::Inline,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GraphProperties {
    pub self_loops: bool,
    pub center_labels: bool,
    pub end_labels: bool,
    pub non_free_ports: bool,
    pub north_south_ports: bool,
    pub hyperedges: bool,
    pub external_ports: bool,
    pub hypernodes: bool,
}

impl GraphProperties {
    pub fn apply_to_options(&self, options: &mut LayeredOptions) {
        options.graph_has_self_loops = self.self_loops;
        options.graph_has_center_labels = self.center_labels;
        options.graph_has_end_labels = self.end_labels;
        options.graph_has_non_free_ports = self.non_free_ports;
        options.graph_has_north_south_ports = self.north_south_ports;
        options.graph_has_hyperedges = self.hyperedges;
        options.graph_has_external_ports = self.external_ports;
        options.graph_has_hypernodes = self.hypernodes;
    }
}

#[cfg(test)]
impl LGraph {
    /// The id of the node named `id`. Panics with the name when no node has it.
    pub(crate) fn node_named(&self, id: &str) -> NodeId {
        self.nodes_with_ids()
            .find(|(_, node)| node.id == id)
            .map(|(node_id, _)| node_id)
            .unwrap_or_else(|| panic!("no node is named {id:?}"))
    }
}

impl LGraph {
    /// The ids of all nodes, in the order they stand in `layerless_nodes`. The iterator does not
    /// borrow the graph, so the loop body may change it.
    pub fn node_ids(&self) -> impl DoubleEndedIterator<Item = NodeId> + ExactSizeIterator + use<> {
        self.layerless_nodes.ids()
    }

    /// Every node with its id, in the order of `layerless_nodes`.
    pub fn nodes_with_ids(
        &self,
    ) -> impl DoubleEndedIterator<Item = (NodeId, &LNode)> + ExactSizeIterator {
        self.layerless_nodes.iter_with_ids()
    }

    /// The ids of all edges, in the order they stand in `edges`. The iterator does not borrow the
    /// graph, so the loop body may change it.
    pub fn edge_ids(&self) -> impl DoubleEndedIterator<Item = EdgeId> + ExactSizeIterator + use<> {
        self.edges.ids()
    }

    /// Every edge with its id, in the order of `edges`.
    pub fn edges_with_ids(
        &self,
    ) -> impl DoubleEndedIterator<Item = (EdgeId, &LayeredEdge)> + ExactSizeIterator {
        self.edges.iter_with_ids()
    }

    /// The id the next edge pushed onto `edges` gets.
    pub fn next_edge_id(&self) -> EdgeId {
        self.edges.next_id()
    }

    /// The id the next node pushed onto `layerless_nodes` gets.
    pub fn next_node_id(&self) -> NodeId {
        self.layerless_nodes.next_id()
    }

    /// Builds a node and pushes it onto `layerless_nodes`. `build` gets the id the node will have,
    /// so it can put the id in the node's name. Whatever owner `build` gives the node's ports, the
    /// method sets every port's owner to that id. The method returns the id.
    ///
    /// Use it instead of `next_node_id` followed by `push`: that pair leaves a gap where a node
    /// can get another id than the one its ports name.
    pub fn push_node_with(&mut self, build: impl FnOnce(NodeId) -> LNode) -> NodeId {
        let id = self.next_node_id();
        let mut node = build(id);
        for port in node.ports.slots_mut() {
            port.node = id;
        }
        self.layerless_nodes.push(node)
    }

    /// The node that `node` forms an in-layer layout unit under, if the north/south port
    /// preprocessing recorded one.
    pub(crate) fn in_layer_layout_unit(&self, node: NodeId) -> Option<NodeId> {
        self.tables
            .in_layer
            .get(node)
            .and_then(|row| row.layout_unit)
    }

    /// The nodes that must stay below `node` in its layer. Empty if none were recorded.
    pub(crate) fn in_layer_successor_constraints(&self, node: NodeId) -> &[NodeId] {
        self.tables
            .in_layer
            .get(node)
            .map_or(&[], |row| row.successor_constraints.as_slice())
    }

    /// The nodes whose positions pull `node` in the crossing minimization. Empty if none were
    /// recorded.
    pub(crate) fn barycenter_associates(&self, node: NodeId) -> &[NodeId] {
        self.tables
            .in_layer
            .get(node)
            .map_or(&[], |row| row.barycenter_associates.as_slice())
    }

    /// The `portAlignment` the node sets, or `None` if it sets none.
    pub(crate) fn port_alignment_of(&self, node: NodeId) -> Option<PortAlignment> {
        self.tables.port_alignment.get(node).copied()
    }

    /// Whether the caller, a hierarchical port dummy, or the graph transform (which transposes a
    /// north or south dummy) set the layer constraint of `node`.
    pub(crate) fn is_layer_constraint_explicit(&self, node: NodeId) -> bool {
        self.tables.layer_constraint_explicit.get(node).is_some()
            || self.layerless_nodes[node]
                .external_port()
                .is_some_and(|external| external.layer_constraint_explicit)
    }

    /// Records that the layer constraint of `node` is explicit.
    pub(crate) fn mark_layer_constraint_explicit(&mut self, node: NodeId) {
        self.tables.layer_constraint_explicit.row_mut(node);
    }

    /// Which nodes the layer constraint preprocessing has hidden from the layering. A node is
    /// hidden while it is in [`Self::hidden_nodes`].
    pub fn hidden_mask(&self) -> IdVec<NodeId, bool> {
        let mut mask = IdVec::filled(self.layerless_nodes.len(), false);
        for &node in &self.hidden_nodes {
            mask[node] = true;
        }
        mask
    }

    /// The layers, left to right.
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    /// The size of layer `layer_index` for change. The size is plain data, and this is the only
    /// way to a layer for change that cannot touch its node list.
    pub(crate) fn layer_size_mut(&mut self, layer_index: usize) -> &mut LSize {
        &mut self.layers[layer_index].size
    }

    /// Renames every node id in the layers in place, keeping the order. Used when graphs merge
    /// and their node ids change; no node's `layer_index` moves.
    pub(crate) fn map_layer_nodes<E>(
        &mut self,
        mut map: impl FnMut(NodeId) -> Result<NodeId, E>,
    ) -> Result<(), E> {
        for layer in &mut self.layers {
            for node in &mut layer.nodes {
                *node = map(*node)?;
            }
        }
        Ok(())
    }

    /// Adds an empty layer at the right end and returns its index.
    pub fn push_layer(&mut self) -> usize {
        self.layers.push(Layer::new());
        self.layers.len() - 1
    }

    pub fn clear_layers(&mut self) {
        self.layers.clear();
        for node in &mut self.layerless_nodes {
            node.layer_index = None;
        }
    }

    pub fn set_node_layer(&mut self, node_index: NodeId, layer_index: usize) {
        while self.layers.len() <= layer_index {
            self.layers.push(Layer::new());
        }

        if let Some(old_layer) = self.layerless_nodes[node_index].layer_index
            && let Some(layer) = self.layers.get_mut(old_layer)
        {
            remove_node(&mut layer.nodes, node_index);
        }

        self.layerless_nodes[node_index].layer_index = Some(layer_index);
        self.layers[layer_index].nodes.push(node_index);
    }

    /// Puts a node into layer `layer_index` at `position`, as `LNode#setLayer(int, Layer)` does.
    /// The node leaves the layer it was in. A `position` past the end appends.
    pub fn insert_node_into_layer(
        &mut self,
        node_index: NodeId,
        layer_index: usize,
        position: usize,
    ) {
        self.remove_node_from_layer(node_index);
        while self.layers.len() <= layer_index {
            self.layers.push(Layer::new());
        }
        let nodes = &mut self.layers[layer_index].nodes;
        nodes.insert(position.min(nodes.len()), node_index);
        self.layerless_nodes[node_index].layer_index = Some(layer_index);
    }

    /// Swaps the places of two nodes that sit in layers: each takes the other's layer and
    /// position. Does nothing unless both are in a layer.
    pub fn swap_layer_places(&mut self, first: NodeId, second: NodeId) {
        let (Some(first_layer), Some(second_layer)) = (
            self.layerless_nodes[first].layer_index,
            self.layerless_nodes[second].layer_index,
        ) else {
            return;
        };
        let Some(first_position) = self.layers[first_layer]
            .nodes
            .iter()
            .position(|n| *n == first)
        else {
            return;
        };
        let Some(second_position) = self.layers[second_layer]
            .nodes
            .iter()
            .position(|n| *n == second)
        else {
            return;
        };
        self.layers[first_layer].nodes[first_position] = second;
        self.layers[second_layer].nodes[second_position] = first;
        self.layerless_nodes[first].layer_index = Some(second_layer);
        self.layerless_nodes[second].layer_index = Some(first_layer);
    }

    /// Makes layer `layer_index` hold exactly `nodes`, in this order.
    ///
    /// Mirrors `LNode#setLayer`, which ELK calls once per node: a node that joins gets
    /// `layer_index`, leaves its old layer, and a node that no longer appears becomes layerless.
    /// Use it to reorder a layer, or to drop nodes from one in a single step.
    pub fn set_layer_nodes(&mut self, layer_index: usize, nodes: Vec<NodeId>) {
        while self.layers.len() <= layer_index {
            self.layers.push(Layer::new());
        }

        let old_nodes = std::mem::take(&mut self.layers[layer_index].nodes);
        for node in old_nodes {
            self.layerless_nodes[node].layer_index = None;
        }
        for &node in &nodes {
            if let Some(other) = self.layerless_nodes[node].layer_index
                && other != layer_index
                && let Some(layer) = self.layers.get_mut(other)
            {
                remove_node(&mut layer.nodes, node);
            }
            self.layerless_nodes[node].layer_index = Some(layer_index);
        }
        self.layers[layer_index].nodes = nodes;
    }

    pub fn insert_layer(&mut self, layer_index: usize) {
        self.layers.insert(layer_index, Layer::new());
        for node in &mut self.layerless_nodes {
            if let Some(current) = node.layer_index
                && current >= layer_index
            {
                node.layer_index = Some(current + 1);
            }
        }
    }

    pub fn remove_node_from_layer(&mut self, node_index: NodeId) {
        let Some(layer_index) = self.layerless_nodes[node_index].layer_index.take() else {
            return;
        };
        if let Some(layer) = self.layers.get_mut(layer_index) {
            remove_node(&mut layer.nodes, node_index);
        }
    }

    /// Takes the node at `position` out of layer `layer_index` and makes it layerless. Unlike
    /// [`Self::remove_node_from_layer`] it works by place, so a loop over a layer always makes
    /// progress. Returns `None` if the layer has no node there.
    pub fn remove_node_at(&mut self, layer_index: usize, position: usize) -> Option<NodeId> {
        let layer = self.layers.get_mut(layer_index)?;
        if position >= layer.nodes.len() {
            return None;
        }
        let node = layer.nodes.remove(position);
        self.layerless_nodes[node].layer_index = None;
        Some(node)
    }

    pub fn compact_empty_layers(&mut self) {
        let mut layer_map = vec![None; self.layers.len()];
        let mut compacted = Vec::new();

        for (old_index, layer) in self.layers.iter().enumerate() {
            if !layer.nodes().is_empty() {
                layer_map[old_index] = Some(compacted.len());
                compacted.push(layer.clone());
            }
        }

        self.layers = compacted;
        for node in &mut self.layerless_nodes {
            if let Some(old_index) = node.layer_index {
                node.layer_index = layer_map.get(old_index).copied().flatten();
            }
        }
    }

    pub fn add_port(
        &mut self,
        node_index: NodeId,
        port_type: PortType,
        side: PortSide,
        position: LPoint,
    ) -> Option<PortRef> {
        let node = self.layerless_nodes.get_mut(node_index)?;
        let mut port = LPort::new(
            format!("{}:{}", node.id, node.ports.slot_count()),
            node_index,
            port_type,
        );
        port.set_side(side);
        port.position = position;
        let port = node.ports.push(port);
        Some(PortRef {
            node: node_index,
            port,
        })
    }

    /// Return a collector port for an input or output edge endpoint, creating it if needed.
    ///
    /// Source:
    /// https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/graph/LGraphUtil.java
    pub fn provide_collector_port(
        &mut self,
        node_index: NodeId,
        port_type: PortType,
        side: PortSide,
    ) -> Option<PortRef> {
        let node = self.layerless_nodes.get_mut(node_index)?;
        if let Some((port, _)) = node
            .ports
            .iter_with_ids()
            .find(|(_, port)| port.collector_type == Some(port_type))
        {
            return Some(PortRef {
                node: node_index,
                port,
            });
        }

        let mut port = LPort::new(
            format!("{}{}", node.id, collector_port_id_suffix(port_type)),
            node_index,
            port_type,
        );
        port.collector_type = Some(port_type);
        port.set_side(side);
        port.position = centered_port_position(node.size, side);
        let port = node.ports.push(port);
        Some(PortRef {
            node: node_index,
            port,
        })
    }

    pub fn set_edge_source(&mut self, edge_index: EdgeId, source: PortRef) -> bool {
        if self.edges.get(edge_index).is_none() || !port_exists(self, source) {
            return false;
        }

        let old_source = self.edges[edge_index].source;
        if port_exists(self, old_source) {
            remove_edge(
                &mut self.layerless_nodes[old_source.node].ports[old_source.port].outgoing_edges,
                edge_index,
            );
        }

        self.layerless_nodes[source.node].ports[source.port]
            .outgoing_edges
            .push(edge_index);
        self.edges[edge_index].source = source;
        true
    }

    pub fn set_edge_target(&mut self, edge_index: EdgeId, target: PortRef) -> bool {
        self.set_edge_target_at(edge_index, target, None)
    }

    pub fn set_edge_target_at(
        &mut self,
        edge_index: EdgeId,
        target: PortRef,
        insert_at: Option<usize>,
    ) -> bool {
        if self.edges.get(edge_index).is_none() || !port_exists(self, target) {
            return false;
        }

        let old_target = self.edges[edge_index].target;
        if port_exists(self, old_target) {
            remove_edge(
                &mut self.layerless_nodes[old_target.node].ports[old_target.port].incoming_edges,
                edge_index,
            );
        }

        let incoming_edges =
            &mut self.layerless_nodes[target.node].ports[target.port].incoming_edges;
        if let Some(index) = insert_at {
            incoming_edges.insert(index.min(incoming_edges.len()), edge_index);
        } else {
            incoming_edges.push(edge_index);
        }
        self.edges[edge_index].target = target;
        true
    }

    /// Puts `edge` into incoming-edge slot `position` of `port`, in place of the edge there, and
    /// makes `port` the target of `edge`. ELK does this with a `ListIterator#set` in
    /// `LongEdgeJoiner#joinAt`, so the edge keeps the exact slot and the other edges do not shift.
    ///
    /// The replaced edge still names `port` as its target but is no longer in its list. Drop it.
    pub(crate) fn replace_incoming_edge_at(
        &mut self,
        port: PortRef,
        position: usize,
        edge: EdgeId,
    ) {
        self.layerless_nodes[port.node].ports[port.port].incoming_edges[position] = edge;
        self.edges[edge].target = port;
    }

    /// Removes the first `count` incoming edges of `port`, as `LongEdgeJoiner#joinAt` does with
    /// the edges it has moved to another target.
    ///
    /// Each removed edge must name another target already, or be dropped by the caller.
    pub(crate) fn remove_leading_incoming_edges(&mut self, port: PortRef, count: usize) {
        self.layerless_nodes[port.node].ports[port.port]
            .incoming_edges
            .drain(..count);
    }

    /// Removes the first `count` outgoing edges of `port`. The same rule holds as for
    /// [`Self::remove_leading_incoming_edges`]: the caller drops each removed edge.
    pub(crate) fn remove_leading_outgoing_edges(&mut self, port: PortRef, count: usize) {
        self.layerless_nodes[port.node].ports[port.port]
            .outgoing_edges
            .drain(..count);
    }

    /// Puts the incoming edges of `port` in this order, as sorting `LPort#getIncomingEdges()`
    /// does in ELK. Returns `false` and changes nothing unless `order` names each edge of the
    /// port exactly once.
    #[cfg(test)]
    pub fn reorder_incoming_edges(&mut self, port: PortRef, order: Vec<EdgeId>) -> bool {
        match self
            .layerless_nodes
            .get_mut(port.node)
            .and_then(|node| node.ports.get_mut(port.port))
        {
            Some(port) if same_edges(&port.incoming_edges, &order) => {
                port.incoming_edges = order;
                true
            }
            _ => false,
        }
    }

    /// Puts the outgoing edges of `port` in this order. The rule is the one of
    /// [`Self::reorder_incoming_edges`].
    #[cfg(test)]
    pub fn reorder_outgoing_edges(&mut self, port: PortRef, order: Vec<EdgeId>) -> bool {
        match self
            .layerless_nodes
            .get_mut(port.node)
            .and_then(|node| node.ports.get_mut(port.port))
        {
            Some(port) if same_edges(&port.outgoing_edges, &order) => {
                port.outgoing_edges = order;
                true
            }
            _ => false,
        }
    }

    pub fn detach_edge_source(&mut self, edge_index: EdgeId) -> Option<PortRef> {
        let source = self.edges.get(edge_index)?.source;
        if port_exists(self, source) {
            remove_edge(
                &mut self.layerless_nodes[source.node].ports[source.port].outgoing_edges,
                edge_index,
            );
        }
        Some(source)
    }

    pub fn detach_edge_target(&mut self, edge_index: EdgeId) -> Option<PortRef> {
        let target = self.edges.get(edge_index)?.target;
        if port_exists(self, target) {
            remove_edge(
                &mut self.layerless_nodes[target.node].ports[target.port].incoming_edges,
                edge_index,
            );
        }
        Some(target)
    }

    pub fn detach_edge(&mut self, edge_index: EdgeId) -> Option<(PortRef, PortRef)> {
        let source = self.detach_edge_source(edge_index)?;
        let target = self.detach_edge_target(edge_index)?;
        Some((source, target))
    }

    pub fn edge_source_attached(&self, edge_index: EdgeId) -> bool {
        let Some(edge) = self.edges.get(edge_index) else {
            return false;
        };
        self.layerless_nodes
            .get(edge.source.node)
            .and_then(|node| node.ports.get(edge.source.port))
            .map(|port| port.outgoing_edges.contains(&edge_index))
            .unwrap_or(false)
    }

    pub fn edge_target_attached(&self, edge_index: EdgeId) -> bool {
        let Some(edge) = self.edges.get(edge_index) else {
            return false;
        };
        self.layerless_nodes
            .get(edge.target.node)
            .and_then(|node| node.ports.get(edge.target.port))
            .map(|port| port.incoming_edges.contains(&edge_index))
            .unwrap_or(false)
    }

    pub fn add_edge(&mut self, edge: LayeredEdge) -> Option<EdgeId> {
        if !port_exists(self, edge.source) || !port_exists(self, edge.target) {
            return None;
        }

        let edge_index = self.next_edge_id();
        let source = edge.source;
        let target = edge.target;
        self.edges.push(edge);
        self.layerless_nodes[source.node].ports[source.port]
            .outgoing_edges
            .push(edge_index);
        self.layerless_nodes[target.node].ports[target.port]
            .incoming_edges
            .push(edge_index);
        Some(edge_index)
    }

    pub fn node_incoming_edges(&self, node_index: NodeId) -> Vec<EdgeId> {
        self.layerless_nodes[node_index]
            .ports
            .iter()
            .flat_map(|port| port.incoming_edges.iter().copied())
            .collect()
    }

    pub fn node_outgoing_edges(&self, node_index: NodeId) -> Vec<EdgeId> {
        self.layerless_nodes[node_index]
            .ports
            .iter()
            .flat_map(|port| port.outgoing_edges.iter().copied())
            .collect()
    }

    pub fn node_connected_edges(&self, node_index: NodeId) -> Vec<EdgeId> {
        self.layerless_nodes[node_index]
            .ports
            .iter()
            .flat_map(|port| {
                port.incoming_edges
                    .iter()
                    .chain(port.outgoing_edges.iter())
                    .copied()
            })
            .collect()
    }

    /// The ports of a node on one side, in the order of the node's port list, as
    /// `LNode#getPorts(PortSide)` gives them.
    pub(crate) fn ports_on_side(&self, node: NodeId, side: PortSide) -> Vec<PortRef> {
        self.layerless_nodes[node]
            .ports
            .iter_with_ids()
            .filter_map(|(port, data)| (data.side == side).then_some(PortRef { node, port }))
            .collect()
    }

    /// ELK's `LNode#getPorts(PortType.OUTPUT)` is based on actual outgoing edge incidence.
    pub fn port_has_output_type(&self, port_ref: PortRef) -> bool {
        self.layerless_nodes
            .get(port_ref.node)
            .and_then(|node| node.ports.get(port_ref.port))
            .map(|port| !port.outgoing_edges.is_empty())
            .unwrap_or(false)
    }

    /// Puts the ports the node lists in this order. Returns `false` and changes nothing unless the
    /// order names each listed port exactly once.
    ///
    /// Only the node's list changes. A port keeps its `PortId`, so no edge, holder or dummy that
    /// names the port has to change.
    pub fn reorder_node_ports(
        &mut self,
        node_index: NodeId,
        new_order: impl IntoIterator<Item = PortId>,
    ) -> bool {
        let new_order = new_order.into_iter().collect::<Vec<_>>();
        self.layerless_nodes
            .get_mut(node_index)
            .is_some_and(|node| node.ports.set_order(new_order))
    }
}

/// Reverse an edge according to `LEdge.reverse(...)`.
///
/// Source:
/// https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/graph/LEdge.java
///
/// `adapt_ports` follows ELK's collector-port handling: a reversed edge that previously targeted
/// an input collector is moved to the same node's output collector, and a reversed edge that
/// previously sourced an output collector is moved to the same node's input collector.
pub fn reverse_edge(graph: &mut LGraph, edge_index: EdgeId, adapt_ports: bool) -> bool {
    let Some(edge) = graph.edges.get(edge_index) else {
        return false;
    };
    let old_source = edge.source;
    let old_target = edge.target;

    if !port_exists(graph, old_source) || !port_exists(graph, old_target) {
        return false;
    }

    remove_edge(
        &mut graph.layerless_nodes[old_source.node].ports[old_source.port].outgoing_edges,
        edge_index,
    );
    remove_edge(
        &mut graph.layerless_nodes[old_target.node].ports[old_target.port].incoming_edges,
        edge_index,
    );

    let new_source = if adapt_ports
        && graph.layerless_nodes[old_target.node].ports[old_target.port].collector_type
            == Some(PortType::Input)
    {
        graph
            .provide_collector_port(old_target.node, PortType::Output, PortSide::East)
            .unwrap_or(old_target)
    } else {
        old_target
    };

    let new_target = if adapt_ports
        && graph.layerless_nodes[old_source.node].ports[old_source.port].collector_type
            == Some(PortType::Output)
    {
        graph
            .provide_collector_port(old_source.node, PortType::Input, PortSide::West)
            .unwrap_or(old_source)
    } else {
        old_source
    };

    graph.layerless_nodes[new_source.node].ports[new_source.port]
        .outgoing_edges
        .push(edge_index);
    graph.layerless_nodes[new_target.node].ports[new_target.port]
        .incoming_edges
        .push(edge_index);

    let edge = &mut graph.edges[edge_index];
    edge.source = new_source;
    edge.target = new_target;

    for label in &mut edge.labels {
        label.placement = match label.placement {
            EdgeLabelPlacement::Head => EdgeLabelPlacement::Tail,
            EdgeLabelPlacement::Tail => EdgeLabelPlacement::Head,
            EdgeLabelPlacement::Center => EdgeLabelPlacement::Center,
        };
    }

    edge.reversed = !edge.reversed;
    edge.bend_points.reverse();
    true
}

fn port_exists(graph: &LGraph, port_ref: PortRef) -> bool {
    graph
        .layerless_nodes
        .get(port_ref.node)
        .and_then(|node| node.ports.get(port_ref.port))
        .is_some()
}

fn centered_port_position(node_size: LSize, side: PortSide) -> LPoint {
    match side {
        PortSide::North => LPoint {
            x: node_size.width / 2.0,
            y: 0.0,
        },
        PortSide::East => LPoint {
            x: node_size.width,
            y: node_size.height / 2.0,
        },
        PortSide::South => LPoint {
            x: node_size.width / 2.0,
            y: node_size.height,
        },
        PortSide::West => LPoint {
            x: 0.0,
            y: node_size.height / 2.0,
        },
        PortSide::Undefined => LPoint::default(),
    }
}

/// Whether two lists hold the same edges, in any order, each as often as the other.
#[cfg(test)]
fn same_edges(left: &[EdgeId], right: &[EdgeId]) -> bool {
    let mut left = left.to_vec();
    let mut right = right.to_vec();
    left.sort_unstable();
    right.sort_unstable();
    left == right
}

fn remove_edge(edges: &mut Vec<EdgeId>, edge_index: EdgeId) {
    if let Some(position) = edges.iter().position(|candidate| *candidate == edge_index) {
        edges.remove(position);
    }
}

fn remove_node(nodes: &mut Vec<NodeId>, node_index: NodeId) {
    if let Some(position) = nodes.iter().position(|candidate| *candidate == node_index) {
        nodes.remove(position);
    }
}

/// Create an external port dummy node following ELK's `LGraphUtil.createExternalPortDummy(...)`.
///
/// Source:
/// https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/graph/LGraphUtil.java
///
/// `node` is the id the dummy will have in its graph. The dummy's one port names it as its owner.
///
/// `data` holds what the caller knows about the dummy and ELK's method does not take: the scope
/// port and the parent port key and type. The method sets the side, the size, the ratio or
/// position and the in-layer constraint itself, so it replaces those four fields of `data`.
#[allow(clippy::too_many_arguments)]
pub fn create_external_port_dummy(
    node: NodeId,
    id: impl Into<String>,
    port_id: impl Into<String>,
    port_type: PortType,
    port_constraints: PortConstraints,
    port_side: PortSide,
    net_flow: isize,
    port_position: LPoint,
    port_size: LSize,
    port_node_size: LSize,
    border_offset: f64,
    layout_direction: ElkDirection,
    data: ExternalPortData,
) -> LNode {
    let external_side = if port_constraints.is_side_fixed() {
        port_side
    } else if net_flow >= 0 {
        port_side_from_direction(layout_direction)
    } else {
        port_side_from_direction(layout_direction).opposed()
    };

    let mut dummy = LNode::new(id, 0.0, 0.0, None);
    let mut external = ExternalPortData {
        side: external_side,
        size: port_size,
        ratio_or_position: 0.0,
        in_layer_constraint: InLayerConstraint::None,
        layer_constraint_explicit: false,
        ..data
    };
    dummy.port_constraints = PortConstraints::FixedPos;

    let position = match external_side {
        PortSide::West | PortSide::East => LPoint {
            x: 0.0,
            y: port_size.height / 2.0,
        },
        PortSide::North | PortSide::South => LPoint {
            x: port_size.width / 2.0,
            y: 0.0,
        },
        PortSide::Undefined => LPoint {
            x: port_size.width / 2.0,
            y: port_size.height / 2.0,
        },
    };
    let mut dummy_port_side = external_side.opposed();

    match external_side {
        PortSide::West => {
            dummy.layer_constraint = LayerConstraint::FirstSeparate;
            external.layer_constraint_explicit = true;
            dummy.size.height = port_size.height;
            if border_offset < 0.0 {
                dummy.size.width = -border_offset;
            }
        }
        PortSide::East => {
            dummy.layer_constraint = LayerConstraint::LastSeparate;
            external.layer_constraint_explicit = true;
            dummy.size.height = port_size.height;
            if border_offset < 0.0 {
                dummy.size.width = -border_offset;
            }
        }
        PortSide::North => {
            external.in_layer_constraint = InLayerConstraint::Top;
            dummy.size.width = port_size.width;
            if border_offset < 0.0 {
                dummy.size.height = -border_offset;
            }
        }
        PortSide::South => {
            external.in_layer_constraint = InLayerConstraint::Bottom;
            dummy.size.width = port_size.width;
            if border_offset < 0.0 {
                dummy.size.height = -border_offset;
            }
        }
        PortSide::Undefined => {
            dummy_port_side = PortSide::Undefined;
        }
    }

    external.anchor = position;
    let mut port = LPort::new(port_id, node, port_type);
    port.side = dummy_port_side;
    port.position = position;
    port.set_side(dummy_port_side);
    port.border_offset = Some(border_offset);
    if port_constraints.is_order_fixed() {
        external.ratio_or_position = port_ratio_or_position(
            external_side,
            port_position,
            port_node_size,
            port_constraints.is_ratio_fixed(),
        );
    }
    dummy.set_dummy(DummyData::ExternalPort(external));
    dummy.ports.push(port);
    dummy
}

/// The part of `LGraphUtil.createExternalPortDummy` that reads `PORT_INDEX`: with exactly
/// `FIXED_ORDER` and an index on the port, the index replaces the position as the order
/// information. Indices count the other way on the south and west sides.
pub fn apply_port_index_to_dummy(
    dummy: &mut LNode,
    port_constraints: PortConstraints,
    port_index: Option<isize>,
) {
    let Some(index) = port_index else {
        return;
    };
    if port_constraints != PortConstraints::FixedOrder {
        return;
    }
    // A node that is no external port dummy has no port ratio or position to set.
    let Some(external) = dummy.external_port_mut() else {
        return;
    };
    external.ratio_or_position = match external.side {
        PortSide::North | PortSide::East => index as f64,
        PortSide::South | PortSide::West => -(index as f64),
        PortSide::Undefined => 0.0,
    };
}

/// Ports `PortSide.fromDirection`. An undefined direction counts as `Right`, as the configurator
/// makes it.
pub(crate) fn port_side_from_direction(direction: ElkDirection) -> PortSide {
    match direction {
        ElkDirection::Right | ElkDirection::Undefined => PortSide::East,
        ElkDirection::Left => PortSide::West,
        ElkDirection::Down => PortSide::South,
        ElkDirection::Up => PortSide::North,
    }
}

/// Whether a port on `side` lies across the layout flow: on a north or south side of a horizontal
/// layout, or on an east or west side of a vertical one.
///
/// An undefined direction has no flow, so no side lies across it. The importer resolves the
/// direction of the root graph before any caller asks, as `LGraphUtil.getDirection` does.
pub(crate) fn is_across_flow(direction: ElkDirection, side: PortSide) -> bool {
    match direction {
        ElkDirection::Left | ElkDirection::Right => {
            matches!(side, PortSide::North | PortSide::South)
        }
        ElkDirection::Up | ElkDirection::Down => {
            matches!(side, PortSide::East | PortSide::West)
        }
        ElkDirection::Undefined => false,
    }
}

fn port_ratio_or_position(
    side: PortSide,
    port_position: LPoint,
    port_node_size: LSize,
    ratio_fixed: bool,
) -> f64 {
    match side {
        PortSide::West | PortSide::East => {
            if ratio_fixed && port_node_size.height > 0.0 {
                port_position.y / port_node_size.height
            } else {
                port_position.y
            }
        }
        PortSide::North | PortSide::South => {
            if ratio_fixed && port_node_size.width > 0.0 {
                port_position.x / port_node_size.width
            } else {
                port_position.x
            }
        }
        PortSide::Undefined => 0.0,
    }
}

#[cfg(test)]
mod tests;
