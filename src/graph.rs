//! Internal layered graph model.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/graph/LGraph.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/graph/LNode.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/graph/LEdge.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/graph/LPort.java

use super::options::{
    Alignment, ElkDirection, LayerConstraint, LayeredOptions, NodeLabelPlacement, NodeSizing,
    PortAlignment, PortConstraints,
};
use crate::random::JavaRandom;

mod arena;
mod ports;
pub use arena::GraphId;
#[cfg(test)]
pub(crate) use ports::port_ids;
pub use ports::{NodePorts, PortId};

#[derive(Debug, Clone, PartialEq)]
pub struct LGraph {
    /// Names this graph in its hierarchy.
    pub id: GraphId,
    /// The caller's id of the node this graph belongs to. It only labels errors and results;
    /// nothing looks a graph up by it.
    pub name: String,
    pub options: LayeredOptions,
    pub size: LSize,
    pub padding: LPadding,
    pub offset: LPoint,
    pub layerless_nodes: Vec<LNode>,
    pub layers: Vec<Layer>,
    pub edges: Vec<LayeredEdge>,
    pub graph_properties: GraphProperties,
    pub cyclic: bool,
    pub(crate) random: JavaRandom,
    pub parent_node_id: Option<String>,
    pub hidden_nodes: Vec<usize>,
    pub replaced_external_port_dummies: Vec<usize>,
    pub hierarchy_edges: Vec<HierarchyEdge>,
    pub cross_hierarchy_edges: Vec<CrossHierarchyEdge>,
    pub self_loop_holders: Vec<SelfLoopHolder>,
    pub in_layer_successor_constraints_between_non_dummies: bool,
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
            layerless_nodes: Vec::new(),
            layers: Vec::new(),
            edges: Vec::new(),
            graph_properties: GraphProperties::default(),
            cyclic: false,
            // `configure_graph_properties` replaces this with the stream seeded from
            // `options.random_seed`. Every public pipeline entry configures first.
            random: JavaRandom::default(),
            parent_node_id: None,
            hidden_nodes: Vec::new(),
            replaced_external_port_dummies: Vec::new(),
            hierarchy_edges: Vec::new(),
            cross_hierarchy_edges: Vec::new(),
            self_loop_holders: Vec::new(),
            in_layer_successor_constraints_between_non_dummies: false,
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
        graph.parent_node_id = self.parent_node_id.clone();
        graph.random = self.random.clone();
        graph.in_layer_successor_constraints_between_non_dummies =
            self.in_layer_successor_constraints_between_non_dummies;
        graph
    }

    pub fn sync_graph_properties_to_options(&mut self) {
        let result = self.try_for_each_graph_mut(|graph| {
            graph.graph_properties.apply_to_options(&mut graph.options);
            Ok::<(), std::convert::Infallible>(())
        });
        result.expect("infallible graph property synchronization");
    }

    pub(crate) fn try_for_each_graph_mut<E>(
        &mut self,
        mut visit: impl FnMut(&mut LGraph) -> Result<(), E>,
    ) -> Result<(), E> {
        struct DetachedGraph {
            graph: Box<LGraph>,
            parent: Option<usize>,
            parent_node: usize,
        }

        fn take_nested_graphs(graph: &mut LGraph) -> Vec<(usize, Box<LGraph>)> {
            graph
                .layerless_nodes
                .iter_mut()
                .enumerate()
                .filter_map(|(node, data)| data.nested_graph.take().map(|graph| (node, graph)))
                .collect()
        }

        let mut detached = take_nested_graphs(self)
            .into_iter()
            .map(|(parent_node, graph)| DetachedGraph {
                graph,
                parent: None,
                parent_node,
            })
            .collect::<Vec<_>>();
        let mut cursor = 0usize;
        while cursor < detached.len() {
            let children = take_nested_graphs(&mut detached[cursor].graph);
            detached.extend(
                children
                    .into_iter()
                    .map(|(parent_node, graph)| DetachedGraph {
                        graph,
                        parent: Some(cursor),
                        parent_node,
                    }),
            );
            cursor += 1;
        }

        let mut root_children = Vec::new();
        let mut children_by_parent = vec![Vec::new(); detached.len()];
        for (index, detached_graph) in detached.iter().enumerate() {
            if let Some(parent) = detached_graph.parent {
                children_by_parent[parent].push(index);
            } else {
                root_children.push(index);
            }
        }
        let mut traversal = Vec::with_capacity(detached.len());
        let mut stack = root_children.into_iter().rev().collect::<Vec<_>>();
        while let Some(index) = stack.pop() {
            traversal.push(index);
            stack.extend(children_by_parent[index].iter().rev().copied());
        }

        let mut result = visit(self);
        if result.is_ok() {
            for index in traversal {
                if let Err(error) = visit(&mut detached[index].graph) {
                    result = Err(error);
                    break;
                }
            }
        }

        while let Some(detached_graph) = detached.pop() {
            let parent_node = detached_graph.parent_node;
            if let Some(parent) = detached_graph.parent {
                detached[parent].graph.layerless_nodes[parent_node].nested_graph =
                    Some(detached_graph.graph);
            } else {
                self.layerless_nodes[parent_node].nested_graph = Some(detached_graph.graph);
            }
        }
        result
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub nodes: Vec<usize>,
    pub size: LSize,
}

