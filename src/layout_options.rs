//! Reads ELK layout options from the strings of a graph's `layoutOptions`.
//!
//! A key names an option the way ELK's JSON importer accepts it: by its full id, such as
//! `org.eclipse.elk.direction`, or by any suffix that starts after a dot, such as
//! `elk.direction`, as long as that suffix names one option only. Values are parsed the way
//! `LayoutOptionData.parseValue` parses them in elkjs 0.9.3.
//!
//! Where elkjs ignores a key it cannot resolve or a value it cannot parse, this module
//! returns an error instead, so a caller never believes an option took effect.
//!
//! Source references:
//! - `org.eclipse.elk.core/src/org/eclipse/elk/core/data/LayoutMetaDataService.java`,
//!   `getOptionDataBySuffix`
//! - `org.eclipse.elk.core/src/org/eclipse/elk/core/data/LayoutOptionData.java`, `parseValue`
//! - `org.eclipse.elk.core/src/org/eclipse/elk/core/math/{Spacing,KVector}.java`, `parse`
//! - `org.eclipse.elk.graph.json/src/org/eclipse/elk/graph/json/JsonImporter.xtend`,
//!   `transformProperties`

use std::collections::BTreeMap;

use crate::graph::{EdgeLabelPlacement, LSize};
use crate::options::{
    ContentAlignment, CrossingMinimizationStrategy, CycleBreakingStrategy, DirectionCongruency,
    EdgeLabelSideSelection, EdgeRouting, EdgeStraighteningStrategy, ElkDirection, ElkPadding,
    ExplicitSpacings, FixedAlignment, GreedySwitchType, HierarchyHandling, LayerConstraint,
    LayeredOptions, LayeringStrategy, LongEdgeOrderingStrategy, NodeFlexibility,
    NodeLabelPlacement, NodeLabelPlacementFlag, NodePlacementStrategy, NodeSizing,
    OrderingStrategy, PortAlignment, PortConstraints, PortSortingStrategy,
    SelfLoopDistributionStrategy, SelfLoopOrderingStrategy, SizeConstraints, SizeOptions,
    SpacingMargin, WrappingStrategy,
};

/// ELK's option targets, as bits.
pub(crate) const PARENTS: u8 = 1;
pub(crate) const NODES: u8 = 1 << 1;
pub(crate) const EDGES: u8 = 1 << 2;
pub(crate) const PORTS: u8 = 1 << 3;
pub(crate) const LABELS: u8 = 1 << 4;

