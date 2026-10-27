//! Writes a finished layered graph back as coordinates of the input graph.
//!
//! This ports `ElkGraphLayoutTransferrer` and, ahead of it, the part of
//! `CompoundGraphPostprocessor` that joins the pieces of an edge that crosses hierarchy levels.
//! The result is a list of coordinates by id; [`crate::layout`] writes them into the
//! graph the caller gave.
//!
//! Source references, under `plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/`:
//! - `graph/transform/ElkGraphLayoutTransferrer.java`
//! - `compound/CompoundGraphPostprocessor.java`, `compound/CrossHierarchyEdge.java`,
//!   `compound/CrossHierarchyEdgeComparator.java`
//! - `graph/LGraphUtil.java`, `changeCoordSystem` and `isDescendant`

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::compound::compare_compound_segments;
use crate::error::Error;
use crate::graph::{GraphId, LGraph, LLabel, LNode, LNodeKind, LPoint, LSize, PortRef, PortSide};

/// `OrthogonalRoutingGenerator.TOLERANCE`.
const TOLERANCE: f64 = 1e-3;

/// What the transfer needs to know about the input graph.
pub(crate) struct TransferInput<'a> {
    /// The id of each input node of this run, with the id of its parent node.
    pub(crate) parents: &'a HashMap<String, String>,
    /// The ids of the nodes whose ELK size constraints are not empty. ELK writes the size of
    /// those nodes back.
    pub(crate) sized_nodes: &'a HashSet<String>,
    /// The ids of the input edges laid out in this run, each with the id of the node ELK
    /// chose to contain it.
    pub(crate) edge_containers: &'a HashMap<String, String>,
}

/// A node's new position, relative to its parent, and its size when ELK writes it.
#[derive(Debug, Clone)]
pub(crate) struct NodeLayout {
    pub(crate) id: String,
    pub(crate) position: LPoint,
    pub(crate) size: Option<LSize>,
    /// New positions by input port index, relative to the node.
    pub(crate) ports: Vec<(usize, LPoint)>,
    /// New positions and sizes by label index.
    pub(crate) labels: Vec<(usize, LPoint, LSize)>,
}

/// An edge's route and label positions, relative to its container.
#[derive(Debug, Clone)]
pub(crate) struct EdgeLayout {
    pub(crate) id: String,
    pub(crate) start: LPoint,
    pub(crate) bend_points: Vec<LPoint>,
    pub(crate) end: LPoint,
    /// Empty when ELK sets no junction points on the edge.
    pub(crate) junction_points: Vec<LPoint>,
    pub(crate) labels: Vec<(usize, LPoint, LSize)>,
}

/// Where a run put a port of the node it lays out, and what it decided about the port.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ScopePortLayout {
    /// The position of the port among the node's ports.
    pub(crate) index: usize,
    /// The port's new position, relative to the node.
    pub(crate) position: LPoint,
    pub(crate) side: PortSide,
    /// The border offset the dummy of the port used.
    pub(crate) border_offset: f64,
}

/// Everything one layered run computed.
#[derive(Debug, Clone)]
pub(crate) struct LayoutResult {
    pub(crate) nodes: Vec<NodeLayout>,
    pub(crate) edges: Vec<EdgeLayout>,
    /// The ports of the node the run lays out, when the graph has external ports.
    pub(crate) scope_ports: Vec<ScopePortLayout>,
    /// Whether the graph has external ports: `GraphProperties.EXTERNAL_PORTS`.
    pub(crate) external_ports: bool,
    /// `lgraph.getActualSize()` of the top graph: its size plus padding.
    pub(crate) actual_size: LSize,
}

/// One graph of the nested hierarchy and the node it lies in.
struct GraphEntry<'a> {
    graph: &'a LGraph,
    /// The graph and node index of the node this graph is inside.
    parent: Option<(usize, usize)>,
}

/// An `LNode` anywhere in the hierarchy: graph index and node index.
type NodeAt = (usize, usize);

/// A port anywhere in the hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PortAt {
    graph: usize,
    port: PortRef,
}