impl AsRef<[usize]> for Layer {
    fn as_ref(&self) -> &[usize] {
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
}

impl Default for Layer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LNode {
    pub id: String,
    pub kind: LNodeKind,
    pub size: LSize,
    pub position: LPoint,
    pub margin: LMargin,
    pub padding: LPadding,
    pub labels: Vec<LLabel>,
    pub represented_labels: Vec<LLabel>,
    pub node_label_placement: NodeLabelPlacement,
    /// The node's own size options. ELK's `LabelAndNodeSizeProcessor` sizes the node by them.
    pub node_size: NodeSizing,
    /// `layered.edgeRouting.selfLoopDistribution`, which ELK reads from each node.
    pub self_loop_distribution: super::options::SelfLoopDistributionStrategy,
    /// `layered.edgeRouting.selfLoopOrdering`, which ELK reads from each node.
    pub self_loop_ordering: super::options::SelfLoopOrderingStrategy,
    pub ports: NodePorts,
    pub nested_graph: Option<Box<LGraph>>,
    pub model_order: Option<usize>,
    pub component_priority: i32,
    pub layer_index: Option<usize>,
    pub layer_constraint: super::options::LayerConstraint,
    pub node_alignment: Alignment,
    pub port_constraints: PortConstraints,
    pub port_alignment: Option<PortAlignment>,
    pub node_flexibility: super::options::NodeFlexibility,
    pub ports_surrounding: Option<super::options::SpacingMargin>,
    pub external_port_side: PortSide,
    pub external_port_size: LSize,
    /// For the dummy of a port of the graph's own node: the position of that port among the
    /// node's input ports. ELK keeps the port in `InternalProperties.ORIGIN`.
    pub scope_port: Option<usize>,
    pub port_ratio_or_position: f64,
    pub replaced_external_port_dummy: Option<usize>,
    pub in_layer_successor_constraints: Vec<usize>,
    pub in_layer_layout_unit: Option<usize>,
    pub barycenter_associates: Vec<usize>,
    pub origin_port: Option<GraphPortRef>,
    pub origin_edge: Option<usize>,
    pub parent_port_key: Option<String>,
    pub parent_port_type: Option<PortType>,
    pub long_edge_source: Option<PortRef>,
    pub long_edge_target: Option<PortRef>,
    pub long_edge_has_label_dummies: bool,
    pub label_side: LabelSide,
    pub in_layer_constraint: InLayerConstraint,
    pub layer_constraint_explicit: bool,
    pub hidden: bool,
    pub compound: bool,
}

impl LNode {
    pub fn new(id: impl Into<String>, width: f64, height: f64, model_order: Option<usize>) -> Self {
        Self {
            id: id.into(),
            kind: LNodeKind::Normal,
            size: LSize { width, height },
            position: LPoint::default(),
            margin: LMargin::default(),
            padding: LPadding::default(),
            labels: Vec::new(),
            represented_labels: Vec::new(),
            node_label_placement: NodeLabelPlacement::FIXED,
            node_size: NodeSizing::default(),
            self_loop_distribution: Default::default(),
            self_loop_ordering: Default::default(),
            ports: NodePorts::default(),
            nested_graph: None,
            model_order,
            component_priority: 0,
            layer_index: None,
            layer_constraint: super::options::LayerConstraint::None,
            node_alignment: Alignment::Automatic,
            port_constraints: PortConstraints::Undefined,
            port_alignment: None,
            node_flexibility: super::options::NodeFlexibility::None,
            ports_surrounding: None,
            external_port_side: PortSide::Undefined,
            external_port_size: LSize::default(),
            scope_port: None,
            port_ratio_or_position: 0.0,
            replaced_external_port_dummy: None,
            in_layer_successor_constraints: Vec::new(),
            in_layer_layout_unit: None,
            barycenter_associates: Vec::new(),
            origin_port: None,
            origin_edge: None,
            parent_port_key: None,
            parent_port_type: None,
            long_edge_source: None,
            long_edge_target: None,
            long_edge_has_label_dummies: false,
            label_side: LabelSide::Below,
            in_layer_constraint: InLayerConstraint::None,
            layer_constraint_explicit: false,
            hidden: false,
            compound: false,
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
    pub id: String,
    pub crossing_minimization_id: Option<usize>,
    pub node: usize,
    pub port_type: PortType,
    pub side: PortSide,
    pub size: LSize,
    pub position: LPoint,
    pub anchor: LPoint,
    pub margin: LMargin,
    pub labels: Vec<LLabel>,
    pub model_order: Option<usize>,
    pub long_edge_target_node: Option<usize>,
    /// The position of the input port this port was made from, among its node's ports. `None` for
    /// a port ELK makes up. ELK keeps the input port in `InternalProperties.ORIGIN`.
    pub input_index: Option<usize>,
    /// Whether the caller named this port, so an edge finds it by its id alone. A port that
    /// ELK makes up is found by its id and type. An external port dummy's port counts as named.
    pub named: bool,
    /// `CompoundGraphPreprocessor.calculateNetFlow` for a port the caller named: the votes of
    /// every edge at the port, those outside its node too. `None` for a port ELK makes up.
    pub hierarchy_net_flow: Option<isize>,
    pub port_index: Option<isize>,
    pub border_offset: Option<f64>,
    pub ratio_or_position: f64,
    pub connected_to_external_nodes: bool,
    pub port_dummy: Option<GraphNodeRef>,
    /// `InternalProperties.ORIGIN` of a port on a north or south port dummy: the port of the
    /// node whose edges `NorthSouthPortPreprocessor` moved here.
    pub origin_port: Option<PortRef>,
    /// `LayeredOptions.ALLOW_NON_FLOW_PORTS_TO_SWITCH_SIDES`, which only a port can set.
    pub allow_non_flow_ports_to_switch_sides: bool,
    pub inside_connections: bool,
    pub collector_type: Option<PortType>,
    pub end_label_cell: Option<LabelCellLayout>,
    pub incoming_edges: Vec<usize>,
    pub outgoing_edges: Vec<usize>,
}

impl LPort {
    pub fn new(id: impl Into<String>, node: usize, port_type: PortType) -> Self {
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

    pub fn net_flow(&self) -> isize {
        self.incoming_edges.len() as isize - self.outgoing_edges.len() as isize
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayeredEdge {
    pub id: String,
    pub source: PortRef,
    pub target: PortRef,
    pub source_node_id: String,
    pub target_node_id: String,
    pub labels: Vec<LLabel>,
    pub minlen: usize,
    pub reversed: bool,
    pub bend_points: Vec<LPoint>,
    /// `LayeredOptions.JUNCTION_POINTS` of the edge. ELK keeps `null` until a router adds one, so
    /// an empty list stands for `null`.
    pub junction_points: Vec<LPoint>,
    pub model_order: Option<usize>,
    pub priority_direction: i32,
    pub priority_shortness: i32,
    pub priority_straightness: i32,
    pub thickness: f64,
    pub original_opposite_port: Option<PortRef>,
    pub compound_segment: Option<CompoundEdgeSegment>,
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
    pub source_path: Vec<String>,
    pub target_path: Vec<String>,
    pub labels: Vec<LLabel>,
    pub minlen: usize,
    pub model_order: Option<usize>,
    pub priority_direction: i32,
    pub priority_shortness: i32,
    pub priority_straightness: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PortRef {
    pub node: usize,
    pub port: PortId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphNodeRef {
    pub graph_id: GraphId,
    pub node: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphPortRef {
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
    pub graph_id: GraphId,
    pub edge: usize,
    pub segment: CompoundEdgeSegment,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelfLoopHolder {
    pub node: usize,
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
    pub edge: usize,
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
    pub edge: usize,
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
    pub end_label_edge: Option<usize>,
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

impl LGraph {
    pub fn clear_layers(&mut self) {
        self.layers.clear();
        for node in &mut self.layerless_nodes {
            node.layer_index = None;
        }
    }

    pub fn set_node_layer(&mut self, node_index: usize, layer_index: usize) {
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

    pub fn remove_node_from_layer(&mut self, node_index: usize) {
        let Some(layer_index) = self.layerless_nodes[node_index].layer_index.take() else {
            return;
        };
        if let Some(layer) = self.layers.get_mut(layer_index) {
            remove_node(&mut layer.nodes, node_index);
        }
    }

    pub fn compact_empty_layers(&mut self) {
        let mut layer_map = vec![None; self.layers.len()];
        let mut compacted = Vec::new();

        for (old_index, layer) in self.layers.iter().enumerate() {
            if !layer.nodes.is_empty() {
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
        node_index: usize,
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
        node_index: usize,
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

    pub fn set_edge_source(&mut self, edge_index: usize, source: PortRef) -> bool {
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

    pub fn set_edge_target(&mut self, edge_index: usize, target: PortRef) -> bool {
        self.set_edge_target_at(edge_index, target, None)
    }

    pub fn set_edge_target_at(
        &mut self,
        edge_index: usize,
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

    pub fn detach_edge_source(&mut self, edge_index: usize) -> Option<PortRef> {
        let source = self.edges.get(edge_index)?.source;
        if port_exists(self, source) {
            remove_edge(
                &mut self.layerless_nodes[source.node].ports[source.port].outgoing_edges,
                edge_index,
            );
        }
        Some(source)
    }

    pub fn detach_edge_target(&mut self, edge_index: usize) -> Option<PortRef> {
        let target = self.edges.get(edge_index)?.target;
        if port_exists(self, target) {
            remove_edge(
                &mut self.layerless_nodes[target.node].ports[target.port].incoming_edges,
                edge_index,
            );
        }
        Some(target)
    }

    pub fn detach_edge(&mut self, edge_index: usize) -> Option<(PortRef, PortRef)> {
        let source = self.detach_edge_source(edge_index)?;
        let target = self.detach_edge_target(edge_index)?;
        Some((source, target))
    }

    pub fn edge_source_attached(&self, edge_index: usize) -> bool {
        let Some(edge) = self.edges.get(edge_index) else {
            return false;
        };
        self.layerless_nodes
            .get(edge.source.node)
            .and_then(|node| node.ports.get(edge.source.port))
            .map(|port| port.outgoing_edges.contains(&edge_index))
            .unwrap_or(false)
    }

    pub fn edge_target_attached(&self, edge_index: usize) -> bool {
        let Some(edge) = self.edges.get(edge_index) else {
            return false;
        };
        self.layerless_nodes
            .get(edge.target.node)
            .and_then(|node| node.ports.get(edge.target.port))
            .map(|port| port.incoming_edges.contains(&edge_index))
            .unwrap_or(false)
    }

    pub fn add_edge(&mut self, edge: LayeredEdge) -> Option<usize> {
        if !port_exists(self, edge.source) || !port_exists(self, edge.target) {
            return None;
        }

        let edge_index = self.edges.len();
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

    pub fn node_incoming_edges(&self, node_index: usize) -> Vec<usize> {
        self.layerless_nodes[node_index]
            .ports
            .iter()
            .flat_map(|port| port.incoming_edges.iter().copied())
            .collect()
    }

    pub fn node_outgoing_edges(&self, node_index: usize) -> Vec<usize> {
        self.layerless_nodes[node_index]
            .ports
            .iter()
            .flat_map(|port| port.outgoing_edges.iter().copied())
            .collect()
    }

    pub fn node_connected_edges(&self, node_index: usize) -> Vec<usize> {
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
    pub(crate) fn ports_on_side(&self, node: usize, side: PortSide) -> Vec<PortRef> {
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
        node_index: usize,
        new_order: impl IntoIterator<Item = PortId>,
    ) -> bool {
        let new_order = new_order.into_iter().collect::<Vec<_>>();
        self.layerless_nodes
            .get_mut(node_index)
            .is_some_and(|node| node.ports.set_order(&new_order))
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
pub fn reverse_edge(graph: &mut LGraph, edge_index: usize, adapt_ports: bool) -> bool {
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

fn remove_edge(edges: &mut Vec<usize>, edge_index: usize) {
    if let Some(position) = edges.iter().position(|candidate| *candidate == edge_index) {
        edges.remove(position);
    }
}

fn remove_node(nodes: &mut Vec<usize>, node_index: usize) {
    if let Some(position) = nodes.iter().position(|candidate| *candidate == node_index) {
        nodes.remove(position);
    }
}

/// Create an external port dummy node following ELK's `LGraphUtil.createExternalPortDummy(...)`.
///
/// Source:
/// https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/graph/LGraphUtil.java
#[allow(clippy::too_many_arguments)]
pub fn create_external_port_dummy(
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
) -> LNode {
    let external_side = if port_constraints.is_side_fixed() {
        port_side
    } else if net_flow >= 0 {
        port_side_from_direction(layout_direction)
    } else {
        port_side_from_direction(layout_direction).opposed()
    };

    let mut dummy = LNode::new(id, 0.0, 0.0, None);
    dummy.kind = LNodeKind::ExternalPort;
    dummy.port_constraints = PortConstraints::FixedPos;
    dummy.external_port_side = external_side;
    dummy.external_port_size = port_size;

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
            dummy.layer_constraint_explicit = true;
            dummy.size.height = port_size.height;
            if border_offset < 0.0 {
                dummy.size.width = -border_offset;
            }
        }
        PortSide::East => {
            dummy.layer_constraint = LayerConstraint::LastSeparate;
            dummy.layer_constraint_explicit = true;
            dummy.size.height = port_size.height;
            if border_offset < 0.0 {
                dummy.size.width = -border_offset;
            }
        }
        PortSide::North => {
            dummy.in_layer_constraint = InLayerConstraint::Top;
            dummy.size.width = port_size.width;
            if border_offset < 0.0 {
                dummy.size.height = -border_offset;
            }
        }
        PortSide::South => {
            dummy.in_layer_constraint = InLayerConstraint::Bottom;
            dummy.size.width = port_size.width;
            if border_offset < 0.0 {
                dummy.size.height = -border_offset;
            }
        }
        PortSide::Undefined => {
            dummy_port_side = PortSide::Undefined;
        }
    }

    let mut port = LPort::new(port_id, 0, port_type);
    port.side = dummy_port_side;
    port.position = position;
    port.set_side(dummy_port_side);
    port.border_offset = Some(border_offset);
    if port_constraints.is_order_fixed() {
        dummy.port_ratio_or_position = port_ratio_or_position(
            external_side,
            port_position,
            port_node_size,
            port_constraints.is_ratio_fixed(),
        );
    }
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
    dummy.port_ratio_or_position = match dummy.external_port_side {
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
mod tests {
    use super::*;

    // Mutation: swap the two arms of `is_across_flow`, or give an undefined direction the
    // horizontal arm. A case of the table then fails.
    #[test]
    fn a_side_across_the_flow_is_a_north_or_south_side_of_a_horizontal_layout() {
        use ElkDirection::{Down, Left, Right, Undefined, Up};
        use PortSide::{East, North, South, West};
        for (direction, across) in [
            (Left, [North, South]),
            (Right, [North, South]),
            (Up, [East, West]),
            (Down, [East, West]),
        ] {
            for side in [North, East, South, West] {
                assert_eq!(
                    is_across_flow(direction, side),
                    across.contains(&side),
                    "{direction:?} {side:?}"
                );
            }
        }
        // No flow, so nothing lies across it. A port without a side lies across no flow either.
        for side in [North, East, South, West, PortSide::Undefined] {
            assert!(!is_across_flow(Undefined, side), "{side:?}");
        }
        for direction in [Left, Right, Up, Down] {
            assert!(!is_across_flow(direction, PortSide::Undefined));
        }
    }

    #[test]
    fn external_port_dummy_uses_direction_and_net_flow_for_free_ports() {
        let dummy = create_external_port_dummy(
            "external:A",
            "external:A:0",
            PortType::Input,
            PortConstraints::Free,
            PortSide::Undefined,
            1,
            LPoint::default(),
            LSize {
                width: 4.0,
                height: 6.0,
            },
            LSize::default(),
            0.0,
            ElkDirection::Down,
        );

        assert_eq!(dummy.kind, LNodeKind::ExternalPort);
        assert_eq!(dummy.in_layer_constraint, InLayerConstraint::Bottom);
        assert_eq!(dummy.port_constraints, PortConstraints::FixedPos);
        assert_eq!(dummy.ports[PortId::new(0)].side, PortSide::North);
        assert_eq!(
            dummy.ports[PortId::new(0)].position,
            LPoint { x: 2.0, y: 0.0 }
        );
    }

    #[test]
    fn external_port_dummy_keeps_fixed_side_and_order_metadata() {
        let dummy = create_external_port_dummy(
            "external:A",
            "external:A:0",
            PortType::Output,
            PortConstraints::FixedRatio,
            PortSide::West,
            -1,
            LPoint { x: 0.0, y: 40.0 },
            LSize {
                width: 4.0,
                height: 6.0,
            },
            LSize {
                width: 20.0,
                height: 80.0,
            },
            -3.0,
            ElkDirection::Right,
        );

        assert_eq!(dummy.layer_constraint, LayerConstraint::FirstSeparate);
        assert_eq!(dummy.size.width, 3.0);
        assert_eq!(dummy.size.height, 6.0);
        assert_eq!(dummy.ports[PortId::new(0)].side, PortSide::East);
        assert_eq!(
            dummy.ports[PortId::new(0)].position,
            LPoint { x: 0.0, y: 3.0 }
        );
        assert_eq!(
            dummy.ports[PortId::new(0)].anchor,
            LPoint { x: 0.0, y: 0.0 }
        );
        assert_eq!(dummy.ports[PortId::new(0)].size, LSize::default());
        assert_eq!(
            dummy.external_port_size,
            LSize {
                width: 4.0,
                height: 6.0
            }
        );
        assert_eq!(dummy.ports[PortId::new(0)].border_offset, Some(-3.0));
        assert_eq!(dummy.port_ratio_or_position, 0.5);
    }

    // Mutation: look a port up by its place in the node's list instead of by its slot.
    #[test]
    fn reordering_ports_leaves_the_origin_of_a_nested_external_dummy_alone() {
        let mut parent = LGraph::new("root", LayeredOptions::default());
        let mut compound = LNode::new("cluster", 10.0, 10.0, Some(0));
        compound.ports.push(LPort::new("p0", 0, PortType::Output));
        compound.ports.push(LPort::new("p1", 0, PortType::Output));

        let mut nested = LGraph::new("cluster", LayeredOptions::default());
        let mut dummy = LNode::new("external:cluster", 0.0, 0.0, None);
        dummy.kind = LNodeKind::ExternalPort;
        let origin = GraphPortRef {
            graph_id: GraphId::ROOT,
            port: PortRef {
                node: 0,
                port: PortId::new(1),
            },
        };
        dummy.origin_port = Some(origin.clone());
        nested.layerless_nodes.push(dummy);
        compound.nested_graph = Some(Box::new(nested));
        parent.layerless_nodes.push(compound);

        assert!(parent.reorder_node_ports(0, port_ids([1, 0])));

        let nested = parent.layerless_nodes[0].nested_graph.as_ref().unwrap();
        assert_eq!(nested.layerless_nodes[0].origin_port, Some(origin));
        // The origin still names the port "p1", which now comes first.
        let compound = &parent.layerless_nodes[0];
        assert_eq!(compound.ports[PortId::new(1)].id, "p1");
        assert_eq!(compound.ports.position(PortId::new(1)), Some(0));
    }

    // Mutation: keep the ports of every side, or list them in reverse.
    #[test]
    fn ports_on_side_lists_the_ports_of_one_side_in_list_order() {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        graph
            .layerless_nodes
            .push(LNode::new("A", 10.0, 10.0, Some(0)));
        for side in [
            PortSide::North,
            PortSide::East,
            PortSide::North,
            PortSide::South,
        ] {
            graph
                .add_port(0, PortType::Output, side, LPoint::default())
                .unwrap();
        }

        assert_eq!(
            graph.ports_on_side(0, PortSide::North),
            [
                PortRef {
                    node: 0,
                    port: PortId::new(0)
                },
                PortRef {
                    node: 0,
                    port: PortId::new(2)
                }
            ]
        );
        assert_eq!(
            graph.ports_on_side(0, PortSide::South),
            [PortRef {
                node: 0,
                port: PortId::new(3)
            }]
        );
        assert!(graph.ports_on_side(0, PortSide::West).is_empty());
    }

    // Mutation: return `true` without rewriting the node's list.
    #[test]
    fn reorder_node_ports_puts_the_ports_in_the_given_order() {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        let mut node = LNode::new("A", 10.0, 10.0, Some(0));
        node.ports.push(LPort::new("p0", 0, PortType::Output));
        node.ports.push(LPort::new("p1", 0, PortType::Output));
        node.ports.push(LPort::new("p2", 0, PortType::Output));
        graph.layerless_nodes.push(node);

        assert!(graph.reorder_node_ports(0, port_ids([2, 0, 1])));

        let ports = &graph.layerless_nodes[0].ports;
        assert_eq!(
            ports
                .iter()
                .map(|port| port.id.as_str())
                .collect::<Vec<_>>(),
            vec!["p2", "p0", "p1"]
        );
        assert_eq!(ports.ids().collect::<Vec<_>>(), port_ids([2, 0, 1]));
    }

    // Mutation: accept an order that leaves out a port, repeats one or names one the node lacks.
    #[test]
    fn reorder_node_ports_refuses_an_order_that_is_not_a_permutation() {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        let mut node = LNode::new("A", 10.0, 10.0, Some(0));
        for id in ["p0", "p1", "p2"] {
            node.ports.push(LPort::new(id, 0, PortType::Output));
        }
        graph.layerless_nodes.push(node);
        let before = graph.layerless_nodes[0].ports.clone();

        assert!(!graph.reorder_node_ports(0, port_ids([0, 1])));
        assert!(!graph.reorder_node_ports(0, port_ids([0, 1, 1])));
        assert!(!graph.reorder_node_ports(0, port_ids([0, 1, 3])));
        assert!(!graph.reorder_node_ports(0, port_ids([0, 1, 2, 2])));
        assert!(!graph.reorder_node_ports(7, port_ids([0, 1, 2])));
        assert_eq!(graph.layerless_nodes[0].ports, before);
    }

    // Mutation: look a port up by its place in the node's list instead of by its slot. The edges
    // then name the port that stands in the old place.
    #[test]
    fn reordering_ports_changes_no_edge_and_no_other_reference() {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        graph
            .layerless_nodes
            .push(LNode::new("A", 10.0, 10.0, Some(0)));
        graph
            .layerless_nodes
            .push(LNode::new("B", 10.0, 10.0, Some(1)));
        let sides = [PortSide::East, PortSide::North, PortSide::South];
        let a = sides.map(|side| {
            graph
                .add_port(0, PortType::Output, side, LPoint::default())
                .unwrap()
        });
        let b = graph
            .add_port(1, PortType::Input, PortSide::West, LPoint::default())
            .unwrap();
        let edge = |id: &str, source: PortRef, target: PortRef| LayeredEdge {
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
            thickness: 0.0,
            original_opposite_port: Some(a[1]),
            compound_segment: None,
        };
        graph.add_edge(edge("e0", a[0], b));
        graph.add_edge(edge("e1", a[2], b));
        graph.add_edge(edge("e2", a[1], a[2]));
        graph.layerless_nodes[1].long_edge_source = Some(a[0]);
        graph.layerless_nodes[1].long_edge_target = Some(a[2]);
        graph.layerless_nodes[1].origin_port = Some(GraphPortRef {
            graph_id: GraphId::ROOT,
            port: a[1],
        });
        let edges_before = graph.edges.clone();
        let nodes_before = graph.layerless_nodes[1].clone();

        assert!(graph.reorder_node_ports(0, [a[2].port, a[0].port, a[1].port]));

        // No edge and no reference of the other node changed.
        assert_eq!(graph.edges, edges_before);
        assert_eq!(graph.layerless_nodes[1], nodes_before);
        // Each port still holds the edges it held, under the same id.
        for (port, outgoing) in [(a[0], "e0"), (a[2], "e1"), (a[1], "e2")] {
            let data = &graph.layerless_nodes[0].ports[port.port];
            assert_eq!(data.outgoing_edges.len(), 1, "{outgoing}");
            assert_eq!(graph.edges[data.outgoing_edges[0]].id, outgoing);
        }
        // Only the list changed: the sides follow the ports, not the places.
        let listed = graph.layerless_nodes[0]
            .ports
            .iter()
            .map(|port| port.side)
            .collect::<Vec<_>>();
        assert_eq!(listed, [PortSide::South, PortSide::East, PortSide::North]);
    }
}