/// Every option elkjs 0.9.3 registers, with its targets, as `knownLayoutOptions()` lists
/// them. A suffix is only unique if it is unique among all of them.
const KNOWN_OPTIONS: &[(&str, u8)] = &[
    ("org.eclipse.elk.algorithm", PARENTS),
    ("org.eclipse.elk.resolvedAlgorithm", PARENTS),
    ("org.eclipse.elk.alignment", NODES),
    ("org.eclipse.elk.aspectRatio", PARENTS),
    ("org.eclipse.elk.bendPoints", EDGES),
    ("org.eclipse.elk.contentAlignment", PARENTS),
    ("org.eclipse.elk.debugMode", PARENTS),
    ("org.eclipse.elk.direction", PARENTS),
    ("org.eclipse.elk.edgeRouting", PARENTS),
    ("org.eclipse.elk.expandNodes", PARENTS),
    ("org.eclipse.elk.hierarchyHandling", PARENTS | NODES),
    ("org.eclipse.elk.padding", PARENTS | NODES),
    ("org.eclipse.elk.interactive", PARENTS),
    ("org.eclipse.elk.interactiveLayout", PARENTS),
    ("org.eclipse.elk.omitNodeMicroLayout", PARENTS),
    ("org.eclipse.elk.portConstraints", NODES),
    ("org.eclipse.elk.position", NODES | PORTS | LABELS),
    ("org.eclipse.elk.priority", NODES | EDGES),
    ("org.eclipse.elk.randomSeed", PARENTS),
    ("org.eclipse.elk.separateConnectedComponents", PARENTS),
    ("org.eclipse.elk.junctionPoints", EDGES),
    ("org.eclipse.elk.commentBox", NODES),
    ("org.eclipse.elk.hypernode", NODES),
    ("org.eclipse.elk.labelManager", PARENTS | LABELS),
    ("org.eclipse.elk.margins", NODES),
    ("org.eclipse.elk.noLayout", NODES | EDGES | PORTS | LABELS),
    ("org.eclipse.elk.scaleFactor", NODES),
    ("org.eclipse.elk.childAreaWidth", PARENTS),
    ("org.eclipse.elk.childAreaHeight", PARENTS),
    ("org.eclipse.elk.topdownLayout", PARENTS),
    ("org.eclipse.elk.animate", PARENTS),
    ("org.eclipse.elk.animTimeFactor", PARENTS),
    ("org.eclipse.elk.layoutAncestors", PARENTS),
    ("org.eclipse.elk.maxAnimTime", PARENTS),
    ("org.eclipse.elk.minAnimTime", PARENTS),
    ("org.eclipse.elk.progressBar", PARENTS),
    ("org.eclipse.elk.validateGraph", PARENTS),
    ("org.eclipse.elk.validateOptions", PARENTS),
    ("org.eclipse.elk.zoomToFit", PARENTS),
    ("org.eclipse.elk.box.packingMode", PARENTS),
    ("org.eclipse.elk.spacing.commentComment", PARENTS),
    ("org.eclipse.elk.spacing.commentNode", PARENTS),
    ("org.eclipse.elk.spacing.componentComponent", PARENTS),
    ("org.eclipse.elk.spacing.edgeEdge", PARENTS),
    ("org.eclipse.elk.spacing.edgeLabel", PARENTS),
    ("org.eclipse.elk.spacing.edgeNode", PARENTS),
    ("org.eclipse.elk.spacing.labelLabel", PARENTS),
    ("org.eclipse.elk.spacing.labelNode", PARENTS),
    ("org.eclipse.elk.spacing.labelPortHorizontal", PARENTS),
    ("org.eclipse.elk.spacing.labelPortVertical", PARENTS),
    ("org.eclipse.elk.spacing.nodeNode", PARENTS),
    ("org.eclipse.elk.spacing.nodeSelfLoop", PARENTS),
    ("org.eclipse.elk.spacing.portPort", PARENTS | NODES),
    (
        "org.eclipse.elk.spacing.individual",
        NODES | EDGES | PORTS | LABELS,
    ),
    ("org.eclipse.elk.spacing.portsSurrounding", PARENTS),
    ("org.eclipse.elk.partitioning.partition", PARENTS | NODES),
    ("org.eclipse.elk.partitioning.activate", PARENTS),
    ("org.eclipse.elk.nodeLabels.padding", PARENTS),
    ("org.eclipse.elk.nodeLabels.placement", NODES | LABELS),
    ("org.eclipse.elk.portAlignment.default", NODES),
    ("org.eclipse.elk.portAlignment.north", NODES),
    ("org.eclipse.elk.portAlignment.south", NODES),
    ("org.eclipse.elk.portAlignment.west", NODES),
    ("org.eclipse.elk.portAlignment.east", NODES),
    ("org.eclipse.elk.nodeSize.constraints", NODES),
    ("org.eclipse.elk.nodeSize.options", NODES),
    ("org.eclipse.elk.nodeSize.minimum", NODES),
    ("org.eclipse.elk.nodeSize.fixedGraphSize", PARENTS),
    ("org.eclipse.elk.edgeLabels.placement", LABELS),
    ("org.eclipse.elk.edgeLabels.inline", LABELS),
    ("org.eclipse.elk.font.name", LABELS),
    ("org.eclipse.elk.font.size", LABELS),
    ("org.eclipse.elk.port.anchor", PORTS),
    ("org.eclipse.elk.port.index", PORTS),
    ("org.eclipse.elk.port.side", PORTS),
    ("org.eclipse.elk.port.borderOffset", PORTS),
    ("org.eclipse.elk.portLabels.placement", NODES),
    ("org.eclipse.elk.portLabels.nextToPortIfPossible", NODES),
    ("org.eclipse.elk.portLabels.treatAsGroup", NODES),
    ("org.eclipse.elk.topdown.scaleFactor", PARENTS),
    ("org.eclipse.elk.topdown.sizeApproximator", NODES),
    (
        "org.eclipse.elk.topdown.hierarchicalNodeWidth",
        PARENTS | NODES,
    ),
    (
        "org.eclipse.elk.topdown.hierarchicalNodeAspectRatio",
        PARENTS | NODES,
    ),
    ("org.eclipse.elk.topdown.nodeType", NODES),
    ("org.eclipse.elk.topdown.scaleCap", PARENTS),
    ("org.eclipse.elk.insideSelfLoops.activate", NODES),
    ("org.eclipse.elk.insideSelfLoops.yo", EDGES),
    ("org.eclipse.elk.edge.thickness", EDGES),
    ("org.eclipse.elk.edge.type", EDGES),
    ("org.eclipse.elk.layered.directionCongruency", PARENTS),
    ("org.eclipse.elk.layered.feedbackEdges", PARENTS),
    ("org.eclipse.elk.layered.interactiveReferencePoint", PARENTS),
    ("org.eclipse.elk.layered.mergeEdges", PARENTS),
    ("org.eclipse.elk.layered.mergeHierarchyEdges", PARENTS),
    (
        "org.eclipse.elk.layered.allowNonFlowPortsToSwitchSides",
        PORTS,
    ),
    ("org.eclipse.elk.layered.portSortingStrategy", PARENTS),
    ("org.eclipse.elk.layered.thoroughness", PARENTS),
    ("org.eclipse.elk.layered.unnecessaryBendpoints", PARENTS),
    (
        "org.eclipse.elk.layered.generatePositionAndLayerIds",
        PARENTS,
    ),
    ("org.eclipse.elk.layered.cycleBreaking.strategy", PARENTS),
    ("org.eclipse.elk.layered.layering.strategy", PARENTS),
    ("org.eclipse.elk.layered.layering.layerConstraint", NODES),
    (
        "org.eclipse.elk.layered.layering.layerChoiceConstraint",
        NODES,
    ),
    ("org.eclipse.elk.layered.layering.layerId", NODES),
    (
        "org.eclipse.elk.layered.layering.minWidth.upperBoundOnWidth",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.layering.minWidth.upperLayerEstimationScalingFactor",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.layering.nodePromotion.strategy",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.layering.nodePromotion.maxIterations",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.layering.coffmanGraham.layerBound",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.crossingMinimization.strategy",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.crossingMinimization.forceNodeModelOrder",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.crossingMinimization.hierarchicalSweepiness",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.crossingMinimization.semiInteractive",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.crossingMinimization.inLayerPredOf",
        NODES,
    ),
    (
        "org.eclipse.elk.layered.crossingMinimization.inLayerSuccOf",
        NODES,
    ),
    (
        "org.eclipse.elk.layered.crossingMinimization.positionChoiceConstraint",
        NODES,
    ),
    (
        "org.eclipse.elk.layered.crossingMinimization.positionId",
        NODES,
    ),
    (
        "org.eclipse.elk.layered.crossingMinimization.greedySwitch.activationThreshold",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.crossingMinimization.greedySwitch.type",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.crossingMinimization.greedySwitchHierarchical.type",
        PARENTS,
    ),
    ("org.eclipse.elk.layered.nodePlacement.strategy", PARENTS),
    (
        "org.eclipse.elk.layered.nodePlacement.favorStraightEdges",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.nodePlacement.bk.edgeStraightening",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.nodePlacement.bk.fixedAlignment",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.nodePlacement.linearSegments.deflectionDampening",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.nodePlacement.networkSimplex.nodeFlexibility",
        NODES,
    ),
    (
        "org.eclipse.elk.layered.nodePlacement.networkSimplex.nodeFlexibility.default",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.edgeRouting.selfLoopDistribution",
        NODES,
    ),
    (
        "org.eclipse.elk.layered.edgeRouting.selfLoopOrdering",
        NODES,
    ),
    ("org.eclipse.elk.layered.edgeRouting.splines.mode", PARENTS),
    (
        "org.eclipse.elk.layered.edgeRouting.splines.sloppy.layerSpacingFactor",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.edgeRouting.polyline.slopedEdgeZoneWidth",
        PARENTS,
    ),
    ("org.eclipse.elk.layered.spacing.baseValue", PARENTS),
    (
        "org.eclipse.elk.layered.spacing.edgeNodeBetweenLayers",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.spacing.edgeEdgeBetweenLayers",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.spacing.nodeNodeBetweenLayers",
        PARENTS,
    ),
    ("org.eclipse.elk.layered.priority.direction", EDGES),
    ("org.eclipse.elk.layered.priority.shortness", EDGES),
    ("org.eclipse.elk.layered.priority.straightness", EDGES),
    (
        "org.eclipse.elk.layered.compaction.connectedComponents",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.compaction.postCompaction.strategy",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.compaction.postCompaction.constraints",
        PARENTS,
    ),
    ("org.eclipse.elk.layered.highDegreeNodes.treatment", PARENTS),
    ("org.eclipse.elk.layered.highDegreeNodes.threshold", PARENTS),
    (
        "org.eclipse.elk.layered.highDegreeNodes.treeHeight",
        PARENTS,
    ),
    ("org.eclipse.elk.layered.wrapping.strategy", PARENTS),
    (
        "org.eclipse.elk.layered.wrapping.additionalEdgeSpacing",
        PARENTS,
    ),
    ("org.eclipse.elk.layered.wrapping.correctionFactor", PARENTS),
    ("org.eclipse.elk.layered.wrapping.cutting.strategy", PARENTS),
    ("org.eclipse.elk.layered.wrapping.cutting.cuts", PARENTS),
    (
        "org.eclipse.elk.layered.wrapping.cutting.msd.freedom",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.wrapping.validify.strategy",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.wrapping.validify.forbiddenIndices",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.wrapping.multiEdge.improveCuts",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.wrapping.multiEdge.distancePenalty",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.wrapping.multiEdge.improveWrappedEdges",
        PARENTS,
    ),
    ("org.eclipse.elk.layered.edgeLabels.sideSelection", PARENTS),
    (
        "org.eclipse.elk.layered.edgeLabels.centerLabelPlacementStrategy",
        PARENTS | LABELS,
    ),
    (
        "org.eclipse.elk.layered.considerModelOrder.strategy",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.considerModelOrder.portModelOrder",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.considerModelOrder.noModelOrder",
        NODES,
    ),
    (
        "org.eclipse.elk.layered.considerModelOrder.components",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.considerModelOrder.longEdgeStrategy",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.considerModelOrder.crossingCounterNodeInfluence",
        PARENTS,
    ),
    (
        "org.eclipse.elk.layered.considerModelOrder.crossingCounterPortInfluence",
        PARENTS,
    ),
    ("org.eclipse.elk.stress.fixed", NODES),
    ("org.eclipse.elk.stress.desiredEdgeLength", PARENTS | EDGES),
    ("org.eclipse.elk.stress.dimension", PARENTS),
    ("org.eclipse.elk.stress.epsilon", PARENTS),
    ("org.eclipse.elk.stress.iterationLimit", PARENTS),
    ("org.eclipse.elk.mrtree.compaction", PARENTS),
    ("org.eclipse.elk.mrtree.edgeEndTextureLength", PARENTS),
    ("org.eclipse.elk.mrtree.treeLevel", NODES),
    ("org.eclipse.elk.mrtree.positionConstraint", NODES),
    ("org.eclipse.elk.mrtree.weighting", PARENTS),
    ("org.eclipse.elk.mrtree.edgeRoutingMode", PARENTS),
    ("org.eclipse.elk.mrtree.searchOrder", PARENTS),
    ("org.eclipse.elk.radial.centerOnRoot", PARENTS),
    ("org.eclipse.elk.radial.orderId", NODES),
    ("org.eclipse.elk.radial.radius", PARENTS),
    ("org.eclipse.elk.radial.rotate", PARENTS),
    ("org.eclipse.elk.radial.compactor", PARENTS),
    ("org.eclipse.elk.radial.compactionStepSize", PARENTS),
    ("org.eclipse.elk.radial.sorter", PARENTS),
    ("org.eclipse.elk.radial.wedgeCriteria", PARENTS),
    ("org.eclipse.elk.radial.optimizationCriteria", PARENTS),
    ("org.eclipse.elk.radial.rotation.targetAngle", PARENTS),
    (
        "org.eclipse.elk.radial.rotation.computeAdditionalWedgeSpace",
        PARENTS,
    ),
    (
        "org.eclipse.elk.radial.rotation.outgoingEdgeAngles",
        PARENTS,
    ),
    ("org.eclipse.elk.force.model", PARENTS),
    ("org.eclipse.elk.force.iterations", PARENTS),
    ("org.eclipse.elk.force.repulsivePower", EDGES),
    ("org.eclipse.elk.force.temperature", PARENTS),
    ("org.eclipse.elk.force.repulsion", PARENTS),
    ("org.eclipse.elk.polyomino.traversalStrategy", PARENTS),
    ("org.eclipse.elk.polyomino.lowLevelSort", PARENTS),
    ("org.eclipse.elk.polyomino.highLevelSort", PARENTS),
    ("org.eclipse.elk.polyomino.fill", PARENTS),
    (
        "org.eclipse.elk.disco.componentCompaction.strategy",
        PARENTS,
    ),
    (
        "org.eclipse.elk.disco.componentCompaction.componentLayoutAlgorithm",
        PARENTS,
    ),
    ("org.eclipse.elk.disco.debug.discoGraph", PARENTS),
    ("org.eclipse.elk.disco.debug.discoPolys", PARENTS),
    ("org.eclipse.elk.underlyingLayoutAlgorithm", PARENTS),
    (
        "org.eclipse.elk.structure.structureExtractionStrategy",
        PARENTS,
    ),
    ("org.eclipse.elk.processingOrder.treeConstruction", PARENTS),
    (
        "org.eclipse.elk.processingOrder.spanningTreeCostFunction",
        PARENTS,
    ),
    ("org.eclipse.elk.processingOrder.preferredRoot", PARENTS),
    ("org.eclipse.elk.processingOrder.rootSelection", PARENTS),
    ("org.eclipse.elk.compaction.compactionStrategy", PARENTS),
    ("org.eclipse.elk.compaction.orthogonal", PARENTS),
    ("org.eclipse.elk.overlapRemoval.maxIterations", PARENTS),
    ("org.eclipse.elk.overlapRemoval.runScanline", PARENTS),
    ("org.eclipse.elk.rectpacking.trybox", PARENTS),
    ("org.eclipse.elk.rectpacking.currentPosition", NODES),
    ("org.eclipse.elk.rectpacking.desiredPosition", NODES),
    ("org.eclipse.elk.rectpacking.inNewRow", NODES),
    (
        "org.eclipse.elk.rectpacking.widthApproximation.strategy",
        PARENTS,
    ),
    (
        "org.eclipse.elk.rectpacking.widthApproximation.targetWidth",
        PARENTS,
    ),
    (
        "org.eclipse.elk.rectpacking.widthApproximation.optimizationGoal",
        PARENTS,
    ),
    (
        "org.eclipse.elk.rectpacking.widthApproximation.lastPlaceShift",
        PARENTS,
    ),
    ("org.eclipse.elk.rectpacking.packing.strategy", PARENTS),
    (
        "org.eclipse.elk.rectpacking.packing.compaction.rowHeightReevaluation",
        PARENTS,
    ),
    (
        "org.eclipse.elk.rectpacking.packing.compaction.iterations",
        PARENTS,
    ),
    (
        "org.eclipse.elk.rectpacking.whiteSpaceElimination.strategy",
        PARENTS,
    ),
];

