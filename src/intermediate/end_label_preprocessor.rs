//! Port of ELK's `EndLabelPreprocessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/EndLabelPreprocessor.java

use crate::common::cells::grow_label_cell;
use crate::graph::{
    EdgeId, EdgeLabelPlacement, HorizontalLabelAlignment, LGraph, LLabel, LNodeKind, LPoint, LSize,
    LabelCellLayout, LabelSide, NodeId, PortId, PortRef, PortSide, VerticalLabelAlignment,
};

/// Ports `EndLabelPreprocessor.process` and `processNode` at commit `62d5909`.
///
/// Java visits every node in every layer, dummy nodes too. A north or south port dummy gathers
/// the end labels of the edges that were moved to it, so its margins grow like the margins of the
/// real node. Only a real node keeps its label cells for `EndLabelPostprocessor`, which skips
/// dummy nodes. Here the cell of a dummy node only changes its margin, then it is dropped.
///
/// Java leaves the labels on their edges. This crate moves the labels of a real node into its
/// ports instead, after every node has gathered its labels: a label that a north or south port
/// dummy and the real port both gather must still be on its edge when the second one looks.
pub fn preprocess_end_labels(graph: &mut LGraph) {
    let edge_label_spacing = graph.options.spacing.edge_label;
    let label_label_spacing = graph.options.spacing.label_label;
    let vertical_layout = graph.options.direction.is_vertical();
    let nodes = graph
        .layers()
        .iter()
        .flat_map(|layer| layer.nodes().iter().copied())
        .collect::<Vec<_>>();

    let mut moved_labels = Vec::new();
    for node in nodes {
        let gathered = gather_end_labels_for_node(graph, node);
        place_end_label_cells_for_node(
            graph,
            node,
            &gathered,
            label_label_spacing,
            edge_label_spacing,
            vertical_layout,
        );
        if matches!(
            graph.layerless_nodes[node].kind(),
            LNodeKind::Normal | LNodeKind::ExternalPort
        ) {
            copy_gathered_end_labels_into_ports(graph, node, &gathered);
            moved_labels.extend(gathered.labels.into_iter().flatten());
        } else {
            // The cell of a dummy only changed its margin. Its labels were never copied.
            for port in graph.layerless_nodes[node].ports.slots_mut() {
                port.end_label_cell = None;
            }
        }
    }
    remove_gathered_end_labels(graph, moved_labels);
}

/// Where one end label sits: the edge that holds it and its index in that edge's label list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EndLabelRef {
    pub(crate) edge: EdgeId,
    pub(crate) index: usize,
}

/// What `EndLabelPreprocessor.gatherLabels` finds at one port.
pub(crate) struct GatheredEndLabels {
    pub(crate) labels: Vec<EndLabelRef>,
    /// The greatest thickness of any edge at the port, the port's dummy included.
    max_edge_thickness: f64,
}

/// Ports `EndLabelPreprocessor.NO_INCIDENT_EDGE_THICKNESS`.
const NO_INCIDENT_EDGE_THICKNESS: f64 = -1.0;

/// Ports `EndLabelPreprocessor.gatherLabels(LPort)` at commit `62d5909`. `LabelSideSelector` uses
/// it too.
///
/// A north or south port gave its edges to a port dummy. The labels of those edges belong to the
/// port, so this also reads the ports of the dummy whose origin is this port (`PORT_DUMMY` and
/// `ORIGIN` in Java). Like Java, it sets `end_label_edge` of a label that has none, and it
/// returns `None` when no edge is connected.
pub(crate) fn gather_end_labels(
    graph: &mut LGraph,
    node: NodeId,
    port_index: PortId,
) -> Option<GatheredEndLabels> {
    let mut labels = Vec::new();
    let mut max_edge_thickness = gather_port_end_labels(graph, node, port_index, &mut labels);

    let dummy = graph.layerless_nodes[node].ports[port_index]
        .port_dummy
        .as_ref()
        .filter(|dummy| dummy.graph_id == graph.id)
        .map(|dummy| dummy.node);
    if let Some(dummy) = dummy {
        let origin = PortRef {
            node,
            port: port_index,
        };
        for position in 0..graph.layerless_nodes[dummy].ports.len() {
            let Some(dummy_port) = graph.layerless_nodes[dummy].ports.id_at(position) else {
                continue;
            };
            if graph.layerless_nodes[dummy].ports[dummy_port].origin_port == Some(origin) {
                max_edge_thickness = max_edge_thickness.max(gather_port_end_labels(
                    graph,
                    dummy,
                    dummy_port,
                    &mut labels,
                ));
            }
        }
    }

    (max_edge_thickness != NO_INCIDENT_EDGE_THICKNESS).then_some(GatheredEndLabels {
        labels,
        max_edge_thickness,
    })
}