struct Hierarchy<'a> {
    graphs: Vec<GraphEntry<'a>>,
    /// For each graph and each of its nodes, the graph inside the node. Indexed like `graphs`.
    nested_graphs: Vec<Vec<Option<usize>>>,
    by_id: HashMap<GraphId, usize>,
    /// The graph of each container the caller names, by the caller's id of the node it is in.
    by_name: HashMap<&'a str, usize>,
}

impl<'a> Hierarchy<'a> {
    /// No ELK counterpart: indexes the nested graphs that ELK reaches through `LNode.getNestedGraph`.
    fn new(root: &'a LGraph) -> Self {
        let mut graphs = vec![GraphEntry {
            graph: root,
            parent: None,
        }];
        let mut nested_graphs = Vec::new();
        let mut cursor = 0;
        while cursor < graphs.len() {
            let graph = graphs[cursor].graph;
            let mut nested_here = vec![None; graph.layerless_nodes.len()];
            for (index, node) in graph.layerless_nodes.iter().enumerate() {
                if let Some(nested) = node.nested_graph.as_deref() {
                    nested_here[index] = Some(graphs.len());
                    graphs.push(GraphEntry {
                        graph: nested,
                        parent: Some((cursor, index)),
                    });
                }
            }
            nested_graphs.push(nested_here);
            cursor += 1;
        }
        let by_id = graphs
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.graph.id, index))
            .collect();
        let by_name = graphs
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.graph.name.as_str(), index))
            .collect();
        Self {
            graphs,
            nested_graphs,
            by_id,
            by_name,
        }
    }

    /// No ELK counterpart: an arena lookup.
    fn graph(&self, index: usize) -> &'a LGraph {
        self.graphs[index].graph
    }

    /// The graph inside a node, if any.
    ///
    /// Ports `LNode.getNestedGraph`.
    fn nested(&self, node: NodeAt) -> Option<usize> {
        self.nested_graphs[node.0][node.1]
    }

    /// Ports `LGraphUtil.isDescendant(child, parent)`: whether `child` lies, at any depth, inside
    /// `parent`.
    fn is_descendant(&self, child: NodeAt, parent: NodeAt) -> bool {
        let mut graph = child.0;
        while let Some(node) = self.graphs[graph].parent {
            if node == parent {
                return true;
            }
            graph = node.0;
        }
        false
    }

    /// `LPort.getAbsoluteAnchor`: the anchor relative to the port's graph.
    ///
    /// Ports `LPort.getAbsoluteAnchor`.
    fn absolute_anchor(&self, port: PortAt) -> LPoint {
        let node = &self.graph(port.graph).layerless_nodes[port.port.node];
        let data = &node.ports[port.port.port];
        // `KVector.sum(owner.getPosition(), this.getPosition(), anchor)`.
        LPoint {
            x: node.position.x + data.position.x + data.anchor.x,
            y: node.position.y + data.position.y + data.anchor.y,
        }
    }

    /// Ports `LGraphUtil.changeCoordSystem`.
    fn change_coord_system(&self, point: &mut LPoint, old: usize, new: usize) {
        if old == new {
            return;
        }
        let mut graph = old;
        loop {
            let data = self.graph(graph);
            point.x += data.offset.x;
            point.y += data.offset.y;
            let Some((parent_graph, parent_node)) = self.graphs[graph].parent else {
                break;
            };
            point.x += data.padding.left;
            point.y += data.padding.top;
            let node = &self.graph(parent_graph).layerless_nodes[parent_node];
            point.x += node.position.x;
            point.y += node.position.y;
            graph = parent_graph;
        }
        let mut graph = new;
        loop {
            let data = self.graph(graph);
            point.x -= data.offset.x;
            point.y -= data.offset.y;
            let Some((parent_graph, parent_node)) = self.graphs[graph].parent else {
                break;
            };
            point.x -= data.padding.left;
            point.y -= data.padding.top;
            let node = &self.graph(parent_graph).layerless_nodes[parent_node];
            point.x -= node.position.x;
            point.y -= node.position.y;
            graph = parent_graph;
        }
    }

    /// Ports `CrossHierarchyEdge.getActualSource` and `getActualTarget`: the port of an external
    /// port dummy is the port on the compound node it stands for.
    fn actual_end(&self, graph: usize, port: PortRef) -> PortAt {
        let node = &self.graph(graph).layerless_nodes[port.node];
        if node.kind == LNodeKind::ExternalPort
            && let Some(origin) = &node.origin_port
            && let Some(&origin_graph) = self.by_id.get(&origin.graph_id)
        {
            return PortAt {
                graph: origin_graph,
                port: origin.port,
            };
        }
        PortAt { graph, port }
    }
}