/// Why a key or value was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OptionProblem {
    /// The key names no ELK option, or more than one.
    Unknown,
    /// The key names an ELK option elk-layered does not support, here or at all.
    Unsupported(&'static str),
    /// The value cannot be parsed.
    InvalidValue,
    /// Two keys name the same option.
    Duplicate { other: String },
}

/// One refused option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OptionError {
    pub(crate) key: String,
    pub(crate) value: String,
    pub(crate) problem: OptionProblem,
}

/// Whether `suffix` is `id` or the end of `id` after a dot, such as `direction` of
/// `org.eclipse.elk.direction`. An empty suffix is none.
///
/// Ports the suffix match of `LayoutMetaDataService.getOptionDataBySuffix` and
/// `getAlgorithmDataBySuffix`.
pub(crate) fn is_id_suffix(id: &str, suffix: &str) -> bool {
    !suffix.is_empty()
        && id.ends_with(suffix)
        && (id.len() == suffix.len() || id.as_bytes()[id.len() - suffix.len() - 1] == b'.')
}

/// Finds the option a key names.
///
/// Ports `LayoutMetaDataService.getOptionDataBySuffix`. ELK's one legacy id is for an internal
/// option, so legacy ids are left out.
pub(crate) fn resolve(key: &str) -> Option<(&'static str, u8)> {
    if key.is_empty() {
        return None;
    }
    let mut found = None;
    for &(id, targets) in KNOWN_OPTIONS {
        if id == key {
            return Some((id, targets));
        }
        if is_id_suffix(id, key) {
            if found.is_some() {
                return None;
            }
            found = Some((id, targets));
        }
    }
    found
}

/// The kind of element an option map belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ElementKind {
    /// A node, the root included. ELK's `PARENTS` and `NODES` targets apply.
    Node,
    Edge,
    Label,
}

impl ElementKind {
    /// Ports `LayoutOptionData.Target` as a bit set.
    fn targets(self) -> u8 {
        match self {
            Self::Node => PARENTS | NODES,
            Self::Edge => EDGES,
            Self::Label => LABELS,
        }
    }
}

/// Resolves every key of an option map, checks it targets this kind of element, and refuses
/// two keys that name the same option. Returns `(option id, key, value)` in key order.
///
/// Ports `JsonImporter.transformProperties`, which resolves each key with `getOptionDataBySuffix`.
fn resolve_all(
    options: &BTreeMap<String, String>,
    kind: ElementKind,
) -> Result<Vec<(&'static str, &str, &str)>, OptionError> {
    let mut resolved: Vec<(&'static str, &str, &str)> = Vec::new();
    for (key, value) in options {
        let error = |problem| OptionError {
            key: key.clone(),
            value: value.clone(),
            problem,
        };
        let Some((id, targets)) = resolve(key) else {
            return Err(error(OptionProblem::Unknown));
        };
        if targets & kind.targets() == 0 {
            return Err(error(OptionProblem::Unsupported(WRONG_TARGET)));
        }
        if let Some((_, other, _)) = resolved.iter().find(|(other_id, _, _)| *other_id == id) {
            // elkjs keeps the key that comes last in the JSON object. A map has no such
            // order, so refuse the pair.
            return Err(error(OptionProblem::Duplicate {
                other: (*other).to_string(),
            }));
        }
        resolved.push((id, key.as_str(), value.as_str()));
    }
    Ok(resolved)
}

/// What a node's `layoutOptions` set: the options of the graph inside it (or of the root
/// graph), and the options ELK reads from the node itself.
#[derive(Debug, Clone)]
pub(crate) struct NodeSettings {
    /// The node's values on top of ELK's defaults. ELK copies a node's options to the graph
    /// inside it, so this is that graph's option set.
    pub(crate) options: LayeredOptions,
    pub(crate) direction: Option<ElkDirection>,
    pub(crate) hierarchy_handling: Option<HierarchyHandling>,
    pub(crate) layer_constraint: Option<LayerConstraint>,
    pub(crate) port_constraints: Option<PortConstraints>,
    pub(crate) port_alignment: Option<PortAlignment>,
    pub(crate) node_label_placement: NodeLabelPlacement,
    pub(crate) node_flexibility: NodeFlexibility,
    pub(crate) node_size: NodeSizing,
    pub(crate) self_loop_distribution: SelfLoopDistributionStrategy,
    pub(crate) self_loop_ordering: SelfLoopOrderingStrategy,
    /// The `algorithm` value, when set.
    pub(crate) algorithm: Option<String>,
}

/// What an edge's `layoutOptions` set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct EdgeSettings {
    pub(crate) priority_direction: i32,
    pub(crate) priority_shortness: i32,
    pub(crate) priority_straightness: i32,
}

/// What a label's `layoutOptions` set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LabelSettings {
    pub(crate) placement: EdgeLabelPlacement,
    pub(crate) inline: bool,
    pub(crate) node_label_placement: Option<NodeLabelPlacement>,
}

impl Default for LabelSettings {
    /// Ports the defaults of `edgeLabels.placement` and `edgeLabels.inline` in `Core.melk`.
    fn default() -> Self {
        Self {
            placement: EdgeLabelPlacement::Center,
            inline: false,
            node_label_placement: None,
        }
    }
}

/// Reads a node's options.
///
/// Ports `JsonImporter.setOption` for a node.
pub(crate) fn node_settings(
    options: &BTreeMap<String, String>,
) -> Result<NodeSettings, OptionError> {
    let mut settings = NodeSettings {
        options: LayeredOptions::default(),
        direction: None,
        hierarchy_handling: None,
        layer_constraint: None,
        port_constraints: None,
        port_alignment: None,
        node_label_placement: NodeLabelPlacement::FIXED,
        node_flexibility: NodeFlexibility::None,
        node_size: NodeSizing::default(),
        self_loop_distribution: SelfLoopDistributionStrategy::North,
        self_loop_ordering: SelfLoopOrderingStrategy::Stacked,
        algorithm: None,
    };
    for (id, key, value) in resolve_all(options, ElementKind::Node)? {
        apply_node_option(&mut settings, id, value).map_err(|problem| OptionError {
            key: key.to_string(),
            value: value.to_string(),
            problem,
        })?;
    }
    settings.options.node_size = settings.node_size;
    Ok(settings)
}

/// Reads an edge's options.
///
/// Ports `JsonImporter.setOption` for an edge.
pub(crate) fn edge_settings(
    options: &BTreeMap<String, String>,
) -> Result<EdgeSettings, OptionError> {
    let mut settings = EdgeSettings::default();
    for (id, key, value) in resolve_all(options, ElementKind::Edge)? {
        let result = match id {
            "org.eclipse.elk.layered.priority.direction" => {
                parse_int(value).map(|v| settings.priority_direction = v)
            }
            "org.eclipse.elk.layered.priority.shortness" => {
                parse_int(value).map(|v| settings.priority_shortness = v)
            }
            "org.eclipse.elk.layered.priority.straightness" => {
                parse_int(value).map(|v| settings.priority_straightness = v)
            }
            "org.eclipse.elk.insideSelfLoops.yo" => {
                parse_bool(value).and_then(|v| refuse_true(v, INSIDE_SELF_LOOPS))
            }
            _ => Err(OptionProblem::Unsupported(NOT_PORTED)),
        };
        result.map_err(|problem| OptionError {
            key: key.to_string(),
            value: value.to_string(),
            problem,
        })?;
    }
    Ok(settings)
}

/// Reads a label's options.
///
/// Ports `JsonImporter.setOption` for a label.
pub(crate) fn label_settings(
    options: &BTreeMap<String, String>,
) -> Result<LabelSettings, OptionError> {
    let mut settings = LabelSettings::default();
    for (id, key, value) in resolve_all(options, ElementKind::Label)? {
        let result = match id {
            "org.eclipse.elk.edgeLabels.placement" => parse_enum(
                value,
                &[
                    ("CENTER", EdgeLabelPlacement::Center),
                    ("HEAD", EdgeLabelPlacement::Head),
                    ("TAIL", EdgeLabelPlacement::Tail),
                ],
            )
            .map(|v| settings.placement = v),
            "org.eclipse.elk.edgeLabels.inline" => parse_bool(value).map(|v| settings.inline = v),
            "org.eclipse.elk.nodeLabels.placement" => {
                parse_node_label_placement(value).map(|v| settings.node_label_placement = Some(v))
            }
            _ => Err(OptionProblem::Unsupported(NOT_PORTED)),
        };
        result.map_err(|problem| OptionError {
            key: key.to_string(),
            value: value.to_string(),
            problem,
        })?;
    }
    Ok(settings)
}