/// Ports `EndLabelPreprocessor.gatherLabels(LPort, List)`: the tail labels of the outgoing edges
/// and the head labels of the incoming edges, edge by edge. Returns the greatest thickness of an
/// incident edge, or `NO_INCIDENT_EDGE_THICKNESS` when there is none.
fn gather_port_end_labels(
    graph: &mut LGraph,
    node: NodeId,
    port_index: PortId,
    target: &mut Vec<EndLabelRef>,
) -> f64 {
    let mut max_edge_thickness = NO_INCIDENT_EDGE_THICKNESS;
    let port = &graph.layerless_nodes[node].ports[port_index];
    // `LPort.getConnectedEdges`: the incoming edges, then the outgoing edges.
    let incident = port
        .incoming_edges()
        .iter()
        .chain(port.outgoing_edges().iter())
        .copied()
        .collect::<Vec<_>>();

    for edge in incident {
        max_edge_thickness = max_edge_thickness.max(graph.edges[edge].thickness);
        let source = graph.edges[edge].source();
        let placement = if source.node == node && source.port == port_index {
            EdgeLabelPlacement::Tail
        } else {
            EdgeLabelPlacement::Head
        };
        for (index, label) in graph.edges[edge].labels.iter_mut().enumerate() {
            if label.placement == placement {
                if label.end_label_edge.is_none() {
                    label.end_label_edge = Some(edge);
                }
                target.push(EndLabelRef { edge, index });
            }
        }
    }
    max_edge_thickness
}

/// What `gather_end_labels_for_node` found at the ports of one node.
pub(crate) struct NodeEndLabels {
    /// The greatest edge thickness per port, by `PortId::index`. Only a port that has labels
    /// reads it.
    max_edge_thickness: Vec<f64>,
    /// The end labels of each port, by `PortId::index`, in the order Java gathers them. They
    /// stay on their edges.
    pub(crate) labels: Vec<Vec<EndLabelRef>>,
}

/// Finds the end labels of each port of `node`, in the order Java gathers them, and clears what an
/// earlier run left at the ports. The labels stay on their edges, so a node that only needs the
/// size of its cells copies nothing. A real node copies them with
/// `copy_gathered_end_labels_into_ports`, and `remove_gathered_end_labels` takes the originals
/// off their edges once every node has gathered.
pub(crate) fn gather_end_labels_for_node(graph: &mut LGraph, node: NodeId) -> NodeEndLabels {
    let slot_count = graph.layerless_nodes[node].ports.slot_count();
    let mut result = NodeEndLabels {
        max_edge_thickness: vec![NO_INCIDENT_EDGE_THICKNESS; slot_count],
        labels: vec![Vec::new(); slot_count],
    };
    for position in 0..graph.layerless_nodes[node].ports.len() {
        let Some(port_index) = graph.layerless_nodes[node].ports.id_at(position) else {
            continue;
        };
        graph.layerless_nodes[node].ports[port_index]
            .end_labels
            .clear();
        graph.layerless_nodes[node].ports[port_index].end_label_cell = None;
        let Some(gathered) = gather_end_labels(graph, node, port_index) else {
            continue;
        };
        result.max_edge_thickness[port_index.index()] = gathered.max_edge_thickness;
        result.labels[port_index.index()] = gathered.labels;
    }
    result
}

/// Copies the gathered end labels of each port of `node` into the port.
pub(crate) fn copy_gathered_end_labels_into_ports(
    graph: &mut LGraph,
    node: NodeId,
    gathered: &NodeEndLabels,
) {
    for position in 0..graph.layerless_nodes[node].ports.len() {
        let Some(port_index) = graph.layerless_nodes[node].ports.id_at(position) else {
            continue;
        };
        let labels = &gathered.labels[port_index.index()];
        let copies = labels
            .iter()
            .map(|label| graph.edges[label.edge].labels[label.index].clone())
            .collect::<Vec<_>>();
        graph.layerless_nodes[node].ports[port_index].end_labels = copies;
    }
}

