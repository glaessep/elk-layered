//! ELK layered option model.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/Layered.melk
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.core/src/org/eclipse/elk/core/Core.melk
//! - https://github.com/mermaid-js/mermaid/blob/41646dfd43ac83f001b03c70605feb036afae46d/packages/mermaid-layout-elk/src/render.ts

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ElkDirection {
    Left,
    #[default]
    Right,
    Up,
    Down,
    Undefined,
}

impl ElkDirection {
    pub fn is_vertical(self) -> bool {
        matches!(self, Self::Up | Self::Down)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DirectionCongruency {
    #[default]
    ReadingDirection,
    Rotation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HierarchyHandling {
    Inherit,
    IncludeChildren,
    #[default]
    SeparateChildren,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EdgeRouting {
    Polyline,
    #[default]
    Orthogonal,
    Splines,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CycleBreakingStrategy {
    #[default]
    Greedy,
    DepthFirst,
    Interactive,
    ModelOrder,
    GreedyModelOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayeringStrategy {
    #[default]
    NetworkSimplex,
    LongestPath,
    LongestPathSource,
    CoffmanGraham,
    Interactive,
    StretchWidth,
    MinWidth,
    BreadthFirstModelOrder,
    DepthFirstModelOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrossingMinimizationStrategy {
    #[default]
    LayerSweep,
    Interactive,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NodePlacementStrategy {
    Simple,
    Interactive,
    /// ELK's linear segments placer is not ported. Selecting it makes the pipeline fail with
    /// `PipelineError::UnsupportedProcessor`.
    LinearSegments,
    #[default]
    BrandesKoepf,
    NetworkSimplex,
}

/// Node-placement flexibility reachable through Mermaid's container options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NodeFlexibility {
    #[default]
    None,
    PortPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FixedAlignment {
    #[default]
    None,
    LeftUp,
    RightUp,
    LeftDown,
    RightDown,
    Balanced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Alignment {
    #[default]
    Automatic,
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EdgeStraighteningStrategy {
    None,
    #[default]
    ImproveStraightness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GreedySwitchType {
    Off,
    OneSided,
    #[default]
    TwoSided,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OrderingStrategy {
    #[default]
    None,
    NodesAndEdges,
    PreferEdges,
    PreferNodes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PortSortingStrategy {
    #[default]
    InputOrder,
    PortDegree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LongEdgeOrderingStrategy {
    #[default]
    DummyNodeOver,
    DummyNodeUnder,
    Equal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WrappingStrategy {
    #[default]
    Off,
    SingleEdge,
    MultiEdge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelfLoopDistributionStrategy {
    #[default]
    North,
    Equally,
    NorthSouth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelfLoopOrderingStrategy {
    #[default]
    Stacked,
    ReverseStacked,
    Sequenced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PortConstraints {
    #[default]
    Undefined,
    Free,
    FixedSide,
    FixedOrder,
    FixedRatio,
    FixedPos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PortAlignment {
    #[default]
    Distributed,
    Justified,
    Begin,
    Center,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayerConstraint {
    #[default]
    None,
    First,
    FirstSeparate,
    Last,
    LastSeparate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EdgeLabelSideSelection {
    AlwaysUp,
    AlwaysDown,
    DirectionUp,
    DirectionDown,
    SmartUp,
    #[default]
    SmartDown,
}

impl EdgeLabelSideSelection {
    pub fn transpose(self) -> Self {
        match self {
            Self::AlwaysUp => Self::AlwaysDown,
            Self::AlwaysDown => Self::AlwaysUp,
            Self::DirectionUp => Self::DirectionDown,
            Self::DirectionDown => Self::DirectionUp,
            Self::SmartUp => Self::SmartDown,
            Self::SmartDown => Self::SmartUp,
        }
    }
}

/// Where a node label goes: one cell of ELK's node label grid, or `Fixed` when ELK leaves the
/// label where the input put it.
///
/// Source: `org.eclipse.elk.alg.common.nodespacing.internal.NodeLabelLocation`. `Fixed` stands
/// for ELK's `UNDEFINED`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NodeLabelLocation {
    #[default]
    Fixed,
    InsideTopLeft,
    InsideTopCenter,
    InsideTopRight,
    InsideCenterLeft,
    InsideCenter,
    InsideCenterRight,
    InsideBottomLeft,
    InsideBottomCenter,
    InsideBottomRight,
    OutsideTopLeft,
    OutsideTopCenter,
    OutsideTopRight,
    OutsideBottomLeft,
    OutsideBottomCenter,
    OutsideBottomRight,
    OutsideLeftTop,
    OutsideLeftCenter,
    OutsideLeftBottom,
    OutsideRightTop,
    OutsideRightCenter,
    OutsideRightBottom,
}

impl NodeLabelLocation {
    /// Whether the location lies inside the node.
    ///
    /// Source: `NodeLabelLocation.isInsideLocation`.
    pub(crate) fn is_inside(self) -> bool {
        matches!(
            self,
            Self::InsideTopLeft
                | Self::InsideTopCenter
                | Self::InsideTopRight
                | Self::InsideCenterLeft
                | Self::InsideCenter
                | Self::InsideCenterRight
                | Self::InsideBottomLeft
                | Self::InsideBottomCenter
                | Self::InsideBottomRight
        )
    }
}

/// One value of ELK's `NodeLabelPlacement` enumeration.
///
/// Source: `org.eclipse.elk.core.options.NodeLabelPlacement`. The order is ELK's ordinal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NodeLabelPlacementFlag {
    HLeft,
    HCenter,
    HRight,
    VTop,
    VCenter,
    VBottom,
    Inside,
    Outside,
    HPriority,
}

impl NodeLabelPlacementFlag {
    pub(crate) const ALL: [Self; 9] = [
        Self::HLeft,
        Self::HCenter,
        Self::HRight,
        Self::VTop,
        Self::VCenter,
        Self::VBottom,
        Self::Inside,
        Self::Outside,
        Self::HPriority,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::HLeft => "H_LEFT",
            Self::HCenter => "H_CENTER",
            Self::HRight => "H_RIGHT",
            Self::VTop => "V_TOP",
            Self::VCenter => "V_CENTER",
            Self::VBottom => "V_BOTTOM",
            Self::Inside => "INSIDE",
            Self::Outside => "OUTSIDE",
            Self::HPriority => "H_PRIORITY",
        }
    }

    const fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

/// ELK's `EnumSet<NodeLabelPlacement>`: where node labels go. The empty set is ELK's `fixed()`,
/// which leaves labels where the input put them.
///
/// ELK keeps the set, not the location it maps to, because mirroring and transposing a layout
/// direction work on the set. A set that maps to no location can map to one once transposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct NodeLabelPlacement(u16);

impl NodeLabelPlacement {
    /// The empty set: labels stay where the input put them.
    pub const FIXED: Self = Self(0);

    pub(crate) const fn of(flags: &[NodeLabelPlacementFlag]) -> Self {
        let mut bits = 0;
        let mut i = 0;
        while i < flags.len() {
            bits |= flags[i].bit();
            i += 1;
        }
        Self(bits)
    }

    pub(crate) fn contains(self, flag: NodeLabelPlacementFlag) -> bool {
        self.0 & flag.bit() != 0
    }

    pub(crate) fn insert(&mut self, flag: NodeLabelPlacementFlag) {
        self.0 |= flag.bit();
    }

    pub(crate) fn remove(&mut self, flag: NodeLabelPlacementFlag) {
        self.0 &= !flag.bit();
    }

    /// Whether the set is empty, which ELK calls fixed.
    pub fn is_fixed(self) -> bool {
        self.0 == 0
    }

    /// Source: `NodeLabelPlacement.isValid`: at most one of inside and outside, one horizontal
    /// and one vertical value.
    pub(crate) fn is_valid(self) -> bool {
        use NodeLabelPlacementFlag::*;
        let count = |flags: [NodeLabelPlacementFlag; 3]| {
            flags.iter().filter(|flag| self.contains(**flag)).count()
        };
        (u16::from(self.contains(Inside)) + u16::from(self.contains(Outside))) <= 1
            && count([HLeft, HCenter, HRight]) <= 1
            && count([VTop, VCenter, VBottom]) <= 1
    }

    /// The location the set maps to.
    ///
    /// Source: `NodeLabelLocation.fromNodeLabelPlacement`, which compares the whole set with
    /// the sets each location is assigned.
    pub fn location(self) -> NodeLabelLocation {
        NODE_LABEL_LOCATIONS
            .iter()
            .find(|(_, sets)| sets.contains(&self))
            .map_or(NodeLabelLocation::Fixed, |(location, _)| *location)
    }

    /// The first set ELK assigns to a location; `Fixed` gives the empty set.
    #[cfg(test)]
    pub fn from_location(location: NodeLabelLocation) -> Self {
        NODE_LABEL_LOCATIONS
            .iter()
            .find(|(candidate, _)| *candidate == location)
            .map_or(Self::FIXED, |(_, sets)| sets[0])
    }

    /// Source: `GraphTransformer.mirrorNodeLabelPlacementX`.
    pub(crate) fn mirror_x(mut self) -> Self {
        use NodeLabelPlacementFlag::*;
        if self.contains(HLeft) {
            self.remove(HLeft);
            self.insert(HRight);
        } else if self.contains(HRight) {
            self.remove(HRight);
            self.insert(HLeft);
        }
        self
    }

    /// Source: `GraphTransformer.mirrorNodeLabelPlacementY`.
    pub(crate) fn mirror_y(mut self) -> Self {
        use NodeLabelPlacementFlag::*;
        if self.contains(VTop) {
            self.remove(VTop);
            self.insert(VBottom);
        } else if self.contains(VBottom) {
            self.remove(VBottom);
            self.insert(VTop);
        }
        self
    }

    /// Source: `GraphTransformer.transposeNodeLabelPlacement`.
    pub(crate) fn transpose(self) -> Self {
        use NodeLabelPlacementFlag::*;
        if self.is_fixed() {
            return self;
        }
        let mut new = Self::FIXED;
        if self.contains(Inside) {
            new.insert(Inside);
        } else {
            new.insert(Outside);
        }
        if !self.contains(HPriority) {
            new.insert(HPriority);
        }
        if self.contains(HLeft) {
            new.insert(VTop);
        } else if self.contains(HCenter) {
            new.insert(VCenter);
        } else if self.contains(HRight) {
            new.insert(VBottom);
        }
        if self.contains(VTop) {
            new.insert(HLeft);
        } else if self.contains(VCenter) {
            new.insert(HCenter);
        } else if self.contains(VBottom) {
            new.insert(HRight);
        }
        new
    }
}

/// Each location with the sets ELK assigns to it, in ELK's declaration order.
///
/// Source: `NodeLabelLocation`'s constructor arguments.
const NODE_LABEL_LOCATIONS: [(NodeLabelLocation, &[NodeLabelPlacement]); 21] = {
    use NodeLabelLocation as L;
    use NodeLabelPlacementFlag::*;
    const fn set(flags: &[NodeLabelPlacementFlag]) -> NodeLabelPlacement {
        NodeLabelPlacement::of(flags)
    }
    [
        (L::OutsideTopLeft, &[set(&[Outside, VTop, HLeft])]),
        (
            L::OutsideTopCenter,
            &[
                set(&[Outside, VTop, HCenter]),
                set(&[Outside, VTop, HCenter, HPriority]),
            ],
        ),
        (L::OutsideTopRight, &[set(&[Outside, VTop, HRight])]),
        (L::OutsideBottomLeft, &[set(&[Outside, VBottom, HLeft])]),
        (
            L::OutsideBottomCenter,
            &[
                set(&[Outside, VBottom, HCenter]),
                set(&[Outside, VBottom, HCenter, HPriority]),
            ],
        ),
        (L::OutsideBottomRight, &[set(&[Outside, VBottom, HRight])]),
        (
            L::OutsideLeftTop,
            &[set(&[Outside, HLeft, VTop, HPriority])],
        ),
        (
            L::OutsideLeftCenter,
            &[
                set(&[Outside, HLeft, VCenter]),
                set(&[Outside, HLeft, VCenter, HPriority]),
            ],
        ),
        (
            L::OutsideLeftBottom,
            &[set(&[Outside, HLeft, VBottom, HPriority])],
        ),
        (
            L::OutsideRightTop,
            &[set(&[Outside, HRight, VTop, HPriority])],
        ),
        (
            L::OutsideRightCenter,
            &[
                set(&[Outside, HRight, VCenter]),
                set(&[Outside, HRight, VCenter, HPriority]),
            ],
        ),
        (
            L::OutsideRightBottom,
            &[set(&[Outside, HRight, VBottom, HPriority])],
        ),
        (
            L::InsideTopLeft,
            &[
                set(&[Inside, VTop, HLeft]),
                set(&[Inside, VTop, HLeft, HPriority]),
            ],
        ),
        (
            L::InsideTopCenter,
            &[
                set(&[Inside, VTop, HCenter]),
                set(&[Inside, VTop, HCenter, HPriority]),
            ],
        ),
        (
            L::InsideTopRight,
            &[
                set(&[Inside, VTop, HRight]),
                set(&[Inside, VTop, HRight, HPriority]),
            ],
        ),
        (
            L::InsideCenterLeft,
            &[
                set(&[Inside, VCenter, HLeft]),
                set(&[Inside, VCenter, HLeft, HPriority]),
            ],
        ),
        (
            L::InsideCenter,
            &[
                set(&[Inside, VCenter, HCenter]),
                set(&[Inside, VCenter, HCenter, HPriority]),
            ],
        ),
        (
            L::InsideCenterRight,
            &[
                set(&[Inside, VCenter, HRight]),
                set(&[Inside, VCenter, HRight, HPriority]),
            ],
        ),
        (
            L::InsideBottomLeft,
            &[
                set(&[Inside, VBottom, HLeft]),
                set(&[Inside, VBottom, HLeft, HPriority]),
            ],
        ),
        (
            L::InsideBottomCenter,
            &[
                set(&[Inside, VBottom, HCenter]),
                set(&[Inside, VBottom, HCenter, HPriority]),
            ],
        ),
        (
            L::InsideBottomRight,
            &[
                set(&[Inside, VBottom, HRight]),
                set(&[Inside, VBottom, HRight, HPriority]),
            ],
        ),
    ]
};

/// ELK's `EnumSet<SizeConstraint>`: what a node's size must make room for. The empty set is
/// ELK's `fixed()`.
///
/// Source: `org.eclipse.elk.core.options.SizeConstraint`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct SizeConstraints(u8);

impl SizeConstraints {
    /// The empty set: the size stays as given.
    pub const FIXED: Self = Self(0);
    /// `PORTS`.
    pub const PORTS: Self = Self(1);
    /// `PORT_LABELS`.
    pub const PORT_LABELS: Self = Self(1 << 1);
    /// `NODE_LABELS`.
    pub const NODE_LABELS: Self = Self(1 << 2);
    /// `MINIMUM_SIZE`.
    pub const MINIMUM_SIZE: Self = Self(1 << 3);

    pub(crate) const NAMES: [(&'static str, Self); 4] = [
        ("PORTS", Self::PORTS),
        ("PORT_LABELS", Self::PORT_LABELS),
        ("NODE_LABELS", Self::NODE_LABELS),
        ("MINIMUM_SIZE", Self::MINIMUM_SIZE),
    ];

    /// Whether every value of `other` is in the set.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether the set is empty.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub(crate) fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

impl std::ops::BitOr for SizeConstraints {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// ELK's `EnumSet<SizeOptions>`: how a node's size is computed.
///
/// Source: `org.eclipse.elk.core.options.SizeOptions`. ELK's default is
/// `DEFAULT_MINIMUM_SIZE` alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SizeOptions(u16);

impl Default for SizeOptions {
    fn default() -> Self {
        Self::DEFAULT_MINIMUM_SIZE
    }
}

impl SizeOptions {
    /// The empty set.
    pub const NONE: Self = Self(0);
    /// `DEFAULT_MINIMUM_SIZE`.
    pub const DEFAULT_MINIMUM_SIZE: Self = Self(1);
    /// `MINIMUM_SIZE_ACCOUNTS_FOR_PADDING`.
    pub const MINIMUM_SIZE_ACCOUNTS_FOR_PADDING: Self = Self(1 << 1);
    /// `COMPUTE_PADDING`.
    pub const COMPUTE_PADDING: Self = Self(1 << 2);
    /// `OUTSIDE_NODE_LABELS_OVERHANG`.
    pub const OUTSIDE_NODE_LABELS_OVERHANG: Self = Self(1 << 3);
    /// `PORTS_OVERHANG`.
    pub const PORTS_OVERHANG: Self = Self(1 << 4);
    /// `UNIFORM_PORT_SPACING`.
    pub const UNIFORM_PORT_SPACING: Self = Self(1 << 5);
    /// `SPACE_EFFICIENT_PORT_LABELS`.
    pub const SPACE_EFFICIENT_PORT_LABELS: Self = Self(1 << 6);
    /// `FORCE_TABULAR_NODE_LABELS`.
    pub const FORCE_TABULAR_NODE_LABELS: Self = Self(1 << 7);
    /// `ASYMMETRICAL`.
    pub const ASYMMETRICAL: Self = Self(1 << 8);

    pub(crate) const NAMES: [(&'static str, Self); 9] = [
        ("DEFAULT_MINIMUM_SIZE", Self::DEFAULT_MINIMUM_SIZE),
        (
            "MINIMUM_SIZE_ACCOUNTS_FOR_PADDING",
            Self::MINIMUM_SIZE_ACCOUNTS_FOR_PADDING,
        ),
        ("COMPUTE_PADDING", Self::COMPUTE_PADDING),
        (
            "OUTSIDE_NODE_LABELS_OVERHANG",
            Self::OUTSIDE_NODE_LABELS_OVERHANG,
        ),
        ("PORTS_OVERHANG", Self::PORTS_OVERHANG),
        ("UNIFORM_PORT_SPACING", Self::UNIFORM_PORT_SPACING),
        (
            "SPACE_EFFICIENT_PORT_LABELS",
            Self::SPACE_EFFICIENT_PORT_LABELS,
        ),
        ("FORCE_TABULAR_NODE_LABELS", Self::FORCE_TABULAR_NODE_LABELS),
        ("ASYMMETRICAL", Self::ASYMMETRICAL),
    ];

    /// Whether every value of `other` is in the set.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub(crate) fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

/// The three node size options ELK keeps on a node and on the graph inside it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct NodeSizing {
    /// `org.eclipse.elk.nodeSize.constraints`.
    pub constraints: SizeConstraints,
    /// `org.eclipse.elk.nodeSize.options`.
    pub options: SizeOptions,
    /// `org.eclipse.elk.nodeSize.minimum`.
    pub minimum: crate::graph::LSize,
}

impl NodeSizing {
    /// The `MINIMUM_SIZE` constraint with the given minimum and ELK's default size options.
    #[cfg(test)]
    pub fn with_minimum(minimum: crate::graph::LSize) -> Self {
        Self {
            constraints: SizeConstraints::MINIMUM_SIZE,
            options: SizeOptions::default(),
            minimum,
        }
    }

    /// The minimum size ELK enforces, or `None` without the `MINIMUM_SIZE` constraint.
    ///
    /// Source: `ElkUtil.effectiveMinSizeConstraintFor` and the matching code in
    /// `ElkLayered.resizeGraph` and `HierarchicalNodeResizingProcessor.resizeGraph`.
    pub(crate) fn effective_minimum(&self) -> Option<crate::graph::LSize> {
        if !self.constraints.contains(SizeConstraints::MINIMUM_SIZE) {
            return None;
        }
        let mut minimum = self.minimum;
        if self.options.contains(SizeOptions::DEFAULT_MINIMUM_SIZE) {
            if minimum.width <= 0.0 {
                minimum.width = 20.0;
            }
            if minimum.height <= 0.0 {
                minimum.height = 20.0;
            }
        }
        Some(minimum)
    }
}

/// Alignment of graph contents when a minimum size leaves additional space on an axis.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ContentAlignment {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("ELK node size minimum must have finite dimensions")]
pub struct NodeSizeError;

#[derive(Debug, Clone, PartialEq)]
pub struct LayeredOptions {
    pub direction: ElkDirection,
    pub direction_congruency: DirectionCongruency,
    pub hierarchy_handling: HierarchyHandling,
    pub separate_connected_components: bool,
    pub aspect_ratio: f64,
    pub port_labels_placement_next_to_port_if_possible: bool,
    pub port_labels_treat_as_group: bool,
    pub inside_self_loops_activate: bool,
    pub edge_routing: EdgeRouting,
    pub padding: ElkPadding,
    pub node_labels_padding: ElkPadding,
    /// The node size options of the node this graph belongs to. ELK copies a node's options to
    /// the graph inside it, which then resizes by them.
    pub node_size: NodeSizing,
    pub horizontal_content_alignment: ContentAlignment,
    pub vertical_content_alignment: ContentAlignment,
    pub spacing: SpacingOptions,
    /// `org.eclipse.elk.layered.spacing.baseValue`, applied when the graph is imported.
    pub spacing_base_value: Option<f64>,
    /// The spacings set explicitly, which the base value does not change.
    pub explicit_spacings: ExplicitSpacings,
    pub cycle_breaking_strategy: CycleBreakingStrategy,
    pub layering_strategy: LayeringStrategy,
    pub layering_coffman_graham_layer_bound: i32,
    pub layering_min_width_upper_bound: i32,
    pub layering_min_width_upper_layer_estimation_scaling_factor: i32,
    pub crossing_minimization_strategy: CrossingMinimizationStrategy,
    pub node_placement_strategy: NodePlacementStrategy,
    pub hierarchical_sweepiness: f64,
    pub greedy_switch_type: GreedySwitchType,
    pub greedy_switch_hierarchical_type: GreedySwitchType,
    pub greedy_switch_activation_threshold: usize,
    pub consider_model_order_strategy: OrderingStrategy,
    pub consider_model_order_long_edge_strategy: LongEdgeOrderingStrategy,
    pub consider_model_order_port_model_order: bool,
    pub consider_model_order_crossing_counter_node_influence: f64,
    pub consider_model_order_crossing_counter_port_influence: f64,
    pub force_node_model_order: bool,
    pub port_sorting_strategy: PortSortingStrategy,
    pub port_constraints: PortConstraints,
    pub port_alignment_default: PortAlignment,
    pub merge_edges: bool,
    pub merge_hierarchy_edges: bool,
    pub unnecessary_bendpoints: bool,
    pub wrapping_strategy: WrappingStrategy,
    pub wrapping_multi_edge_improve_cuts: bool,
    pub wrapping_multi_edge_improve_wrapped_edges: bool,
    pub feedback_edges: bool,
    pub random_seed: i32,
    pub thoroughness: usize,
    pub node_placement_favor_straight_edges: Option<bool>,
    pub node_placement_bk_fixed_alignment: FixedAlignment,
    pub node_placement_bk_edge_straightening: EdgeStraighteningStrategy,
    pub graph_has_self_loops: bool,
    pub graph_has_center_labels: bool,
    pub graph_has_end_labels: bool,
    pub edge_label_side_selection: EdgeLabelSideSelection,
    pub graph_has_non_free_ports: bool,
    pub graph_has_north_south_ports: bool,
    pub graph_has_hyperedges: bool,
    pub graph_has_external_ports: bool,
    pub graph_has_hypernodes: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ElkPadding {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

impl ElkPadding {
    pub fn uniform(value: f64) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }
}

impl Default for ElkPadding {
    fn default() -> Self {
        Self::uniform(12.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpacingOptions {
    pub component_component: f64,
    pub node_node: f64,
    pub node_self_loop: f64,
    pub edge_edge: f64,
    pub edge_node: f64,
    pub edge_label: f64,
    pub label_label: f64,
    pub label_node: f64,
    pub label_port_horizontal: f64,
    pub label_port_vertical: f64,
    pub port_port: f64,
    pub edge_edge_between_layers: f64,
    pub edge_node_between_layers: f64,
    pub node_node_between_layers: f64,
    pub ports_surrounding: SpacingMargin,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SpacingMargin {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

impl Default for SpacingOptions {
    fn default() -> Self {
        Self {
            component_component: 20.0,
            node_node: 20.0,
            node_self_loop: 10.0,
            edge_edge: 10.0,
            edge_node: 10.0,
            edge_label: 2.0,
            label_label: 0.0,
            label_node: 5.0,
            label_port_horizontal: 1.0,
            label_port_vertical: 1.0,
            port_port: 10.0,
            edge_edge_between_layers: 10.0,
            edge_node_between_layers: 10.0,
            node_node_between_layers: 20.0,
            ports_surrounding: SpacingMargin::default(),
        }
    }
}

impl SpacingOptions {
    /// Applies Eclipse ELK's `LayeredSpacings.withBaseValue(...)` default factors.
    ///
    /// Source:
    /// https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/options/LayeredSpacings.java
    #[cfg(test)]
    pub fn layered_base_value(base_value: f64) -> Self {
        Self::default().with_base_value(base_value, ExplicitSpacings::NONE)
    }

    /// Sets every spacing the user did not set to its default factor times the base value.
    ///
    /// Source: `ElkSpacings.AbstractSpacingsBuilder.build`, through
    /// `LayeredSpacings.withBaseValue`. ELK skips the whole step when the base value is within
    /// `10e-5` of the node-node default, and computes each value as
    /// `(default / nodeNodeDefault) * base`.
    pub(crate) fn with_base_value(mut self, base: f64, explicit: ExplicitSpacings) -> Self {
        let defaults = Self::default();
        let node_node_default = defaults.node_node;
        if (base - node_node_default).abs() <= 10e-5 {
            return self;
        }
        let scaled = |default: f64| (default / node_node_default) * base;
        let set = |flag: ExplicitSpacings, field: &mut f64, default: f64| {
            if !explicit.contains(flag) {
                *field = scaled(default);
            }
        };
        set(
            ExplicitSpacings::NODE_NODE,
            &mut self.node_node,
            defaults.node_node,
        );
        set(
            ExplicitSpacings::COMPONENT_COMPONENT,
            &mut self.component_component,
            defaults.component_component,
        );
        set(
            ExplicitSpacings::EDGE_EDGE,
            &mut self.edge_edge,
            defaults.edge_edge,
        );
        set(
            ExplicitSpacings::EDGE_LABEL,
            &mut self.edge_label,
            defaults.edge_label,
        );
        set(
            ExplicitSpacings::EDGE_NODE,
            &mut self.edge_node,
            defaults.edge_node,
        );
        set(
            ExplicitSpacings::LABEL_LABEL,
            &mut self.label_label,
            defaults.label_label,
        );
        set(
            ExplicitSpacings::LABEL_NODE,
            &mut self.label_node,
            defaults.label_node,
        );
        set(
            ExplicitSpacings::LABEL_PORT_HORIZONTAL,
            &mut self.label_port_horizontal,
            defaults.label_port_horizontal,
        );
        set(
            ExplicitSpacings::LABEL_PORT_VERTICAL,
            &mut self.label_port_vertical,
            defaults.label_port_vertical,
        );
        set(
            ExplicitSpacings::NODE_SELF_LOOP,
            &mut self.node_self_loop,
            defaults.node_self_loop,
        );
        set(
            ExplicitSpacings::PORT_PORT,
            &mut self.port_port,
            defaults.port_port,
        );
        set(
            ExplicitSpacings::EDGE_EDGE_BETWEEN_LAYERS,
            &mut self.edge_edge_between_layers,
            defaults.edge_edge_between_layers,
        );
        set(
            ExplicitSpacings::EDGE_NODE_BETWEEN_LAYERS,
            &mut self.edge_node_between_layers,
            defaults.edge_node_between_layers,
        );
        set(
            ExplicitSpacings::NODE_NODE_BETWEEN_LAYERS,
            &mut self.node_node_between_layers,
            defaults.node_node_between_layers,
        );
        self
    }
}

/// Which spacing options a graph sets itself. `spacing.baseValue` leaves those alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExplicitSpacings(u16);

impl ExplicitSpacings {
    pub(crate) const NONE: Self = Self(0);
    pub(crate) const NODE_NODE: Self = Self(1);
    pub(crate) const COMPONENT_COMPONENT: Self = Self(1 << 1);
    pub(crate) const EDGE_EDGE: Self = Self(1 << 2);
    pub(crate) const EDGE_LABEL: Self = Self(1 << 3);
    pub(crate) const EDGE_NODE: Self = Self(1 << 4);
    pub(crate) const LABEL_LABEL: Self = Self(1 << 5);
    pub(crate) const LABEL_NODE: Self = Self(1 << 6);
    pub(crate) const LABEL_PORT_HORIZONTAL: Self = Self(1 << 7);
    pub(crate) const LABEL_PORT_VERTICAL: Self = Self(1 << 8);
    pub(crate) const NODE_SELF_LOOP: Self = Self(1 << 9);
    pub(crate) const PORT_PORT: Self = Self(1 << 10);
    pub(crate) const EDGE_EDGE_BETWEEN_LAYERS: Self = Self(1 << 11);
    pub(crate) const EDGE_NODE_BETWEEN_LAYERS: Self = Self(1 << 12);
    pub(crate) const NODE_NODE_BETWEEN_LAYERS: Self = Self(1 << 13);

    pub(crate) fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub(crate) fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

impl LayeredOptions {
    /// The options as ELK's importer leaves them on a new `LGraph`: with
    /// `spacing.baseValue` applied to the spacings the graph does not set itself.
    ///
    /// Source: `ElkGraphImporter.importGraph` and `importHierarchicalGraph`, which apply
    /// `LayeredSpacings.withBaseValue` to each graph that has the option.
    pub(crate) fn with_spacing_base_value_applied(mut self) -> Self {
        if let Some(base) = self.spacing_base_value {
            self.spacing = self.spacing.with_base_value(base, self.explicit_spacings);
        }
        self
    }
}

impl Default for LayeredOptions {
    fn default() -> Self {
        Self {
            direction: ElkDirection::Undefined,
            direction_congruency: DirectionCongruency::ReadingDirection,
            hierarchy_handling: HierarchyHandling::SeparateChildren,
            separate_connected_components: true,
            aspect_ratio: f64::from(1.6_f32),
            port_labels_placement_next_to_port_if_possible: false,
            port_labels_treat_as_group: true,
            inside_self_loops_activate: false,
            edge_routing: EdgeRouting::Orthogonal,
            padding: ElkPadding::default(),
            node_labels_padding: ElkPadding::uniform(5.0),
            node_size: NodeSizing::default(),
            horizontal_content_alignment: ContentAlignment::Start,
            vertical_content_alignment: ContentAlignment::Start,
            spacing: SpacingOptions::default(),
            spacing_base_value: None,
            explicit_spacings: ExplicitSpacings::NONE,
            cycle_breaking_strategy: CycleBreakingStrategy::Greedy,
            layering_strategy: LayeringStrategy::NetworkSimplex,
            layering_coffman_graham_layer_bound: i32::MAX,
            layering_min_width_upper_bound: 4,
            layering_min_width_upper_layer_estimation_scaling_factor: 2,
            crossing_minimization_strategy: CrossingMinimizationStrategy::LayerSweep,
            node_placement_strategy: NodePlacementStrategy::BrandesKoepf,
            hierarchical_sweepiness: 0.1,
            greedy_switch_type: GreedySwitchType::TwoSided,
            greedy_switch_hierarchical_type: GreedySwitchType::Off,
            greedy_switch_activation_threshold: 40,
            consider_model_order_strategy: OrderingStrategy::None,
            consider_model_order_long_edge_strategy: LongEdgeOrderingStrategy::DummyNodeOver,
            consider_model_order_port_model_order: false,
            consider_model_order_crossing_counter_node_influence: 0.0,
            consider_model_order_crossing_counter_port_influence: 0.0,
            force_node_model_order: false,
            port_sorting_strategy: PortSortingStrategy::InputOrder,
            port_constraints: PortConstraints::Undefined,
            port_alignment_default: PortAlignment::Distributed,
            merge_edges: false,
            merge_hierarchy_edges: true,
            unnecessary_bendpoints: false,
            wrapping_strategy: WrappingStrategy::Off,
            wrapping_multi_edge_improve_cuts: true,
            wrapping_multi_edge_improve_wrapped_edges: true,
            feedback_edges: false,
            random_seed: 1,
            thoroughness: 7,
            node_placement_favor_straight_edges: None,
            node_placement_bk_fixed_alignment: FixedAlignment::None,
            node_placement_bk_edge_straightening: EdgeStraighteningStrategy::ImproveStraightness,
            graph_has_self_loops: false,
            graph_has_center_labels: false,
            graph_has_end_labels: false,
            edge_label_side_selection: EdgeLabelSideSelection::SmartDown,
            graph_has_non_free_ports: false,
            graph_has_north_south_ports: false,
            graph_has_hyperedges: false,
            graph_has_external_ports: false,
            graph_has_hypernodes: false,
        }
    }
}

impl LayeredOptions {
    pub fn validate_node_size_minimum(&self) -> Result<(), NodeSizeError> {
        let minimum = self.node_size.minimum;
        if !minimum.width.is_finite() || !minimum.height.is_finite() {
            return Err(NodeSizeError);
        }
        Ok(())
    }

    pub(crate) fn effective_node_size_minimum(&self) -> Option<crate::graph::LSize> {
        self.node_size.effective_minimum()
    }

    /// The option set most pinned elkjs 0.9.3 cases in the tests were captured with. Only tests
    /// need it, so it is not part of the public API.
    #[cfg(test)]
    pub(crate) fn flowchart_test_defaults(direction: ElkDirection) -> Self {
        Self {
            direction,
            hierarchy_handling: HierarchyHandling::IncludeChildren,
            port_labels_placement_next_to_port_if_possible: true,
            port_labels_treat_as_group: true,
            inside_self_loops_activate: false,
            node_placement_strategy: NodePlacementStrategy::BrandesKoepf,
            consider_model_order_strategy: OrderingStrategy::NodesAndEdges,
            spacing: SpacingOptions::layered_base_value(40.0),
            unnecessary_bendpoints: true,
            wrapping_multi_edge_improve_cuts: true,
            wrapping_multi_edge_improve_wrapped_edges: true,
            merge_hierarchy_edges: true,
            ..Self::default()
        }
    }

    pub fn is_hierarchical_layout(&self) -> bool {
        self.hierarchy_handling == HierarchyHandling::IncludeChildren
    }
}

impl LongEdgeOrderingStrategy {
    pub fn return_value(self) -> i64 {
        match self {
            Self::DummyNodeOver => i64::MAX,
            Self::DummyNodeUnder => -1,
            Self::Equal => 0,
        }
    }
}

impl PortConstraints {
    pub fn is_pos_fixed(self) -> bool {
        self == Self::FixedPos
    }

    pub fn is_ratio_fixed(self) -> bool {
        self == Self::FixedRatio
    }

    pub fn is_order_fixed(self) -> bool {
        matches!(self, Self::FixedOrder | Self::FixedRatio | Self::FixedPos)
    }

    pub fn is_side_fixed(self) -> bool {
        !matches!(self, Self::Free | Self::Undefined)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layered_default_padding_is_twelve() {
        let options = LayeredOptions::default();

        assert_eq!(options.padding, ElkPadding::uniform(12.0));
    }

    #[test]
    fn layered_base_spacing_scales_every_spacing_from_the_node_spacing() {
        let options = LayeredOptions {
            spacing: SpacingOptions::layered_base_value(40.0),
            ..LayeredOptions::default()
        };

        assert_eq!(options.spacing.node_node, 40.0);
        assert_eq!(options.spacing.node_self_loop, 20.0);
        assert_eq!(options.spacing.edge_edge, 20.0);
        assert_eq!(options.spacing.edge_node, 20.0);
        assert_eq!(options.spacing.edge_label, 4.0);
        assert_eq!(options.spacing.label_label, 0.0);
        assert_eq!(options.spacing.label_node, 10.0);
        assert_eq!(options.spacing.label_port_horizontal, 2.0);
        assert_eq!(options.spacing.label_port_vertical, 2.0);
        assert_eq!(options.spacing.port_port, 20.0);
        assert_eq!(options.spacing.edge_edge_between_layers, 20.0);
        assert_eq!(options.spacing.edge_node_between_layers, 20.0);
        assert_eq!(options.spacing.node_node_between_layers, 40.0);
    }

    #[test]
    fn edge_label_side_selection_transposes_symmetrically() {
        use EdgeLabelSideSelection::*;

        assert_eq!(AlwaysUp.transpose(), AlwaysDown);
        assert_eq!(AlwaysDown.transpose(), AlwaysUp);
        assert_eq!(DirectionUp.transpose(), DirectionDown);
        assert_eq!(DirectionDown.transpose(), DirectionUp);
        assert_eq!(SmartUp.transpose(), SmartDown);
        assert_eq!(SmartDown.transpose(), SmartUp);
    }
}