const NOT_PORTED: &str = "elk-layered does not support this option";
const WRONG_TARGET: &str = "the option does not apply to this kind of element";
const PORTS_LATER: &str = "ports are not supported yet";
const INTERACTIVE: &str = "interactive strategies need input positions, which are not read";
const INSIDE_SELF_LOOPS: &str = "inside self-loops are not supported";
const JUNCTION_POINTS: &str = "merged edges need junction points, which are not ported";

/// No ELK counterpart: refuses a value whose effect is not ported.
fn refuse_true(value: bool, reason: &'static str) -> Result<(), OptionProblem> {
    if value {
        Err(OptionProblem::Unsupported(reason))
    } else {
        Ok(())
    }
}

/// Ports `LayoutOptionData.parseValue` and `IPropertyHolder.setProperty` for each node option.
#[allow(clippy::too_many_lines)]
fn apply_node_option(s: &mut NodeSettings, id: &str, value: &str) -> Result<(), OptionProblem> {
    let o = &mut s.options;
    match id {
        "org.eclipse.elk.algorithm" => s.algorithm = Some(value.to_string()),
        "org.eclipse.elk.direction" => {
            let direction = parse_enum(
                value,
                &[
                    ("UNDEFINED", ElkDirection::Undefined),
                    ("RIGHT", ElkDirection::Right),
                    ("LEFT", ElkDirection::Left),
                    ("DOWN", ElkDirection::Down),
                    ("UP", ElkDirection::Up),
                ],
            )?;
            o.direction = direction;
            s.direction = Some(direction);
        }
        "org.eclipse.elk.layered.directionCongruency" => {
            o.direction_congruency = parse_enum(
                value,
                &[
                    ("READING_DIRECTION", DirectionCongruency::ReadingDirection),
                    ("ROTATION", DirectionCongruency::Rotation),
                ],
            )?;
        }
        "org.eclipse.elk.hierarchyHandling" => {
            let handling = parse_enum(
                value,
                &[
                    ("INHERIT", HierarchyHandling::Inherit),
                    ("INCLUDE_CHILDREN", HierarchyHandling::IncludeChildren),
                    ("SEPARATE_CHILDREN", HierarchyHandling::SeparateChildren),
                ],
            )?;
            o.hierarchy_handling = handling;
            s.hierarchy_handling = Some(handling);
        }
        "org.eclipse.elk.separateConnectedComponents" => {
            o.separate_connected_components = parse_bool(value)?;
        }
        "org.eclipse.elk.aspectRatio" => o.aspect_ratio = parse_double(value)?,
        "org.eclipse.elk.portLabels.nextToPortIfPossible" => {
            o.port_labels_placement_next_to_port_if_possible = parse_bool(value)?;
        }
        "org.eclipse.elk.portLabels.treatAsGroup" => {
            o.port_labels_treat_as_group = parse_bool(value)?;
        }
        "org.eclipse.elk.insideSelfLoops.activate" => {
            refuse_true(parse_bool(value)?, INSIDE_SELF_LOOPS)?;
        }
        "org.eclipse.elk.edgeRouting" => {
            // Layered routes `UNDEFINED` orthogonally, but other steps treat it apart from
            // `ORTHOGONAL`, which elk-layered does not model.
            o.edge_routing = parse_enum(
                value,
                &[
                    ("UNDEFINED", None),
                    ("POLYLINE", Some(EdgeRouting::Polyline)),
                    ("ORTHOGONAL", Some(EdgeRouting::Orthogonal)),
                    ("SPLINES", Some(EdgeRouting::Splines)),
                ],
            )?
            .ok_or(OptionProblem::Unsupported(
                "the UNDEFINED edge routing is not ported",
            ))?;
        }
        "org.eclipse.elk.padding" => o.padding = parse_spacing(value)?,
        "org.eclipse.elk.nodeLabels.padding" => o.node_labels_padding = parse_spacing(value)?,
        "org.eclipse.elk.nodeLabels.placement" => {
            s.node_label_placement = parse_node_label_placement(value)?;
        }
        "org.eclipse.elk.nodeSize.constraints" => {
            let constraints = parse_flags(
                value,
                &SizeConstraints::NAMES,
                SizeConstraints::FIXED,
                |a, b| a | b,
            )?;
            if constraints.contains(SizeConstraints::PORTS)
                || constraints.contains(SizeConstraints::PORT_LABELS)
            {
                return Err(OptionProblem::Unsupported(PORTS_LATER));
            }
            s.node_size.constraints = constraints;
        }
        "org.eclipse.elk.nodeSize.options" => {
            s.node_size.options =
                parse_flags(value, &SizeOptions::NAMES, SizeOptions::NONE, |mut a, b| {
                    a.insert(b);
                    a
                })?;
        }
        "org.eclipse.elk.nodeSize.minimum" => {
            let (width, height) = parse_vector(value)?;
            s.node_size.minimum = LSize { width, height };
        }
        "org.eclipse.elk.contentAlignment" => {
            // `[V_TOP, V_CENTER, V_BOTTOM, H_LEFT, H_CENTER, H_RIGHT]`, in ELK's order.
            let mask = parse_flags(
                value,
                &[
                    ("V_TOP", 1u8),
                    ("V_CENTER", 2),
                    ("V_BOTTOM", 4),
                    ("H_LEFT", 8),
                    ("H_CENTER", 16),
                    ("H_RIGHT", 32),
                ],
                0,
                |a, b| a | b,
            )?;
            // `ElkLayered.resizeGraphNoReallyIMeanIt` checks center before the far side.
            o.horizontal_content_alignment = if mask & 16 != 0 {
                ContentAlignment::Center
            } else if mask & 32 != 0 {
                ContentAlignment::End
            } else {
                ContentAlignment::Start
            };
            o.vertical_content_alignment = if mask & 2 != 0 {
                ContentAlignment::Center
            } else if mask & 4 != 0 {
                ContentAlignment::End
            } else {
                ContentAlignment::Start
            };
        }
        "org.eclipse.elk.spacing.componentComponent" => {
            spacing(o, ExplicitSpacings::COMPONENT_COMPONENT, value, |sp| {
                &mut sp.component_component
            })?;
        }
        "org.eclipse.elk.spacing.edgeEdge" => {
            spacing(o, ExplicitSpacings::EDGE_EDGE, value, |sp| {
                &mut sp.edge_edge
            })?;
        }
        "org.eclipse.elk.spacing.edgeLabel" => {
            spacing(o, ExplicitSpacings::EDGE_LABEL, value, |sp| {
                &mut sp.edge_label
            })?;
        }
        "org.eclipse.elk.spacing.edgeNode" => {
            spacing(o, ExplicitSpacings::EDGE_NODE, value, |sp| {
                &mut sp.edge_node
            })?;
        }
        "org.eclipse.elk.spacing.labelLabel" => {
            spacing(o, ExplicitSpacings::LABEL_LABEL, value, |sp| {
                &mut sp.label_label
            })?;
        }
        "org.eclipse.elk.spacing.labelNode" => {
            spacing(o, ExplicitSpacings::LABEL_NODE, value, |sp| {
                &mut sp.label_node
            })?;
        }
        "org.eclipse.elk.spacing.labelPortHorizontal" => {
            spacing(o, ExplicitSpacings::LABEL_PORT_HORIZONTAL, value, |sp| {
                &mut sp.label_port_horizontal
            })?;
        }
        "org.eclipse.elk.spacing.labelPortVertical" => {
            spacing(o, ExplicitSpacings::LABEL_PORT_VERTICAL, value, |sp| {
                &mut sp.label_port_vertical
            })?;
        }
        "org.eclipse.elk.spacing.nodeNode" => {
            spacing(o, ExplicitSpacings::NODE_NODE, value, |sp| {
                &mut sp.node_node
            })?;
        }
        "org.eclipse.elk.spacing.nodeSelfLoop" => {
            spacing(o, ExplicitSpacings::NODE_SELF_LOOP, value, |sp| {
                &mut sp.node_self_loop
            })?;
        }
        "org.eclipse.elk.spacing.portPort" => {
            spacing(o, ExplicitSpacings::PORT_PORT, value, |sp| {
                &mut sp.port_port
            })?;
        }
        "org.eclipse.elk.layered.spacing.edgeEdgeBetweenLayers" => {
            spacing(o, ExplicitSpacings::EDGE_EDGE_BETWEEN_LAYERS, value, |sp| {
                &mut sp.edge_edge_between_layers
            })?;
        }
        "org.eclipse.elk.layered.spacing.edgeNodeBetweenLayers" => {
            spacing(o, ExplicitSpacings::EDGE_NODE_BETWEEN_LAYERS, value, |sp| {
                &mut sp.edge_node_between_layers
            })?;
        }
        "org.eclipse.elk.layered.spacing.nodeNodeBetweenLayers" => {
            spacing(o, ExplicitSpacings::NODE_NODE_BETWEEN_LAYERS, value, |sp| {
                &mut sp.node_node_between_layers
            })?;
        }
        "org.eclipse.elk.layered.spacing.baseValue" => {
            o.spacing_base_value = Some(parse_double(value)?);
        }
        "org.eclipse.elk.spacing.portsSurrounding" => {
            let margin = parse_spacing(value)?;
            o.spacing.ports_surrounding = SpacingMargin {
                top: margin.top,
                right: margin.right,
                bottom: margin.bottom,
                left: margin.left,
            };
        }
        "org.eclipse.elk.layered.cycleBreaking.strategy" => {
            o.cycle_breaking_strategy = parse_enum(
                value,
                &[
                    ("GREEDY", CycleBreakingStrategy::Greedy),
                    ("DEPTH_FIRST", CycleBreakingStrategy::DepthFirst),
                    ("INTERACTIVE", CycleBreakingStrategy::Interactive),
                    ("MODEL_ORDER", CycleBreakingStrategy::ModelOrder),
                    (
                        "GREEDY_MODEL_ORDER",
                        CycleBreakingStrategy::GreedyModelOrder,
                    ),
                ],
            )?;
            if o.cycle_breaking_strategy == CycleBreakingStrategy::Interactive {
                return Err(OptionProblem::Unsupported(INTERACTIVE));
            }
        }
        "org.eclipse.elk.layered.layering.strategy" => {
            o.layering_strategy = parse_enum(
                value,
                &[
                    ("NETWORK_SIMPLEX", LayeringStrategy::NetworkSimplex),
                    ("LONGEST_PATH", LayeringStrategy::LongestPath),
                    ("LONGEST_PATH_SOURCE", LayeringStrategy::LongestPathSource),
                    ("COFFMAN_GRAHAM", LayeringStrategy::CoffmanGraham),
                    ("INTERACTIVE", LayeringStrategy::Interactive),
                    ("STRETCH_WIDTH", LayeringStrategy::StretchWidth),
                    ("MIN_WIDTH", LayeringStrategy::MinWidth),
                    ("BF_MODEL_ORDER", LayeringStrategy::BreadthFirstModelOrder),
                    ("DF_MODEL_ORDER", LayeringStrategy::DepthFirstModelOrder),
                ],
            )?;
            if o.layering_strategy == LayeringStrategy::Interactive {
                return Err(OptionProblem::Unsupported(INTERACTIVE));
            }
        }
        "org.eclipse.elk.layered.layering.coffmanGraham.layerBound" => {
            o.layering_coffman_graham_layer_bound = parse_int(value)?;
        }
        "org.eclipse.elk.layered.layering.minWidth.upperBoundOnWidth" => {
            o.layering_min_width_upper_bound = parse_int(value)?;
        }
        "org.eclipse.elk.layered.layering.minWidth.upperLayerEstimationScalingFactor" => {
            o.layering_min_width_upper_layer_estimation_scaling_factor = parse_int(value)?;
        }
        "org.eclipse.elk.layered.layering.layerConstraint" => {
            s.layer_constraint = Some(parse_enum(
                value,
                &[
                    ("NONE", LayerConstraint::None),
                    ("FIRST", LayerConstraint::First),
                    ("FIRST_SEPARATE", LayerConstraint::FirstSeparate),
                    ("LAST", LayerConstraint::Last),
                    ("LAST_SEPARATE", LayerConstraint::LastSeparate),
                ],
            )?);
        }
        "org.eclipse.elk.layered.crossingMinimization.strategy" => {
            o.crossing_minimization_strategy = parse_enum(
                value,
                &[
                    ("LAYER_SWEEP", CrossingMinimizationStrategy::LayerSweep),
                    ("INTERACTIVE", CrossingMinimizationStrategy::Interactive),
                    ("NONE", CrossingMinimizationStrategy::None),
                ],
            )?;
            if o.crossing_minimization_strategy == CrossingMinimizationStrategy::Interactive {
                return Err(OptionProblem::Unsupported(INTERACTIVE));
            }
        }
        "org.eclipse.elk.layered.crossingMinimization.hierarchicalSweepiness" => {
            o.hierarchical_sweepiness = parse_double(value)?;
        }
        "org.eclipse.elk.layered.crossingMinimization.forceNodeModelOrder" => {
            o.force_node_model_order = parse_bool(value)?;
        }
        "org.eclipse.elk.layered.crossingMinimization.greedySwitch.type" => {
            o.greedy_switch_type = parse_greedy_switch(value)?;
        }
        "org.eclipse.elk.layered.crossingMinimization.greedySwitchHierarchical.type" => {
            o.greedy_switch_hierarchical_type = parse_greedy_switch(value)?;
        }
        "org.eclipse.elk.layered.crossingMinimization.greedySwitch.activationThreshold" => {
            o.greedy_switch_activation_threshold =
                usize::try_from(parse_int(value)?).map_err(|_| OptionProblem::InvalidValue)?;
        }
        "org.eclipse.elk.layered.nodePlacement.strategy" => {
            o.node_placement_strategy = parse_enum(
                value,
                &[
                    ("SIMPLE", NodePlacementStrategy::Simple),
                    ("INTERACTIVE", NodePlacementStrategy::Interactive),
                    ("LINEAR_SEGMENTS", NodePlacementStrategy::LinearSegments),
                    ("BRANDES_KOEPF", NodePlacementStrategy::BrandesKoepf),
                    ("NETWORK_SIMPLEX", NodePlacementStrategy::NetworkSimplex),
                ],
            )?;
            match o.node_placement_strategy {
                NodePlacementStrategy::Interactive => {
                    return Err(OptionProblem::Unsupported(INTERACTIVE));
                }
                NodePlacementStrategy::LinearSegments => {
                    return Err(OptionProblem::Unsupported(
                        "the linear segments node placer is not ported",
                    ));
                }
                _ => {}
            }
        }
        "org.eclipse.elk.layered.nodePlacement.favorStraightEdges" => {
            o.node_placement_favor_straight_edges = Some(parse_bool(value)?);
        }
        "org.eclipse.elk.layered.nodePlacement.bk.fixedAlignment" => {
            o.node_placement_bk_fixed_alignment = parse_enum(
                value,
                &[
                    ("NONE", FixedAlignment::None),
                    ("LEFTUP", FixedAlignment::LeftUp),
                    ("RIGHTUP", FixedAlignment::RightUp),
                    ("LEFTDOWN", FixedAlignment::LeftDown),
                    ("RIGHTDOWN", FixedAlignment::RightDown),
                    ("BALANCED", FixedAlignment::Balanced),
                ],
            )?;
        }
        "org.eclipse.elk.layered.nodePlacement.bk.edgeStraightening" => {
            o.node_placement_bk_edge_straightening = parse_enum(
                value,
                &[
                    ("NONE", EdgeStraighteningStrategy::None),
                    (
                        "IMPROVE_STRAIGHTNESS",
                        EdgeStraighteningStrategy::ImproveStraightness,
                    ),
                ],
            )?;
        }
        "org.eclipse.elk.layered.nodePlacement.networkSimplex.nodeFlexibility" => {
            // `NODE_SIZE_WHERE_SPACE_PERMITS` and `NODE_SIZE` are not ported.
            s.node_flexibility = parse_enum(
                value,
                &[
                    ("NONE", Some(NodeFlexibility::None)),
                    ("PORT_POSITION", Some(NodeFlexibility::PortPosition)),
                    ("NODE_SIZE_WHERE_SPACE_PERMITS", None),
                    ("NODE_SIZE", None),
                ],
            )?
            .ok_or(OptionProblem::Unsupported(
                "node size flexibility is not ported",
            ))?;
        }
        "org.eclipse.elk.layered.considerModelOrder.strategy" => {
            o.consider_model_order_strategy = parse_enum(
                value,
                &[
                    ("NONE", OrderingStrategy::None),
                    ("NODES_AND_EDGES", OrderingStrategy::NodesAndEdges),
                    ("PREFER_EDGES", OrderingStrategy::PreferEdges),
                    ("PREFER_NODES", OrderingStrategy::PreferNodes),
                ],
            )?;
        }
        "org.eclipse.elk.layered.considerModelOrder.longEdgeStrategy" => {
            o.consider_model_order_long_edge_strategy = parse_enum(
                value,
                &[
                    ("DUMMY_NODE_OVER", LongEdgeOrderingStrategy::DummyNodeOver),
                    ("DUMMY_NODE_UNDER", LongEdgeOrderingStrategy::DummyNodeUnder),
                    ("EQUAL", LongEdgeOrderingStrategy::Equal),
                ],
            )?;
        }
        "org.eclipse.elk.layered.considerModelOrder.portModelOrder" => {
            o.consider_model_order_port_model_order = parse_bool(value)?;
        }
        "org.eclipse.elk.layered.considerModelOrder.crossingCounterNodeInfluence" => {
            o.consider_model_order_crossing_counter_node_influence = parse_double(value)?;
        }
        "org.eclipse.elk.layered.considerModelOrder.crossingCounterPortInfluence" => {
            o.consider_model_order_crossing_counter_port_influence = parse_double(value)?;
        }
        "org.eclipse.elk.layered.portSortingStrategy" => {
            o.port_sorting_strategy = parse_enum(
                value,
                &[
                    ("INPUT_ORDER", PortSortingStrategy::InputOrder),
                    ("PORT_DEGREE", PortSortingStrategy::PortDegree),
                ],
            )?;
        }
        "org.eclipse.elk.portConstraints" => {
            s.port_constraints = Some(parse_enum(
                value,
                &[
                    ("UNDEFINED", PortConstraints::Undefined),
                    ("FREE", PortConstraints::Free),
                    ("FIXED_SIDE", PortConstraints::FixedSide),
                    ("FIXED_ORDER", PortConstraints::FixedOrder),
                    ("FIXED_RATIO", PortConstraints::FixedRatio),
                    ("FIXED_POS", PortConstraints::FixedPos),
                ],
            )?);
        }
        "org.eclipse.elk.portAlignment.default" => {
            s.port_alignment = Some(parse_enum(
                value,
                &[
                    ("DISTRIBUTED", PortAlignment::Distributed),
                    ("JUSTIFIED", PortAlignment::Justified),
                    ("BEGIN", PortAlignment::Begin),
                    ("CENTER", PortAlignment::Center),
                    ("END", PortAlignment::End),
                ],
            )?);
        }
        "org.eclipse.elk.layered.mergeEdges" => {
            refuse_true(parse_bool(value)?, JUNCTION_POINTS)?;
            o.merge_edges = false;
        }
        "org.eclipse.elk.layered.mergeHierarchyEdges" => {
            o.merge_hierarchy_edges = parse_bool(value)?;
        }
        "org.eclipse.elk.layered.unnecessaryBendpoints" => {
            o.unnecessary_bendpoints = parse_bool(value)?;
        }
        "org.eclipse.elk.layered.edgeRouting.selfLoopDistribution" => {
            s.self_loop_distribution = parse_enum(
                value,
                &[
                    ("EQUALLY", SelfLoopDistributionStrategy::Equally),
                    ("NORTH", SelfLoopDistributionStrategy::North),
                    ("NORTH_SOUTH", SelfLoopDistributionStrategy::NorthSouth),
                ],
            )?;
        }
        "org.eclipse.elk.layered.edgeRouting.selfLoopOrdering" => {
            s.self_loop_ordering = parse_enum(
                value,
                &[
                    ("STACKED", SelfLoopOrderingStrategy::Stacked),
                    ("REVERSE_STACKED", SelfLoopOrderingStrategy::ReverseStacked),
                    ("SEQUENCED", SelfLoopOrderingStrategy::Sequenced),
                ],
            )?;
        }
        "org.eclipse.elk.layered.wrapping.strategy" => {
            o.wrapping_strategy = parse_enum(
                value,
                &[
                    ("OFF", WrappingStrategy::Off),
                    ("SINGLE_EDGE", WrappingStrategy::SingleEdge),
                    ("MULTI_EDGE", WrappingStrategy::MultiEdge),
                ],
            )?;
        }
        "org.eclipse.elk.layered.wrapping.multiEdge.improveCuts" => {
            o.wrapping_multi_edge_improve_cuts = parse_bool(value)?;
        }
        "org.eclipse.elk.layered.wrapping.multiEdge.improveWrappedEdges" => {
            o.wrapping_multi_edge_improve_wrapped_edges = parse_bool(value)?;
        }
        "org.eclipse.elk.layered.feedbackEdges" => o.feedback_edges = parse_bool(value)?,
        "org.eclipse.elk.randomSeed" => o.random_seed = parse_int(value)?,
        "org.eclipse.elk.layered.thoroughness" => {
            o.thoroughness =
                usize::try_from(parse_int(value)?).map_err(|_| OptionProblem::InvalidValue)?;
        }
        "org.eclipse.elk.layered.edgeLabels.sideSelection" => {
            o.edge_label_side_selection = parse_enum(
                value,
                &[
                    ("ALWAYS_UP", EdgeLabelSideSelection::AlwaysUp),
                    ("ALWAYS_DOWN", EdgeLabelSideSelection::AlwaysDown),
                    ("DIRECTION_UP", EdgeLabelSideSelection::DirectionUp),
                    ("DIRECTION_DOWN", EdgeLabelSideSelection::DirectionDown),
                    ("SMART_UP", EdgeLabelSideSelection::SmartUp),
                    ("SMART_DOWN", EdgeLabelSideSelection::SmartDown),
                ],
            )?;
        }
        "org.eclipse.elk.port.anchor"
        | "org.eclipse.elk.port.index"
        | "org.eclipse.elk.port.side"
        | "org.eclipse.elk.port.borderOffset"
        | "org.eclipse.elk.portLabels.placement"
        | "org.eclipse.elk.portAlignment.north"
        | "org.eclipse.elk.portAlignment.south"
        | "org.eclipse.elk.portAlignment.west"
        | "org.eclipse.elk.portAlignment.east" => {
            return Err(OptionProblem::Unsupported(PORTS_LATER));
        }
        _ => return Err(OptionProblem::Unsupported(NOT_PORTED)),
    }
    Ok(())
}