/// Takes the labels that ports now hold off their edges. The postprocessor puts them back.
pub(crate) fn remove_gathered_end_labels(graph: &mut LGraph, mut labels: Vec<EndLabelRef>) {
    // Highest index first, so a removal never shifts an index that is still to come.
    labels.sort_by_key(|label| std::cmp::Reverse((label.edge, label.index)));
    labels.dedup();
    for label in labels {
        graph.edges[label.edge].labels.remove(label.index);
    }
}

fn place_end_label_cells_for_node(
    graph: &mut LGraph,
    node: NodeId,
    gathered: &NodeEndLabels,
    label_label_spacing: f64,
    edge_label_spacing: f64,
    vertical_layout: bool,
) {
    for position in 0..graph.layerless_nodes[node].ports.len() {
        let Some(port_index) = graph.layerless_nodes[node].ports.id_at(position) else {
            continue;
        };
        let labels = &gathered.labels[port_index.index()];
        if labels.is_empty() {
            continue;
        }
        let labels_on_edges = || {
            labels
                .iter()
                .map(|label| &graph.edges[label.edge].labels[label.index])
        };
        let cell = create_end_label_cell(labels_on_edges(), label_label_spacing, vertical_layout);
        let label_side = labels_on_edges()
            .next()
            .and_then(|label| label.label_side)
            .unwrap_or(LabelSide::Below);
        graph.layerless_nodes[node].ports[port_index].end_label_cell = Some(cell);
        place_end_label_cell_for_port(
            graph,
            node,
            port_index,
            gathered.max_edge_thickness[port_index.index()],
            label_side,
            edge_label_spacing,
        );
    }

    // `EndLabelPreprocessor.process`: east and west in a vertical layout, else north and south.
    let sides = if vertical_layout {
        [PortSide::East, PortSide::West]
    } else {
        [PortSide::North, PortSide::South]
    };
    for side in sides {
        remove_end_label_overlaps(
            graph,
            node,
            side,
            2.0 * label_label_spacing,
            edge_label_spacing,
        );
    }

    update_end_label_node_margin(graph, node);
}

fn create_end_label_cell<'a>(
    labels: impl Iterator<Item = &'a LLabel>,
    label_label_spacing: f64,
    vertical_layout: bool,
) -> LabelCellLayout {
    let horizontal_layout = !vertical_layout;
    let size = label_cell_size(labels, label_label_spacing, horizontal_layout);
    LabelCellLayout {
        position: LPoint::default(),
        size,
        horizontal_layout,
        horizontal_alignment: HorizontalLabelAlignment::Center,
        vertical_alignment: VerticalLabelAlignment::Center,
    }
}

/// The minimum size of a label cell that holds `labels`.
///
/// Ports `LabelCell.addLabel` at commit `62d5909`, label by label and in Java's order of
/// additions, so the sum rounds the way Java's does. In horizontal layout mode a cell stacks its
/// labels one below the other; in vertical mode it puts them side by side.
fn label_cell_size<'a>(
    labels: impl Iterator<Item = &'a LLabel>,
    label_label_spacing: f64,
    horizontal_layout: bool,
) -> LSize {
    let mut size = LSize::default();
    for (index, label) in labels.enumerate() {
        grow_label_cell(
            &mut size,
            label.size,
            index + 1,
            label_label_spacing,
            horizontal_layout,
        );
    }
    size
}