/// An original edge as ELK has it after `CompoundGraphPostprocessor`: its real source and
/// target ports, and its bend points, labels and target offset in the reference graph.
struct JoinedEdge {
    source: PortAt,
    target: PortAt,
    bend_points: Vec<LPoint>,
    junction_points: Vec<LPoint>,
    labels: Vec<LLabel>,
    target_offset: LPoint,
}

/// Joins the pieces of each edge that crosses a hierarchy level.
///
/// Ports `CompoundGraphPostprocessor.process`, with `CrossHierarchyEdgeComparator.compare`.
fn join_cross_hierarchy_edges(hierarchy: &Hierarchy<'_>) -> BTreeMap<String, JoinedEdge> {
    // Pieces by original edge id, in hierarchy order.
    let mut pieces: BTreeMap<&str, Vec<(usize, &crate::graph::CrossHierarchyEdge)>> =
        BTreeMap::new();
    for (graph_index, entry) in hierarchy.graphs.iter().enumerate() {
        for record in &entry.graph.cross_hierarchy_edges {
            let graph = hierarchy
                .by_id
                .get(&record.graph_id)
                .copied()
                .unwrap_or(graph_index);
            pieces
                .entry(record.original_edge_id.as_str())
                .or_default()
                .push((graph, record));
        }
    }
    let unnecessary_bendpoints = hierarchy.graph(0).options.unnecessary_bendpoints;

    let mut joined = BTreeMap::new();
    for (id, mut list) in pieces {
        // A stable sort, like `Collections.sort`.
        list.sort_by(|a, b| compare_compound_segments(a.1.segment, b.1.segment));
        let edge_of = |(graph, record): &(usize, &crate::graph::CrossHierarchyEdge)| {
            (*graph, &hierarchy.graph(*graph).edges[record.edge])
        };
        let (first_graph, first_edge) = edge_of(&list[0]);
        let (last_graph, last_edge) = edge_of(&list[list.len() - 1]);
        let source = hierarchy.actual_end(first_graph, first_edge.source);
        let target = hierarchy.actual_end(last_graph, last_edge.target);

        let reference_node = (source.graph, source.port.node);
        let target_node = (target.graph, target.port.node);
        let reference = if hierarchy.is_descendant(target_node, reference_node) {
            hierarchy.nested(reference_node).unwrap_or(source.graph)
        } else {
            source.graph
        };

        let mut bend_points = Vec::new();
        let mut junction_points = Vec::new();
        let mut labels = Vec::new();
        let mut target_offset = LPoint::default();
        let mut last_point: Option<LPoint> = None;
        for piece in &list {
            let (graph, edge) = edge_of(piece);
            let mut offset = LPoint::default();
            hierarchy.change_coord_system(&mut offset, graph, reference);

            let piece_bends: Vec<LPoint> = edge
                .bend_points
                .iter()
                .map(|p| LPoint {
                    x: p.x + offset.x,
                    y: p.y + offset.y,
                })
                .collect();
            let mut source_point = hierarchy.absolute_anchor(PortAt {
                graph,
                port: edge.source,
            });
            source_point.x += offset.x;
            source_point.y += offset.y;
            let mut target_point = hierarchy.absolute_anchor(PortAt {
                graph,
                port: edge.target,
            });
            target_point.x += offset.x;
            target_point.y += offset.y;

            if let Some(last) = last_point {
                let next = piece_bends.first().copied().unwrap_or(target_point);
                let x_diff = (last.x - next.x).abs() > TOLERANCE;
                let y_diff = (last.y - next.y).abs() > TOLERANCE;
                if (!unnecessary_bendpoints && x_diff && y_diff)
                    || (unnecessary_bendpoints && (x_diff || y_diff))
                {
                    bend_points.push(source_point);
                }
            }
            bend_points.extend(piece_bends.iter().copied());
            last_point = Some(piece_bends.last().copied().unwrap_or(source_point));

            // `copyJunctionPoints`: the junction points of each piece, moved to the reference graph.
            junction_points.extend(edge.junction_points.iter().map(|p| LPoint {
                x: p.x + offset.x,
                y: p.y + offset.y,
            }));

            if hierarchy.actual_end(graph, edge.target) == target {
                let target_graph = target.graph;
                if target_graph != graph {
                    offset = LPoint::default();
                    hierarchy.change_coord_system(&mut offset, target_graph, reference);
                }
                target_offset = offset;
            }

            // `copyLabelsBack`: only the labels of this original edge, moved from the
            // coordinates of the piece's source graph.
            let label_graph = graph;
            for label in &edge.labels {
                if label.original_label_edge.as_deref() != Some(id) {
                    continue;
                }
                let mut label = label.clone();
                hierarchy.change_coord_system(&mut label.position, label_graph, reference);
                labels.push(label);
            }
        }
        joined.insert(
            id.to_string(),
            JoinedEdge {
                source,
                target,
                bend_points,
                junction_points,
                labels,
                target_offset,
            },
        );
    }
    joined
}