/// Ports `LayoutOptionData.parseValue` for a spacing, remembering it was set for `ElkSpacings.AbstractSpacingsBuilder`.
fn spacing(
    options: &mut LayeredOptions,
    flag: ExplicitSpacings,
    value: &str,
    field: impl FnOnce(&mut crate::options::SpacingOptions) -> &mut f64,
) -> Result<(), OptionProblem> {
    *field(&mut options.spacing) = parse_double(value)?;
    options.explicit_spacings.insert(flag);
    Ok(())
}

/// Ports `LayoutOptionData.enumForString` for `GreedySwitchType`.
fn parse_greedy_switch(value: &str) -> Result<GreedySwitchType, OptionProblem> {
    parse_enum(
        value,
        &[
            ("ONE_SIDED", GreedySwitchType::OneSided),
            ("TWO_SIDED", GreedySwitchType::TwoSided),
            ("OFF", GreedySwitchType::Off),
        ],
    )
}

/// Ports `LayoutOptionData.enumSetForStringArray` and `NodeLabelPlacement.isValid`.
fn parse_node_label_placement(value: &str) -> Result<NodeLabelPlacement, OptionProblem> {
    let names = NodeLabelPlacementFlag::ALL.map(|flag| (flag.name(), flag));
    let placement = parse_flags(value, &names, NodeLabelPlacement::FIXED, |mut set, flag| {
        set.insert(flag);
        set
    })?;
    // `NodeContext` throws for an invalid set, so elkjs fails such a layout.
    if !placement.is_valid() {
        return Err(OptionProblem::InvalidValue);
    }
    Ok(placement)
}