fn place_end_label_cell_for_port(
    graph: &mut LGraph,
    node: NodeId,
    port_index: PortId,
    max_edge_thickness: f64,
    label_side: LabelSide,
    edge_label_spacing: f64,
) {
    let node_size = graph.layerless_nodes[node].size;
    let node_margin = graph.layerless_nodes[node].margin;
    let port = &graph.layerless_nodes[node].ports[port_index];
    let Some(mut cell) = port.end_label_cell else {
        return;
    };
    let port_anchor = LPoint {
        x: port.position.x + port.anchor.x,
        y: port.position.y + port.anchor.y,
    };
    let port_side = port.side;

    match port_side {
        PortSide::North => {
            cell.vertical_alignment = VerticalLabelAlignment::Bottom;
            cell.position.y = -node_margin.top - edge_label_spacing - cell.size.height;
            if label_side == LabelSide::Above {
                cell.horizontal_alignment = HorizontalLabelAlignment::Right;
                cell.position.x =
                    port_anchor.x - max_edge_thickness - edge_label_spacing - cell.size.width;
            } else {
                cell.horizontal_alignment = HorizontalLabelAlignment::Left;
                cell.position.x = port_anchor.x + max_edge_thickness + edge_label_spacing;
            }
        }
        PortSide::East => {
            cell.horizontal_alignment = HorizontalLabelAlignment::Left;
            cell.position.x = node_size.width + node_margin.right + edge_label_spacing;
            if label_side == LabelSide::Above {
                cell.vertical_alignment = VerticalLabelAlignment::Bottom;
                cell.position.y =
                    port_anchor.y - max_edge_thickness - edge_label_spacing - cell.size.height;
            } else {
                cell.vertical_alignment = VerticalLabelAlignment::Top;
                cell.position.y = port_anchor.y + max_edge_thickness + edge_label_spacing;
            }
        }
        PortSide::South => {
            cell.vertical_alignment = VerticalLabelAlignment::Top;
            cell.position.y = node_size.height + node_margin.bottom + edge_label_spacing;
            if label_side == LabelSide::Above {
                cell.horizontal_alignment = HorizontalLabelAlignment::Right;
                cell.position.x =
                    port_anchor.x - max_edge_thickness - edge_label_spacing - cell.size.width;
            } else {
                cell.horizontal_alignment = HorizontalLabelAlignment::Left;
                cell.position.x = port_anchor.x + max_edge_thickness + edge_label_spacing;
            }
        }
        PortSide::West => {
            cell.horizontal_alignment = HorizontalLabelAlignment::Right;
            cell.position.x = -node_margin.left - edge_label_spacing - cell.size.width;
            if label_side == LabelSide::Above {
                cell.vertical_alignment = VerticalLabelAlignment::Bottom;
                cell.position.y =
                    port_anchor.y - max_edge_thickness - edge_label_spacing - cell.size.height;
            } else {
                cell.vertical_alignment = VerticalLabelAlignment::Top;
                cell.position.y = port_anchor.y + max_edge_thickness + edge_label_spacing;
            }
        }
        PortSide::Undefined => {}
    }

    graph.layerless_nodes[node].ports[port_index].end_label_cell = Some(cell);
}

/// A label cell inside the strip overlap remover, in the remover's own coordinates.
///
/// Ports `RectangleStripOverlapRemover.RectangleNode` at commit `62d5909`.
struct StripRectangle {
    port: PortId,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    overlapping: Vec<usize>,
    placed: bool,
}