/// An edge to apply: an ordinary edge of one graph, or a joined cross-hierarchy edge.
struct EdgeToApply<'a> {
    id: &'a str,
    source: PortAt,
    target: PortAt,
    bend_points: Vec<LPoint>,
    junction_points: Vec<LPoint>,
    labels: Vec<LLabel>,
    target_offset: Option<LPoint>,
}

/// Ports `ElkGraphLayoutTransferrer.applyLayout`, for the whole hierarchy.
pub(crate) fn apply_layout(
    root: &mut LGraph,
    input: &TransferInput<'_>,
) -> Result<LayoutResult, Error> {
    // The external ports of the top graph are placed first. This moves their dummies, which
    // the edges that end at them read.
    let mut scope_ports = Vec::new();
    for index in 0..root.layerless_nodes.len() {
        let Some(port) = root.layerless_nodes[index].scope_port else {
            continue;
        };
        // The port keeps its size, and every step of the run is over: no turn of the graph is left.
        let size = root.layerless_nodes[index].external_port_size;
        let position = crate::pipeline::external_port_position(root, index, size);
        let dummy = &root.layerless_nodes[index];
        scope_ports.push(ScopePortLayout {
            index: port,
            position,
            side: dummy.external_port_side,
            border_offset: dummy
                .ports
                .first()
                .and_then(|port| port.border_offset)
                .unwrap_or(0.0),
        });
    }
    let external_ports = root.graph_properties.external_ports;
    let root: &LGraph = root;
    let hierarchy = Hierarchy::new(root);
    let joined = join_cross_hierarchy_edges(&hierarchy);

    // Every edge in the graph where its source node is, as ELK collects them from the source
    // node's ports. ELK skips an edge into a descendant of its source; the graph inside the
    // source picks it up instead.
    let mut by_graph: Vec<Vec<EdgeToApply<'_>>> =
        (0..hierarchy.graphs.len()).map(|_| Vec::new()).collect();
    for (graph_index, entry) in hierarchy.graphs.iter().enumerate() {
        for (edge_index, edge) in entry.graph.edges.iter().enumerate() {
            if !is_original_edge(entry.graph, edge_index, input) {
                continue;
            }
            by_graph[graph_index].push(EdgeToApply {
                id: edge.id.as_str(),
                source: PortAt {
                    graph: graph_index,
                    port: edge.source,
                },
                target: PortAt {
                    graph: graph_index,
                    port: edge.target,
                },
                bend_points: edge.bend_points.clone(),
                junction_points: edge.junction_points.clone(),
                labels: edge.labels.clone(),
                target_offset: None,
            });
        }
    }
    for (id, edge) in &joined {
        if !input.edge_containers.contains_key(id) {
            continue;
        }
        let source_node = (edge.source.graph, edge.source.port.node);
        let target_node = (edge.target.graph, edge.target.port.node);
        let graph = if hierarchy.is_descendant(target_node, source_node) {
            hierarchy.nested(source_node).unwrap_or(edge.source.graph)
        } else {
            edge.source.graph
        };
        by_graph[graph].push(EdgeToApply {
            id: id.as_str(),
            source: edge.source,
            target: edge.target,
            bend_points: edge.bend_points.clone(),
            junction_points: edge.junction_points.clone(),
            labels: edge.labels.clone(),
            target_offset: Some(edge.target_offset),
        });
    }

    let mut result = LayoutResult {
        nodes: Vec::new(),
        edges: Vec::new(),
        scope_ports,
        external_ports,
        actual_size: actual_size(root),
    };
    for (graph_index, edges) in by_graph.into_iter().enumerate() {
        apply_graph_layout(&hierarchy, graph_index, edges, input, &mut result)?;
    }
    Ok(result)
}