/// Ports `LayoutOptionData.parseValue`, case `BOOLEAN`.
pub(crate) fn parse_bool(value: &str) -> Result<bool, OptionProblem> {
    if value.eq_ignore_ascii_case("true") {
        Ok(true)
    } else if value.eq_ignore_ascii_case("false") {
        Ok(false)
    } else {
        Err(OptionProblem::InvalidValue)
    }
}

/// Ports `LayoutOptionData.parseValue`, case `INT`: `Integer.valueOf`.
pub(crate) fn parse_int(value: &str) -> Result<i32, OptionProblem> {
    let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(OptionProblem::InvalidValue);
    }
    value
        .parse::<i32>()
        .map_err(|_| OptionProblem::InvalidValue)
}

/// Ports `LayoutOptionData.parseValue`, case `DOUBLE`, as GWT's `Double.valueOf` parses:
/// `^\s*[+-]?(NaN|Infinity|((\d+\.?\d*)|(\.\d+))([eE][+-]?\d+)?[dDfF]?)\s*$`. A value that is not
/// finite is refused too, because no layout can use it.
pub(crate) fn parse_double(value: &str) -> Result<f64, OptionProblem> {
    let trimmed = value.trim_matches(|c: char| c.is_whitespace());
    let body = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    let body = body.strip_suffix(['d', 'D', 'f', 'F']).unwrap_or(body);
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(at) => (&body[..at], Some(&body[at + 1..])),
        None => (body, None),
    };
    let (whole, fraction) = match mantissa.find('.') {
        Some(at) => (&mantissa[..at], Some(&mantissa[at + 1..])),
        None => (mantissa, None),
    };
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok = digits(whole)
        && fraction.is_none_or(digits)
        && (!whole.is_empty() || fraction.is_some_and(|f| !f.is_empty()));
    let exponent_ok = exponent.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && digits(e)
    });
    if !mantissa_ok || !exponent_ok {
        return Err(OptionProblem::InvalidValue);
    }
    let number = trimmed
        .strip_suffix(['d', 'D', 'f', 'F'])
        .unwrap_or(trimmed);
    let parsed = number
        .parse::<f64>()
        .map_err(|_| OptionProblem::InvalidValue)?;
    if parsed.is_finite() {
        Ok(parsed)
    } else {
        Err(OptionProblem::InvalidValue)
    }
}

/// Ports `LayoutOptionData.enumForString`: the exact constant name, or its ordinal. elkjs compares
/// names case-sensitively, so `down` is no direction.
fn parse_enum<T: Copy>(value: &str, values: &[(&str, T)]) -> Result<T, OptionProblem> {
    if let Some((_, v)) = values.iter().find(|(name, _)| *name == value) {
        return Ok(*v);
    }
    let ordinal = parse_int(value)?;
    usize::try_from(ordinal)
        .ok()
        .and_then(|index| values.get(index))
        .map(|(_, v)| *v)
        .ok_or(OptionProblem::InvalidValue)
}