/// Ports `EndLabelPreprocessor.removeLabelOverlaps` and `calculateOverlapStartCoordinate` at
/// commit `62d5909`. It hands the label cells of one port side to `RectangleStripOverlapRemover`
/// with its default greedy strategy (`GreedyRectangleStripOverlapRemover`).
///
/// Only cells that overlap across the side (north and south: in x) are pushed apart. Two cells
/// side by side keep their place. Every cell is moved, even a lone one, as in Java.
fn remove_end_label_overlaps(
    graph: &mut LGraph,
    node: NodeId,
    port_side: PortSide,
    gap: f64,
    edge_label_spacing: f64,
) {
    let node_size = graph.layerless_nodes[node].size;
    let node_margin = graph.layerless_nodes[node].margin;
    let start = match port_side {
        PortSide::North => -node_margin.top - edge_label_spacing,
        PortSide::South => node_size.height + node_margin.bottom + edge_label_spacing,
        PortSide::East => node_size.width + node_margin.right + edge_label_spacing,
        PortSide::West => -node_margin.left - edge_label_spacing,
        PortSide::Undefined => return,
    };
    let sideways = matches!(port_side, PortSide::East | PortSide::West);

    // Import the rectangles (`importRectangle`): for east and west the strip axis is x, so the
    // rectangle is turned by 90 degrees. The values stay as `importRectangle` gives them,
    // including a `y` of 0 for east and west. The greedy pass sorts each cell's overlaps by `y`,
    // so these values reach the result.
    let mut rects = Vec::new();
    for (port_index, port) in graph.layerless_nodes[node].ports.iter_with_ids() {
        if port.side != port_side {
            continue;
        }
        let Some(cell) = port.end_label_cell else {
            continue;
        };
        let (x, y, width, height) = if sideways {
            (cell.position.y, 0.0, cell.size.height, cell.size.width)
        } else {
            (
                cell.position.x,
                cell.position.y,
                cell.size.width,
                cell.size.height,
            )
        };
        rects.push(StripRectangle {
            port: port_index,
            x,
            y,
            width,
            height,
            overlapping: Vec::new(),
            placed: false,
        });
    }
    if rects.is_empty() {
        return;
    }

    // removeOverlaps: a stable sort by left border (Java's List.sort, also stable in GWT: see
    // `crate::gwt::sort`; the key is a total order, so Rust's stable sort gives the same order).
    // `RectangleStripOverlapRemover.rectangleNodes` is an `ArrayList` that
    // `rectangleNodes.sort(compareLeftRectangleBorders)` sorts in place. The greedy remover
    // walks that same sorted list (`overlapRemover.getRectangleNodes()`). So `rects`, sorted
    // here, is also the order in which the cells are placed below.
    rects.sort_by(|a, b| a.x.total_cmp(&b.x));

    // computeOverlaps: scanline over the left borders. Java keeps the cells that touch the
    // scanline in a TreeSet ordered by right border, so a cell whose right border equals one
    // already in the set is not added to it. Touching borders count as overlapping.
    let mut intersecting: Vec<usize> = Vec::new();
    for current in 0..rects.len() {
        let scanline = rects[current].x;
        while let Some(&first) = intersecting.first() {
            if rects[first].x + rects[first].width < scanline {
                intersecting.remove(0);
            } else {
                break;
            }
        }
        for &other in &intersecting {
            rects[other].overlapping.push(current);
            rects[current].overlapping.push(other);
        }
        let right = rects[current].x + rects[current].width;
        if let Err(position) = intersecting
            .binary_search_by(|&other| (rects[other].x + rects[other].width).total_cmp(&right))
        {
            intersecting.insert(position, current);
        }
    }

    // GreedyRectangleStripOverlapRemover.removeOverlaps: lowest position that clears every
    // already placed cell it overlaps.
    for current in 0..rects.len() {
        let mut y_pos = 0.0;
        // Java sorts this list in place. Nothing reads it again, so a sorted copy is enough.
        let mut overlapping = std::mem::take(&mut rects[current].overlapping);
        overlapping.sort_by(|&a, &b| rects[a].y.total_cmp(&rects[b].y));
        for &other in &overlapping {
            if rects[other].placed
                && y_pos < rects[other].y + rects[other].height + gap
                && y_pos + rects[current].height + gap > rects[other].y
            {
                y_pos = rects[other].y + rects[other].height + gap;
            }
        }
        rects[current].y = y_pos;
        rects[current].placed = true;
    }

    // exportRectangle
    for rect in &rects {
        let Some(mut cell) = graph.layerless_nodes[node].ports[rect.port].end_label_cell else {
            continue;
        };
        match port_side {
            PortSide::North => cell.position.y = start - rect.height - rect.y,
            PortSide::South => cell.position.y = rect.y + start,
            PortSide::West => cell.position.x = start - rect.height - rect.y,
            PortSide::East => cell.position.x = start + rect.y,
            PortSide::Undefined => {}
        }
        graph.layerless_nodes[node].ports[rect.port].end_label_cell = Some(cell);
    }
}

/// Source: `EndLabelPreprocessor.updateNodeMargins`. ELK keeps the margin as a rectangle with
/// an x, a y, a width and a height, and reads the right and bottom margin back from the width
/// and height. This is not the same as reading them from the far corner.
fn update_end_label_node_margin(graph: &mut LGraph, node: NodeId) {
    // ELK skips nodes without end label cells.
    if graph.layerless_nodes[node]
        .ports
        .iter()
        .all(|port| port.end_label_cell.is_none())
    {
        return;
    }

    let node_size = graph.layerless_nodes[node].size;
    let node_margin = graph.layerless_nodes[node].margin;
    let mut min_x = -node_margin.left;
    let mut min_y = -node_margin.top;
    let mut width = node_margin.left + node_size.width + node_margin.right;
    let mut height = node_margin.top + node_size.height + node_margin.bottom;

    for port in &graph.layerless_nodes[node].ports {
        if let Some(cell) = port.end_label_cell {
            // ElkRectangle.union
            let x1 = min_x.min(cell.position.x);
            let y1 = min_y.min(cell.position.y);
            let x2 = (min_x + width).max(cell.position.x + cell.size.width);
            let y2 = (min_y + height).max(cell.position.y + cell.size.height);
            min_x = x1;
            min_y = y1;
            width = x2 - x1;
            height = y2 - y1;
        }
    }

    let margin = &mut graph.layerless_nodes[node].margin;
    margin.left = -min_x;
    margin.top = -min_y;
    margin.right = width - margin.left - node_size.width;
    margin.bottom = height - margin.top - node_size.height;
}