/// Whether a layered edge is an input edge as the layout left it, not a dummy edge.
///
/// Ports `ElkGraphLayoutTransferrer.applyEdgeLayout`'s `ORIGIN` check: only input edges are written.
fn is_original_edge(graph: &LGraph, index: usize, input: &TransferInput<'_>) -> bool {
    let edge = &graph.edges[index];
    if edge.compound_segment.is_some() || !input.edge_containers.contains_key(&edge.id) {
        return false;
    }
    let attached = |port: PortRef, outgoing: bool| {
        graph
            .layerless_nodes
            .get(port.node)
            .and_then(|node| node.ports.get(port.port))
            .is_some_and(|data| {
                let list = if outgoing {
                    &data.outgoing_edges
                } else {
                    &data.incoming_edges
                };
                list.contains(&index)
            })
    };
    attached(edge.source, true)
        && attached(edge.target, false)
        && is_input_end(&graph.layerless_nodes[edge.source.node])
        && is_input_end(&graph.layerless_nodes[edge.target.node])
}

/// Whether a node stands for something the input has: a node, or a port of the graph's own node.
fn is_input_end(node: &LNode) -> bool {
    node.kind == LNodeKind::Normal
        || (node.kind == LNodeKind::ExternalPort && node.scope_port.is_some())
}

/// `LGraph.getActualSize`.
///
/// Ports `LGraph.getActualSize`.
fn actual_size(graph: &LGraph) -> LSize {
    LSize {
        width: graph.size.width + graph.padding.left + graph.padding.right,
        height: graph.size.height + graph.padding.top + graph.padding.bottom,
    }
}

/// Ports `ElkGraphLayoutTransferrer.applyLayout` for one graph, without the recursion and without
/// `applyParentNodeLayout`, which [`crate::layout`] runs on the input node.
fn apply_graph_layout(
    hierarchy: &Hierarchy<'_>,
    graph_index: usize,
    edges: Vec<EdgeToApply<'_>>,
    input: &TransferInput<'_>,
    result: &mut LayoutResult,
) -> Result<(), Error> {
    let graph = hierarchy.graph(graph_index);
    let offset = LPoint {
        x: graph.offset.x + graph.padding.left,
        y: graph.offset.y + graph.padding.top,
    };
    for (index, node) in graph.layerless_nodes.iter().enumerate() {
        if node.kind != LNodeKind::Normal || !input.parents.contains_key(&node.id) {
            continue;
        }
        result
            .nodes
            .push(node_layout(hierarchy, (graph_index, index), offset, input));
    }
    for edge in edges {
        result
            .edges
            .push(edge_layout(hierarchy, edge, offset, input)?);
    }
    Ok(())
}

/// Ports `ElkGraphLayoutTransferrer.applyNodeLayout`.
fn node_layout(
    hierarchy: &Hierarchy<'_>,
    at: NodeAt,
    offset: LPoint,
    input: &TransferInput<'_>,
) -> NodeLayout {
    let node = &hierarchy.graph(at.0).layerless_nodes[at.1];
    let sized = input.sized_nodes.contains(&node.id);
    let size = (sized || node.nested_graph.is_some()).then_some(node.size);
    let node_has_placement = !node.node_label_placement.is_fixed();
    let labels = node
        .labels
        .iter()
        .filter(|label| {
            node_has_placement || label.node_label_placement.is_some_and(|p| !p.is_fixed())
        })
        .filter_map(|label| {
            label
                .source_index
                .map(|index| (index, label.position, label.size))
        })
        .collect();
    // `elkport.setLocation(lport.getPosition().x, lport.getPosition().y)` for each port that
    // came from the input.
    let ports = node
        .ports
        .iter()
        .filter_map(|port| port.input_index.map(|index| (index, port.position)))
        .collect();
    NodeLayout {
        id: node.id.clone(),
        position: LPoint {
            x: node.position.x + offset.x,
            y: node.position.y + offset.y,
        },
        size,
        ports,
        labels,
    }
}