/// Ports `LayoutOptionData.enumSetForStringArray`: the value splits at `[\[\]\s,]+`, and every part
/// must name a constant.
fn parse_flags<T: Copy, S>(
    value: &str,
    values: &[(&str, T)],
    empty: S,
    add: impl Fn(S, T) -> S,
) -> Result<S, OptionProblem> {
    let mut set = empty;
    for component in value
        .split(|c: char| c == '[' || c == ']' || c == ',' || c.is_whitespace())
        .filter(|component| !component.is_empty())
    {
        set = add(set, parse_enum(component, values)?);
    }
    Ok(set)
}

/// `String.split` the Java way: trailing empty strings are dropped.
///
/// Ports Java's `String.split`, which drops trailing empty strings.
fn java_split(value: &str, separator: impl Fn(char) -> bool) -> Vec<&str> {
    let mut parts: Vec<&str> = value.split(separator).collect();
    while parts.last().is_some_and(|part| part.is_empty()) {
        parts.pop();
    }
    parts
}

/// Strips the brackets and quotes `Spacing.parse` and `KVector.parse` skip at both ends.
///
/// Ports the delimiter skipping of `Spacing.parse` and `KVector.parse`.
fn strip_delimiters(value: &str) -> &str {
    value
        .trim_start_matches(['(', '[', '{', '"', '\'', ' ', '\t', '\r', '\n'])
        .trim_end_matches([')', ']', '}', '"', '\'', ' ', '\t', '\r', '\n'])
}

/// Ports `Spacing.parse`, which `ElkPadding` and `ElkMargin` share. Keys it does not know are
/// skipped; sides not named stay zero.
pub(crate) fn parse_spacing(value: &str) -> Result<ElkPadding, OptionProblem> {
    let mut padding = ElkPadding::uniform(0.0);
    let inner = strip_delimiters(value);
    if inner.is_empty() {
        return Ok(padding);
    }
    for token in java_split(inner, |c| c == ',' || c == ';') {
        let pair = java_split(token, |c| c == '=');
        if pair.len() != 2 {
            return Err(OptionProblem::InvalidValue);
        }
        let number = parse_double(pair[1].trim())?;
        match pair[0].trim() {
            "top" => padding.top = number,
            "left" => padding.left = number,
            "bottom" => padding.bottom = number,
            "right" => padding.right = number,
            _ => {}
        }
    }
    Ok(padding)
}