/// Ports `ElkGraphLayoutTransferrer.applyEdgeLayout` and `calculateHierarchicalOffset`.
fn edge_layout(
    hierarchy: &Hierarchy<'_>,
    edge: EdgeToApply<'_>,
    offset: LPoint,
    input: &TransferInput<'_>,
) -> Result<EdgeLayout, Error> {
    let source_node = (edge.source.graph, edge.source.port.node);
    let target_node = (edge.target.graph, edge.target.port.node);

    let mut edge_offset = offset;
    let hierarchical = hierarchical_offset(hierarchy, source_node, target_node, edge.id, input)?;
    edge_offset.x += hierarchical.x;
    edge_offset.y += hierarchical.y;

    let source_point = if hierarchy.is_descendant(target_node, source_node) {
        // The port's anchor relative to its node, less the offset added below.
        let node = &hierarchy.graph(edge.source.graph).layerless_nodes[source_node.1];
        let port = &node.ports[edge.source.port.port];
        LPoint {
            x: port.position.x + port.anchor.x - offset.x,
            y: port.position.y + port.anchor.y - offset.y,
        }
    } else {
        hierarchy.absolute_anchor(edge.source)
    };
    let mut target_point = hierarchy.absolute_anchor(edge.target);
    if let Some(target_offset) = edge.target_offset {
        target_point.x += target_offset.x;
        target_point.y += target_offset.y;
    }
    let shift = |p: LPoint| LPoint {
        x: p.x + edge_offset.x,
        y: p.y + edge_offset.y,
    };
    let labels = edge
        .labels
        .iter()
        .filter_map(|label| {
            label.source_index.map(|index| {
                (
                    index,
                    LPoint {
                        x: label.position.x + edge_offset.x,
                        y: label.position.y + edge_offset.y,
                    },
                    label.size,
                )
            })
        })
        .collect();
    Ok(EdgeLayout {
        id: edge.id.to_string(),
        start: shift(source_point),
        bend_points: edge.bend_points.iter().copied().map(shift).collect(),
        end: shift(target_point),
        // `junctionPoints.offset(edgeOffset)`.
        junction_points: edge.junction_points.iter().copied().map(shift).collect(),
        labels,
    })
}