/// Ports `KVector.parse`: exactly two numbers.
pub(crate) fn parse_vector(value: &str) -> Result<(f64, f64), OptionProblem> {
    let inner = strip_delimiters(value);
    if inner.is_empty() {
        return Err(OptionProblem::InvalidValue);
    }
    let tokens = java_split(inner, |c| c == ',' || c == ';' || c == '\r' || c == '\n');
    if tokens.len() != 2 {
        return Err(OptionProblem::InvalidValue);
    }
    Ok((
        parse_double(tokens[0].trim())?,
        parse_double(tokens[1].trim())?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    fn node(pairs: &[(&str, &str)]) -> Result<NodeSettings, OptionProblem> {
        node_settings(&map(pairs)).map_err(|error| error.problem)
    }

    // Mutation: drop the dot check in `is_id_suffix`.
    #[test]
    fn an_id_suffix_is_the_id_or_the_part_after_a_dot() {
        let id = "org.eclipse.elk.layered";
        assert!(is_id_suffix(id, id));
        assert!(is_id_suffix(id, "layered"));
        assert!(is_id_suffix(id, "elk.layered"));
        assert!(!is_id_suffix(id, "ayered"));
        assert!(!is_id_suffix(id, "xorg.eclipse.elk.layered"));
        assert!(!is_id_suffix(id, ""));
    }

    // Mutation: drop the dot check in `resolve`.
    #[test]
    fn a_key_resolves_by_its_id_or_a_suffix_after_a_dot() {
        let direction = Some(("org.eclipse.elk.direction", PARENTS));
        assert_eq!(resolve("org.eclipse.elk.direction"), direction);
        assert_eq!(resolve("elk.direction"), direction);
        assert_eq!(resolve("eclipse.elk.direction"), direction);
        assert_eq!(
            resolve("spacing.nodeNode").map(|r| r.0),
            Some("org.eclipse.elk.spacing.nodeNode")
        );
        assert_eq!(
            resolve("nodeNode").map(|r| r.0),
            Some("org.eclipse.elk.spacing.nodeNode")
        );
        // `direction` also ends `layered.priority.direction`, so elkjs cannot resolve it.
        assert_eq!(resolve("direction"), None);
        assert_eq!(resolve("irection"), None);
        assert_eq!(resolve("lk.direction"), None);
        assert_eq!(resolve("elk.Direction"), None);
        assert_eq!(resolve(""), None);
    }

    // Mutation: compare with `==` instead of `eq_ignore_ascii_case` in `parse_bool`.
    #[test]
    fn booleans_ignore_case_and_nothing_else() {
        assert_eq!(parse_bool("TRUE"), Ok(true));
        assert_eq!(parse_bool("False"), Ok(false));
        for value in ["yes", "1", " true", ""] {
            assert_eq!(
                parse_bool(value),
                Err(OptionProblem::InvalidValue),
                "{value}"
            );
        }
    }

    // Mutation: let `parse_int` accept any `i64`.
    #[test]
    fn integers_follow_integer_value_of() {
        assert_eq!(parse_int("+3"), Ok(3));
        assert_eq!(parse_int("-2147483648"), Ok(i32::MIN));
        for value in ["2147483648", "3.0", "", "+", " 3", "0x1"] {
            assert_eq!(
                parse_int(value),
                Err(OptionProblem::InvalidValue),
                "{value}"
            );
        }
    }

    // Mutation: drop the `[dDfF]` suffix handling in `parse_double`.
    #[test]
    fn doubles_follow_the_gwt_syntax() {
        for (value, want) in [
            ("1e1", 10.0),
            (".5", 0.5),
            ("5.", 5.0),
            ("2d", 2.0),
            ("-1.5F", -1.5),
            (" 3 ", 3.0),
            ("+4E-1", 0.4),
        ] {
            assert_eq!(parse_double(value), Ok(want), "{value}");
        }
        for value in [
            "NaN", "Infinity", "abc", "1e", ".", "", "1,5", "0x10", "1e999",
        ] {
            assert_eq!(
                parse_double(value),
                Err(OptionProblem::InvalidValue),
                "{value}"
            );
        }
    }

    // Mutation: clamp the ordinal to the last value in `parse_enum`.
    #[test]
    fn an_enum_is_its_exact_name_or_its_ordinal() {
        let values = [("A", 1), ("B_C", 2)];
        assert_eq!(parse_enum("B_C", &values), Ok(2));
        assert_eq!(parse_enum("0", &values), Ok(1));
        for value in ["b_c", "2", "-1", "", " A"] {
            assert_eq!(
                parse_enum(value, &values),
                Err(OptionProblem::InvalidValue),
                "{value}"
            );
        }
    }

    // Mutation: split EnumSet values at brackets and commas, not at blanks.
    #[test]
    fn an_enum_set_splits_at_brackets_commas_and_blanks() {
        let parse = |value| {
            parse_flags(
                value,
                &SizeConstraints::NAMES,
                SizeConstraints::FIXED,
                |a, b| a | b,
            )
        };
        let both = SizeConstraints::NODE_LABELS | SizeConstraints::MINIMUM_SIZE;
        assert_eq!(parse("[NODE_LABELS, MINIMUM_SIZE]"), Ok(both));
        assert_eq!(parse("NODE_LABELS MINIMUM_SIZE"), Ok(both));
        assert_eq!(parse("[ NODE_LABELS,,MINIMUM_SIZE ]"), Ok(both));
        assert_eq!(parse(""), Ok(SizeConstraints::FIXED));
        assert_eq!(parse("[]"), Ok(SizeConstraints::FIXED));
        assert_eq!(
            parse("[NODE_LABELS, OTHER]"),
            Err(OptionProblem::InvalidValue)
        );
    }

    // Mutation: start `parse_spacing` from ELK's default padding of 12.
    #[test]
    fn a_padding_names_its_sides_and_leaves_the_rest_zero() {
        let padding = |top, right, bottom, left| ElkPadding {
            top,
            right,
            bottom,
            left,
        };
        assert_eq!(
            parse_spacing("[top=1,left=2,bottom=3,right=4]"),
            Ok(padding(1.0, 4.0, 3.0, 2.0))
        );
        assert_eq!(
            parse_spacing("(top=5; left = 6)"),
            Ok(padding(5.0, 0.0, 0.0, 6.0))
        );
        assert_eq!(parse_spacing("[top=1,]"), Ok(padding(1.0, 0.0, 0.0, 0.0)));
        assert_eq!(parse_spacing("[other=1]"), Ok(padding(0.0, 0.0, 0.0, 0.0)));
        assert_eq!(parse_spacing("[]"), Ok(padding(0.0, 0.0, 0.0, 0.0)));
        for value in ["[top]", "[top=x]", "[,top=1]", "[top=1=2=3]"] {
            assert_eq!(
                parse_spacing(value),
                Err(OptionProblem::InvalidValue),
                "{value}"
            );
        }
    }

    // Mutation: let `parse_vector` accept any number of numbers.
    #[test]
    fn a_vector_has_exactly_two_numbers() {
        assert_eq!(parse_vector("(1, 2)"), Ok((1.0, 2.0)));
        assert_eq!(parse_vector("3;4"), Ok((3.0, 4.0)));
        assert_eq!(parse_vector("{5,6}"), Ok((5.0, 6.0)));
        for value in ["(1)", "()", "(1,2,3)", "(a,b)"] {
            assert_eq!(
                parse_vector(value),
                Err(OptionProblem::InvalidValue),
                "{value}"
            );
        }
    }

    // Mutation: let `parse_node_label_placement` skip the validity check.
    #[test]
    fn a_node_label_placement_must_be_valid() {
        let settings = node(&[("elk.nodeLabels.placement", "[H_LEFT, V_TOP, INSIDE]")]).unwrap();
        assert_eq!(
            settings.node_label_placement.location(),
            crate::options::NodeLabelLocation::InsideTopLeft
        );
        assert_eq!(
            node(&[("elk.nodeLabels.placement", "[H_LEFT, H_RIGHT]")]).err(),
            Some(OptionProblem::InvalidValue)
        );
        assert_eq!(
            node(&[("elk.nodeLabels.placement", "[INSIDE, OUTSIDE]")]).err(),
            Some(OptionProblem::InvalidValue)
        );
    }

    // Mutation: drop `settings.options.node_size = settings.node_size` in `node_settings`.
    #[test]
    fn node_size_options_reach_the_node_and_its_graph() {
        let settings = node(&[
            ("elk.nodeSize.constraints", "[NODE_LABELS]"),
            ("elk.nodeSize.options", "[ASYMMETRICAL]"),
            ("elk.nodeSize.minimum", "(3, 4)"),
        ])
        .unwrap();
        assert_eq!(settings.node_size.constraints, SizeConstraints::NODE_LABELS);
        assert!(
            settings
                .node_size
                .options
                .contains(SizeOptions::ASYMMETRICAL)
        );
        assert!(
            !settings
                .node_size
                .options
                .contains(SizeOptions::DEFAULT_MINIMUM_SIZE)
        );
        assert_eq!(
            settings.node_size.minimum,
            LSize {
                width: 3.0,
                height: 4.0
            }
        );
        assert_eq!(settings.options.node_size, settings.node_size);
    }

    // Mutation: let `H_RIGHT` win over `H_CENTER` for the content alignment.
    #[test]
    fn content_alignment_prefers_the_center() {
        let settings = node(&[("elk.contentAlignment", "[H_RIGHT, H_CENTER, V_BOTTOM]")]).unwrap();
        assert_eq!(
            settings.options.horizontal_content_alignment,
            ContentAlignment::Center
        );
        assert_eq!(
            settings.options.vertical_content_alignment,
            ContentAlignment::End
        );
        let settings = node(&[("elk.contentAlignment", "[]")]).unwrap();
        assert_eq!(
            settings.options.horizontal_content_alignment,
            ContentAlignment::Start
        );
    }

    // Mutation: drop `options.explicit_spacings.insert(flag)` in `spacing`.
    #[test]
    fn a_spacing_is_marked_explicit_and_the_base_value_is_kept() {
        let settings = node(&[
            ("elk.spacing.nodeNode", "7"),
            ("elk.layered.spacing.baseValue", "40"),
        ])
        .unwrap();
        assert_eq!(settings.options.spacing.node_node, 7.0);
        assert!(
            settings
                .options
                .explicit_spacings
                .contains(ExplicitSpacings::NODE_NODE)
        );
        assert!(
            !settings
                .options
                .explicit_spacings
                .contains(ExplicitSpacings::EDGE_EDGE)
        );
        assert_eq!(settings.options.spacing_base_value, Some(40.0));
        let applied = settings.options.with_spacing_base_value_applied();
        assert_eq!(applied.spacing.node_node, 7.0);
        assert_eq!(applied.spacing.edge_edge, 20.0);
    }

    // Mutation: drop the target check in `resolve_all`.
    #[test]
    fn an_option_must_target_its_kind_of_element() {
        let wrong = Some(OptionProblem::Unsupported(WRONG_TARGET));
        assert_eq!(node(&[("elk.edgeLabels.placement", "HEAD")]).err(), wrong);
        assert_eq!(
            edge_settings(&map(&[("elk.direction", "DOWN")]))
                .map_err(|e| e.problem)
                .err(),
            wrong
        );
        assert_eq!(
            label_settings(&map(&[("elk.padding", "[]")]))
                .map_err(|e| e.problem)
                .err(),
            wrong
        );
    }

    // Mutation: let `refuse_true` accept true.
    #[test]
    fn options_with_unported_values_are_refused_only_for_those_values() {
        assert!(node(&[("elk.layered.mergeEdges", "false")]).is_ok());
        assert!(matches!(
            node(&[("elk.layered.mergeEdges", "true")]),
            Err(OptionProblem::Unsupported(_))
        ));
        assert!(node(&[("elk.layered.nodePlacement.strategy", "SIMPLE")]).is_ok());
        for (key, value) in [
            ("elk.layered.nodePlacement.strategy", "LINEAR_SEGMENTS"),
            ("elk.layered.nodePlacement.strategy", "INTERACTIVE"),
            ("elk.layered.layering.strategy", "INTERACTIVE"),
            ("elk.layered.crossingMinimization.strategy", "INTERACTIVE"),
            (
                "elk.layered.nodePlacement.networkSimplex.nodeFlexibility",
                "NODE_SIZE",
            ),
            ("elk.insideSelfLoops.activate", "true"),
            ("elk.nodeSize.constraints", "[PORT_LABELS]"),
            ("elk.port.side", "WEST"),
        ] {
            assert!(
                matches!(node(&[(key, value)]), Err(OptionProblem::Unsupported(_))),
                "{key} = {value}"
            );
        }
    }

    // Mutation: drop the duplicate check in `resolve_all`.
    #[test]
    fn two_keys_for_one_option_are_refused() {
        assert_eq!(
            node(&[("elk.direction", "UP"), ("org.eclipse.elk.direction", "UP")]).err(),
            Some(OptionProblem::Duplicate {
                other: "elk.direction".into()
            })
        );
    }

    // Mutation: drop the `HEAD` entry of the edge label placements.
    #[test]
    fn edge_and_label_options_are_read() {
        let edge = edge_settings(&map(&[
            ("elk.layered.priority.direction", "3"),
            ("elk.layered.priority.shortness", "-1"),
            ("org.eclipse.elk.layered.priority.straightness", "+2"),
            ("elk.insideSelfLoops.yo", "false"),
        ]))
        .unwrap();
        assert_eq!(
            edge,
            EdgeSettings {
                priority_direction: 3,
                priority_shortness: -1,
                priority_straightness: 2,
            }
        );
        let label = label_settings(&map(&[
            ("elk.edgeLabels.placement", "HEAD"),
            ("elk.edgeLabels.inline", "true"),
            ("elk.nodeLabels.placement", "[H_CENTER, V_CENTER, INSIDE]"),
        ]))
        .unwrap();
        assert_eq!(label.placement, EdgeLabelPlacement::Head);
        assert!(label.inline);
        assert!(label.node_label_placement.is_some());
        assert_eq!(label_settings(&map(&[])).unwrap(), LabelSettings::default());
    }

    // Mutation: ignore `elk.algorithm` in `apply_node_option`.
    #[test]
    fn every_supported_node_option_is_read() {
        let settings = node(&[
            ("elk.algorithm", "layered"),
            ("elk.hierarchyHandling", "INCLUDE_CHILDREN"),
            ("elk.layered.layering.layerConstraint", "FIRST"),
            ("elk.portConstraints", "FIXED_ORDER"),
            ("elk.portAlignment.default", "BEGIN"),
            ("elk.layered.edgeRouting.selfLoopDistribution", "EQUALLY"),
            ("elk.layered.edgeRouting.selfLoopOrdering", "SEQUENCED"),
            (
                "elk.layered.nodePlacement.networkSimplex.nodeFlexibility",
                "PORT_POSITION",
            ),
            ("elk.randomSeed", "5"),
            ("elk.aspectRatio", "2"),
            ("elk.layered.thoroughness", "3"),
        ])
        .unwrap();
        assert_eq!(settings.algorithm.as_deref(), Some("layered"));
        assert_eq!(
            settings.hierarchy_handling,
            Some(HierarchyHandling::IncludeChildren)
        );
        assert_eq!(settings.layer_constraint, Some(LayerConstraint::First));
        assert_eq!(settings.port_constraints, Some(PortConstraints::FixedOrder));
        assert_eq!(settings.port_alignment, Some(PortAlignment::Begin));
        assert_eq!(
            settings.self_loop_distribution,
            SelfLoopDistributionStrategy::Equally
        );
        assert_eq!(
            settings.self_loop_ordering,
            SelfLoopOrderingStrategy::Sequenced
        );
        assert_eq!(settings.node_flexibility, NodeFlexibility::PortPosition);
        assert_eq!(settings.options.random_seed, 5);
        assert_eq!(settings.options.aspect_ratio, 2.0);
        assert_eq!(settings.options.thoroughness, 3);
        assert!(matches!(
            node(&[("elk.layered.thoroughness", "-1")]),
            Err(OptionProblem::InvalidValue)
        ));
    }
}