/// The offset from the coordinates of the edge's source graph to those of the graph ELK
/// expects them in.
///
/// Ports `ElkGraphLayoutTransferrer.calculateHierarchicalOffset`, and
/// `ElkGraphImporter.findCoordinateSystemOrigin`, which sets the target graph at import.
fn hierarchical_offset(
    hierarchy: &Hierarchy<'_>,
    source: NodeAt,
    target: NodeAt,
    edge_id: &str,
    input: &TransferInput<'_>,
) -> Result<LPoint, Error> {
    // Siblings, or an edge into a descendant of its source, need no change.
    if source.0 == target.0 || hierarchy.is_descendant(target, source) {
        return Ok(LPoint::default());
    }
    // Otherwise the coordinates belong to the edge's container: the root graph, or the graph
    // inside the container's node.
    // Every edge written has a container, and the container's graph is in the hierarchy. A
    // miss would put the edge at the wrong place, so it is an error, not a zero offset.
    let Some(container) = input.edge_containers.get(edge_id) else {
        return Err(Error::Internal {
            message: format!("edge \"{edge_id}\" has no container"),
        });
    };
    let Some(&target_graph) = hierarchy.by_name.get(container.as_str()) else {
        return Err(Error::Internal {
            message: format!("the container \"{container}\" of edge \"{edge_id}\" has no graph"),
        });
    };
    let mut result = LPoint::default();
    let mut current = source.0;
    while current != target_graph {
        let Some((parent_graph, parent_node)) = hierarchy.graphs[current].parent else {
            break;
        };
        let node = &hierarchy.graph(parent_graph).layerless_nodes[parent_node];
        let graph = hierarchy.graph(parent_graph);
        // `result.add(position).add(offset).add(padding.left, padding.top)`.
        result.x += node.position.x;
        result.y += node.position.y;
        result.x += graph.offset.x;
        result.y += graph.offset.y;
        result.x += graph.padding.left;
        result.y += graph.padding.top;
        current = parent_graph;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::LNode;
    use crate::options::LayeredOptions;

    /// A root with two nodes, each holding a graph. A point in one of them has no ancestor
    /// link to a point in the other, so the offset between them needs the edge's container.
    fn two_nested_graphs() -> LGraph {
        let mut root = LGraph::new("root", LayeredOptions::default());
        for id in ["a", "b"] {
            let mut node = LNode::new(id, 10.0, 10.0, None);
            node.nested_graph = Some(Box::new(LGraph::new(id, LayeredOptions::default())));
            root.layerless_nodes.push(node);
        }
        root
    }

    fn offset_error(containers: &HashMap<String, String>) -> Option<Error> {
        let root = two_nested_graphs();
        let hierarchy = Hierarchy::new(&root);
        let parents = HashMap::new();
        let sized_nodes = HashSet::new();
        let input = TransferInput {
            parents: &parents,
            sized_nodes: &sized_nodes,
            edge_containers: containers,
        };
        hierarchical_offset(&hierarchy, (1, 0), (2, 0), "e", &input).err()
    }

    // Mutation: leave out `copyJunctionPoints` in `join_cross_hierarchy_edges`. The joined edge
    // then has no junction points.
    #[test]
    fn the_junction_points_of_an_edge_across_levels_are_joined_and_moved_to_one_graph() {
        use crate::importer::{ElkInputGraph, import_graph};
        use crate::options::{ElkDirection, HierarchyHandling};
        use crate::snapshot_tests::{edge, node};

        let mut group = node("group", 40.0, 30.0, None);
        group.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let input = ElkInputGraph {
            id: "root".to_string(),
            options: LayeredOptions::flowchart_test_defaults(ElkDirection::Right),
            nodes: vec![
                group,
                node("a", 40.0, 30.0, Some("group")),
                node("b", 40.0, 30.0, None),
            ],
            edges: vec![edge("e", "a", "b")],
        };
        let mut graph = import_graph(&input).expect("imports");
        crate::pipeline::execute_ported_compound_processors(&mut graph).expect("lays out");
        // Give every piece of the edge a junction point, as a router would.
        fn mark(graph: &mut LGraph) {
            for edge in &mut graph.edges {
                edge.junction_points = vec![LPoint { x: 1.0, y: 2.0 }];
            }
            for node in &mut graph.layerless_nodes {
                if let Some(nested) = node.nested_graph.as_deref_mut() {
                    mark(nested);
                }
            }
        }
        mark(&mut graph);

        let parents = HashMap::new();
        let sized_nodes = HashSet::new();
        let containers = HashMap::from([("e".to_string(), "root".to_string())]);
        let input = TransferInput {
            parents: &parents,
            sized_nodes: &sized_nodes,
            edge_containers: &containers,
        };
        let result = apply_layout(&mut graph, &input).expect("transfers");
        let edge = result
            .edges
            .iter()
            .find(|edge| edge.id == "e")
            .expect("the edge");
        // One junction point per piece. The pieces are in two graphs, so they land apart.
        assert_eq!(edge.junction_points.len(), 2);
        assert_ne!(edge.junction_points[0], edge.junction_points[1]);
    }

    // Mutation: return `Ok(LPoint::default())` when the edge has no container.
    #[test]
    fn an_edge_without_a_container_is_an_internal_error() {
        let error = offset_error(&HashMap::new()).expect("an error");
        assert!(matches!(error, Error::Internal { ref message } if message.contains("\"e\"")));
    }

    // Mutation: return `Ok(LPoint::default())` when the container has no graph.
    #[test]
    fn a_container_without_a_graph_is_an_internal_error() {
        let containers = HashMap::from([("e".to_string(), "nowhere".to_string())]);
        let error = offset_error(&containers).expect("an error");
        assert!(matches!(error, Error::Internal { ref message } if message.contains("nowhere")));
    }

    // Mutation: return an error whenever the container is found.
    #[test]
    fn a_known_container_gives_no_error() {
        let containers = HashMap::from([("e".to_string(), "root".to_string())]);
        assert!(offset_error(&containers).is_none());
    }
}
