//! The public entry point: an ELK graph in, the same graph with positions out.
//!
//! [`layout`] does what elkjs does with a graph: it resolves the options, lays out each part
//! of the hierarchy that ELK lays out on its own (children before their parents), and writes
//! the results back.
//!
//! Source references:
//! - `org.eclipse.elk.core/src/org/eclipse/elk/core/RecursiveGraphLayoutEngine.java`,
//!   `layoutRecursively` and `evaluateHierarchyHandlingInheritance`
//! - `org.eclipse.elk.graph/src/org/eclipse/elk/graph/util/ElkGraphUtil.java`,
//!   `findBestEdgeContainment`
//! - `org.eclipse.elk.core/src/org/eclipse/elk/core/util/ElkUtil.java`, `resizeNode`
//! - `org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/LayeredLayoutProvider.java`
//! - `org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/graph/transform/
//!   ElkGraphLayoutTransferrer.java`, `applyParentNodeLayout`

use std::collections::{HashMap, HashSet, VecDeque};

use crate::api::{ElkEdge, ElkEdgeSection, ElkLabel, ElkNode, ElkPoint, ElkPort};
use crate::common::networksimplex::NetworkSimplexError;
use crate::components::ComponentError;
use crate::compound::calc_port_side;
use crate::error::Error;
use crate::export::{self, LayoutResult, TransferInput};
use crate::graph::{DummyLabelSpacings, LPoint, LSize, PortSide, port_side_from_direction};
use crate::importer::{
    self, ElkInputEdge, ElkInputGraph, ElkInputLabel, ElkInputNode, ElkInputPort, ParentSettings,
};
use crate::layout_options::{
    self, EdgeSettings, LabelSettings, NodeSettings, OptionError, OptionProblem, PortSettings,
};
use crate::options::{
    ElkDirection, HierarchyHandling, LayeredOptions, PortConstraints, PortLabelPlacement,
    SizeConstraints,
};
use crate::phase::layering::LayeringError;
use crate::pipeline::{self, PipelineError};

/// Lays out an ELK graph the way elkjs 0.9.3 does with the layered algorithm.
///
/// The graph is given as elkjs reads it from JSON. The result is the same graph with every
/// position filled in, as elkjs returns it: nodes relative to their parent node, edge routes
/// and edge labels relative to the node that contains the edge, which
/// [`ElkEdge::container`] names.
///
/// # Errors
///
/// Returns an [`Error`] for a graph elk-layered cannot lay out exactly as elkjs does: one
/// with a label on a port of a node with children, a node with children that sizes itself to
/// its ports, an edge between a port of the root and a node inside it, an option elk-layered
/// does not support or cannot read, an id used twice or not found, or a size that is no finite,
/// non-negative number.
///
/// Ports `RecursiveGraphLayoutEngine.layout` with the JSON round trip of elkjs's `JsonImporter`.
pub fn layout(graph: &ElkNode) -> Result<ElkNode, Error> {
    let tree = Tree::new(graph)?;
    let mut work = Work::new(graph, &tree);
    work.layout_recursively(0)?;
    Ok(work.finish())
}

/// What the edges at one port say about it, as ELK counts them over the whole graph.
#[derive(Debug, Clone, Copy, Default)]
struct PortFacts {
    /// Ports `ElkGraphImporter.calculateNetFlow`.
    net_flow: i32,
    /// How many edges at the port connect to a child of its node, at most two.
    child_edges: usize,
    /// Ports `ElkGraphImporter.isConnectedToExternalNodes`.
    connected_to_external_nodes: bool,
    /// Whether an edge at the port leads to or comes from a descendant of its node.
    inside_connections: bool,
    /// Whether any edge starts or ends at the port: what `ElkPortAdapter` shows the cells.
    has_edges: bool,
}

/// One node of the input, with what `layout` reads from it.
struct NodeInfo<'a> {
    node: &'a ElkNode,
    parent: Option<usize>,
    children: Vec<usize>,
    /// Indices into the node's `children` from the root, to find it in the output.
    path: Vec<usize>,
    settings: NodeSettings,
    labels: Vec<LabelSettings>,
    /// The options of each port of `node`, in order.
    ports: Vec<PortSettings>,
    /// The options of the labels of each port of `node`, in order.
    port_labels: Vec<Vec<LabelSettings>>,
    /// What the edges say about each port of `node`, in order.
    port_facts: Vec<PortFacts>,
}

/// One edge of the input.
struct EdgeInfo<'a> {
    edge: &'a ElkEdge,
    /// The node whose `edges` list holds it, and its index there.
    given_in: usize,
    index: usize,
    source: usize,
    target: usize,
    /// The index in the source node's `ports`, when the edge starts at a port.
    source_port: Option<usize>,
    /// The index in the target node's `ports`, when the edge ends at a port.
    target_port: Option<usize>,
    /// The node ELK moves the edge to: `ElkGraphUtil.findBestEdgeContainment`.
    container: usize,
    settings: EdgeSettings,
    labels: Vec<LabelSettings>,
}

struct Tree<'a> {
    /// In preorder; the root is 0.
    nodes: Vec<NodeInfo<'a>>,
    /// In the order elkjs's `JsonImporter.transformEdges` reads them: a node's edges, then
    /// those of its children, depth first.
    edges: Vec<EdgeInfo<'a>>,
    /// The index in `nodes` of each node id.
    by_id: HashMap<&'a str, usize>,
    /// For each node, the indices in `edges` of the edges it contains, in ascending order.
    edges_by_container: Vec<Vec<usize>>,
}

/// No ELK counterpart: names a node in an error.
fn node_element(id: &str) -> String {
    format!("node \"{id}\"")
}

/// No ELK counterpart: names a port in an error.
fn port_element(id: &str) -> String {
    format!("port \"{id}\"")
}

/// No ELK counterpart: names an edge in an error.
fn edge_element(id: &str) -> String {
    format!("edge \"{id}\"")
}

/// No ELK counterpart: turns a refused option into an `Error`.
fn option_error(element: String, error: OptionError) -> Error {
    match error.problem {
        OptionProblem::Unknown => Error::UnknownOption {
            element,
            key: error.key,
        },
        OptionProblem::Unsupported(reason) => Error::UnsupportedOption {
            element,
            key: error.key,
            value: error.value,
            reason: reason.to_string(),
        },
        OptionProblem::InvalidValue => Error::InvalidOptionValue {
            element,
            key: error.key,
            value: error.value,
        },
        OptionProblem::Duplicate { other } => Error::DuplicateOption {
            element,
            key: error.key,
            other,
        },
    }
}

/// No ELK counterpart: elkjs reads a NaN or infinite number as zero, elk-layered refuses it.
fn check_number(element: &str, field: &'static str, value: f64, size: bool) -> Result<(), Error> {
    if !value.is_finite() || (size && value < 0.0) {
        return Err(Error::InvalidSize {
            element: element.to_string(),
            field,
            value,
        });
    }
    Ok(())
}

/// No ELK counterpart: checks and reads the labels of one element.
fn label_settings(labels: &[ElkLabel], owner: &str) -> Result<Vec<LabelSettings>, Error> {
    labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let element = format!("label {index} of {owner}");
            check_number(&element, "x", label.x, false)?;
            check_number(&element, "y", label.y, false)?;
            check_number(&element, "width", label.width, true)?;
            check_number(&element, "height", label.height, true)?;
            layout_options::label_settings(&label.layout_options)
                .map_err(|error| option_error(element, error))
        })
        .collect()
}

/// No ELK counterpart: checks and reads one port and its labels.
fn port_settings(port: &ElkPort) -> Result<(PortSettings, Vec<LabelSettings>), Error> {
    let element = port_element(&port.id);
    check_number(&element, "x", port.x, false)?;
    check_number(&element, "y", port.y, false)?;
    check_number(&element, "width", port.width, true)?;
    check_number(&element, "height", port.height, true)?;
    let settings = layout_options::port_settings(&port.layout_options)
        .map_err(|error| option_error(element.clone(), error))?;
    let labels = label_settings(&port.labels, &element)?;
    Ok((settings, labels))
}

/// The `feature` of the refusal for an inside self loop in a graph laid out on its own: ELK's
/// importer looks for the loop in the run of its node and finds no end for it:
/// `UnsupportedGraphException`.
const INSIDE_SELF_LOOP_ALONE: &str = "an inside self loop in a graph laid out on its own";

/// The `feature` of the refusal for an edge end that ELK's importer cannot find, such as an edge
/// between a node and its own children when the children are laid out on their own:
/// `UnsupportedGraphException`.
const UNKNOWN_EDGE_END: &str = "an edge between a node and its own children laid out on their own";

/// The most levels a node may sit below the root. ELK has no such limit; Rust's stack needs one.
/// 256 leaves a wide margin on every target, and is twice what serde_json reads by default.
const MAX_NESTING_DEPTH: usize = 256;

impl<'a> Tree<'a> {
    /// Ports `JsonImporter.transform`: nodes in JSON order, then edges depth first, each moved by `ElkGraphUtil.updateContainment`.
    fn new(root: &'a ElkNode) -> Result<Self, Error> {
        let mut nodes: Vec<NodeInfo<'a>> = Vec::new();
        let mut by_id: HashMap<&'a str, usize> = HashMap::new();
        let mut port_by_id: HashMap<&'a str, (usize, usize)> = HashMap::new();
        // Preorder, so a node's index is smaller than its children's.
        let mut stack: Vec<(&'a ElkNode, Option<usize>, Vec<usize>)> =
            vec![(root, None, Vec::new())];
        while let Some((node, parent, path)) = stack.pop() {
            let element = node_element(&node.id);
            // The graph types clone and drop their children recursively, and a debug build in
            // a 2 MB thread overflows its stack at about 1600 levels on Linux x86-64. Refuse a
            // graph far deeper than any diagram, with an error, rather than abort.
            if path.len() > MAX_NESTING_DEPTH {
                return Err(Error::Unsupported {
                    element,
                    feature: format!("a graph nested deeper than {MAX_NESTING_DEPTH} levels"),
                });
            }
            // elkjs keeps `nodeIdMap`, `portIdMap` and `edgeIdMap` apart, so a node and a port may
            // share an id.
            if by_id.insert(node.id.as_str(), nodes.len()).is_some() {
                return Err(Error::DuplicateId {
                    id: node.id.clone(),
                });
            }
            check_number(&element, "x", node.x, false)?;
            check_number(&element, "y", node.y, false)?;
            check_number(&element, "width", node.width, true)?;
            check_number(&element, "height", node.height, true)?;
            let settings = layout_options::node_settings(&node.layout_options)
                .map_err(|error| option_error(element.clone(), error))?;
            let labels = label_settings(&node.labels, &element)?;
            let node_index = nodes.len();
            let mut ports = Vec::with_capacity(node.ports.len());
            let mut port_labels = Vec::with_capacity(node.ports.len());
            for (port_position, port) in node.ports.iter().enumerate() {
                if port_by_id
                    .insert(port.id.as_str(), (node_index, port_position))
                    .is_some()
                {
                    return Err(Error::DuplicateId {
                        id: port.id.clone(),
                    });
                }
                let (settings, labels) = port_settings(port)?;
                ports.push(settings);
                port_labels.push(labels);
            }
            let index = node_index;
            if let Some(parent) = parent {
                nodes[parent].children.push(index);
            }
            nodes.push(NodeInfo {
                node,
                parent,
                children: Vec::new(),
                path: path.clone(),
                settings,
                labels,
                port_facts: vec![PortFacts::default(); ports.len()],
                ports,
                port_labels,
            });
            for (child_index, child) in node.children.iter().enumerate().rev() {
                let mut child_path = path.clone();
                child_path.push(child_index);
                stack.push((child, Some(index), child_path));
            }
        }

        let mut edges = Vec::new();
        let mut edge_ids = HashSet::new();
        for (given_in, info) in nodes.iter().enumerate() {
            for (index, edge) in info.node.edges.iter().enumerate() {
                let element = edge_element(&edge.id);
                if !edge_ids.insert(edge.id.as_str()) {
                    return Err(Error::DuplicateId {
                        id: edge.id.clone(),
                    });
                }
                if edge.sources.is_empty() || edge.targets.is_empty() {
                    return Err(Error::MissingEdgeEnd {
                        edge: edge.id.clone(),
                    });
                }
                // Ports `JsonImporter.shapeById`: an id names a node first, and a port if no node
                // has it. `ElkGraphUtil.connectableShapeToNode` gives the port's node.
                let find = |id: &String| {
                    by_id
                        .get(id.as_str())
                        .map(|&node| (node, None))
                        .or_else(|| {
                            port_by_id
                                .get(id.as_str())
                                .map(|&(node, port)| (node, Some(port)))
                        })
                        .ok_or_else(|| Error::UnknownId {
                            edge: edge.id.clone(),
                            id: id.clone(),
                        })
                };
                // Ports `JsonImporter.transformPrimitiveEdge`, the old form: the id names a node,
                // looked up in `nodeIdMap` alone. A port key is looked up in `portIdMap` alone. A
                // port of another node is an error, and a key that names no port leaves the edge
                // at the node.
                let find_old = |id: &String, port_key: Option<&String>| {
                    let node = *by_id.get(id.as_str()).ok_or_else(|| Error::UnknownId {
                        edge: edge.id.clone(),
                        id: id.clone(),
                    })?;
                    match port_key.and_then(|key| port_by_id.get(key.as_str()).map(|p| (key, p))) {
                        None => Ok((node, None)),
                        Some((_, &(owner, port))) if owner == node => Ok((node, Some(port))),
                        Some((key, _)) => Err(Error::PortOfOtherNode {
                            edge: edge.id.clone(),
                            port: key.clone(),
                            node: id.clone(),
                        }),
                    }
                };
                let end = |ids: &[String], port_key: Option<&String>| {
                    ids.iter()
                        .map(|id| match &edge.old_form {
                            Some(_) => find_old(id, port_key),
                            None => find(id),
                        })
                        .collect::<Result<Vec<_>, _>>()
                };
                // Check every end before refusing a hyperedge, so an unknown id wins.
                let old = edge.old_form.as_ref();
                let sources = end(&edge.sources, old.and_then(|o| o.source_port.as_ref()))?;
                let targets = end(&edge.targets, old.and_then(|o| o.target_port.as_ref()))?;
                if sources.len() > 1 || targets.len() > 1 {
                    return Err(Error::Unsupported {
                        element,
                        feature: "an edge with more than one source or target".to_string(),
                    });
                }
                let settings = layout_options::edge_settings(&edge.layout_options)
                    .map_err(|error| option_error(element.clone(), error))?;
                let labels = label_settings(&edge.labels, &element)?;
                edges.push(EdgeInfo {
                    edge,
                    given_in,
                    index,
                    source: sources[0].0,
                    target: targets[0].0,
                    source_port: sources[0].1,
                    target_port: targets[0].1,
                    container: 0,
                    settings,
                    labels,
                });
            }
        }
        // elkjs reads edges depth first through the JSON, which is preorder: the order above.
        let edges_by_container = vec![Vec::new(); nodes.len()];
        let mut tree = Self {
            nodes,
            edges,
            by_id,
            edges_by_container,
        };
        for edge in 0..tree.edges.len() {
            let container = tree.best_containment(tree.edges[edge].source, tree.edges[edge].target);
            tree.edges[edge].container = container;
            tree.edges_by_container[container].push(edge);
        }
        tree.count_port_facts();
        Ok(tree)
    }

    /// Counts, for each port, the votes of the edges at it.
    ///
    /// Ports `ElkGraphImporter.calculateNetFlow`, `checkExternalPorts` and
    /// `isConnectedToExternalNodes`. An edge into a child of the port's node, or out of one,
    /// votes that the port is an input port of the node and an output port, in that order.
    /// Every other edge votes the other way round. A self loop votes as an edge to the outside. An
    /// inside self loop at a port votes too, but `run_layered` refuses it later.
    fn count_port_facts(&mut self) {
        for index in 0..self.edges.len() {
            let (source, target) = (self.edges[index].source, self.edges[index].target);
            if let Some(port) = self.edges[index].source_port {
                let to_child = self.nodes[target].parent == Some(source);
                let outside = !self.is_descendant(target, source);
                let facts = &mut self.nodes[source].port_facts[port];
                facts.net_flow += if to_child { -1 } else { 1 };
                if to_child {
                    facts.child_edges = (facts.child_edges + 1).min(2);
                }
                facts.connected_to_external_nodes |= outside;
                facts.inside_connections |= !outside;
                facts.has_edges = true;
            }
            if let Some(port) = self.edges[index].target_port {
                let from_child = self.nodes[source].parent == Some(target);
                let outside = !self.is_descendant(source, target);
                let facts = &mut self.nodes[target].port_facts[port];
                facts.net_flow += if from_child { 1 } else { -1 };
                if from_child {
                    facts.child_edges = (facts.child_edges + 1).min(2);
                }
                facts.connected_to_external_nodes |= outside;
                facts.inside_connections |= !outside;
                facts.has_edges = true;
            }
        }
    }

    /// Ports `ElkGraphUtil.isDescendant`.
    fn is_descendant(&self, child: usize, ancestor: usize) -> bool {
        let mut current = child;
        while let Some(parent) = self.nodes[current].parent {
            if parent == ancestor {
                return true;
            }
            current = parent;
        }
        false
    }

    /// Ports `ElkGraphUtil.findBestEdgeContainment`, for one source and one target.
    fn best_containment(&self, source: usize, target: usize) -> usize {
        let source_parent = self.nodes[source].parent;
        let target_parent = self.nodes[target].parent;
        if source_parent == target_parent {
            // Only the root has no parent; an edge from the root to itself stays in the root.
            return source_parent.unwrap_or(source);
        }
        if Some(source) == target_parent {
            return source;
        }
        if Some(target) == source_parent {
            return target;
        }
        // The general case starts from the source node itself.
        let common = source;
        if target == common || self.is_descendant(target, common) {
            return common;
        }
        // Java's loop also meets the sibling case here. With one source and one target, it has
        // returned above as the equal-parent case.
        self.lowest_common_ancestor(common, target)
    }

    /// Ports `ElkGraphUtil.findLowestCommonAncestor`, which counts a node as its own ancestor.
    fn lowest_common_ancestor(&self, a: usize, b: usize) -> usize {
        let ancestors = |mut node: usize| {
            let mut list = vec![node];
            while let Some(parent) = self.nodes[node].parent {
                list.push(parent);
                node = parent;
            }
            list.reverse();
            list
        };
        let (a, b) = (ancestors(a), ancestors(b));
        let mut common = 0;
        for (x, y) in a.iter().zip(&b) {
            if x == y {
                common = *x;
            } else {
                break;
            }
        }
        common
    }
}

/// An edge's route as layout left it.
struct EdgeRoute {
    start: LPoint,
    bend_points: Vec<LPoint>,
    end: LPoint,
    junction_points: Vec<LPoint>,
}

/// One node `layout_recursively` is working through: the children still to visit, in order.
struct Level {
    node: usize,
    include: bool,
    pending: VecDeque<usize>,
}

/// The layout in progress: a copy of the input that runs write into.
struct Work<'a, 't> {
    tree: &'t Tree<'a>,
    out: ElkNode,
    /// Size constraints and minimum size as ELK keeps them on the node, which
    /// `calculateMinimumGraphSize` and `resizeNode` may change.
    sizing: Vec<crate::options::NodeSizing>,
    /// The node's hierarchy handling once ELK has resolved `INHERIT`.
    handling: Vec<HierarchyHandling>,
    /// The options of each port as ELK keeps them on the port. A run defines the side of the
    /// ports of its own node and infers their border offsets, and the run of the parent reads
    /// that.
    port_state: Vec<Vec<PortSettings>>,
    /// The port constraints of a node whose run has placed its ports: `FIXED_POS`.
    placed_ports: Vec<bool>,
    routes: HashMap<usize, EdgeRoute>,
    edge_labels: HashMap<usize, Vec<(usize, LPoint, LSize)>>,
}

impl<'a, 't> Work<'a, 't> {
    /// No ELK counterpart: copies the input to write the layout into, as elkjs writes into its JSON.
    fn new(graph: &ElkNode, tree: &'t Tree<'a>) -> Self {
        let handling = tree
            .nodes
            .iter()
            .map(|info| {
                info.settings
                    .hierarchy_handling
                    .unwrap_or(HierarchyHandling::Inherit)
            })
            .collect();
        Self {
            tree,
            out: graph.clone(),
            sizing: tree
                .nodes
                .iter()
                .map(|info| info.settings.node_size)
                .collect(),
            handling,
            port_state: tree.nodes.iter().map(|info| info.ports.clone()).collect(),
            placed_ports: vec![false; tree.nodes.len()],
            routes: HashMap::new(),
            edge_labels: HashMap::new(),
        }
    }

    /// No ELK counterpart: finds a node of the output tree.
    fn out_node(&mut self, index: usize) -> &mut ElkNode {
        let mut node = &mut self.out;
        for &step in &self.tree.nodes[index].path {
            node = &mut node.children[step];
        }
        node
    }

    /// No ELK counterpart: finds a node of the output tree.
    fn out_node_ref(&self, index: usize) -> &ElkNode {
        let mut node = &self.out;
        for &step in &self.tree.nodes[index].path {
            node = &node.children[step];
        }
        node
    }

    /// Ports `RecursiveGraphLayoutEngine.evaluateHierarchyHandlingInheritance`.
    fn evaluate_handling(&mut self, node: usize) {
        if self.handling[node] == HierarchyHandling::Inherit {
            self.handling[node] = match self.tree.nodes[node].parent {
                None => HierarchyHandling::SeparateChildren,
                Some(parent) => self.handling[parent],
            };
        }
    }

    /// Ports the algorithm check of `ElkGraphImporter.importHierarchicalGraph` and `LayoutMetaDataService.getAlgorithmDataBySuffix`.
    fn check_algorithm(&self, node: usize) -> Result<(), Error> {
        if let Some(algorithm) = &self.tree.nodes[node].settings.algorithm {
            // `LayoutMetaDataService.getAlgorithmDataBySuffix`: the id or a suffix after a dot.
            let id = "org.eclipse.elk.layered";
            if !layout_options::is_id_suffix(id, algorithm) {
                return Err(Error::Unsupported {
                    element: node_element(&self.tree.nodes[node].node.id),
                    feature: format!("the layout algorithm \"{algorithm}\""),
                });
            }
        }
        Ok(())
    }

    /// Ports `RecursiveGraphLayoutEngine.layoutRecursively`, without top-down layout. Its inside
    /// self loop branch lives in the checks of `run_layered` and in `finish`.
    ///
    /// Java recurses once per nesting level. This keeps the same visiting order on a work list, so
    /// a deeply nested graph cannot overflow Rust's stack.
    fn layout_recursively(&mut self, root: usize) -> Result<(), Error> {
        let tree = self.tree;
        let mut stack: Vec<Level> = Vec::new();
        stack.extend(self.enter_level(root)?);
        while let Some(level) = stack.last_mut() {
            let Some(child) = level.pending.pop_front() else {
                if let Some(Level { node, include, .. }) = stack.pop() {
                    self.run_layered(node, include)?;
                }
                continue;
            };
            if !level.include {
                stack.extend(self.enter_level(child)?);
                continue;
            }
            self.evaluate_handling(child);
            if self.handling[child] == HierarchyHandling::SeparateChildren {
                stack.extend(self.enter_level(child)?);
            } else {
                if !tree.nodes[child].children.is_empty() {
                    self.check_algorithm(child)?;
                }
                level
                    .pending
                    .extend(tree.nodes[child].children.iter().copied());
            }
        }
        Ok(())
    }

    /// The start of `layoutRecursively` for one node: a node without children has no level.
    fn enter_level(&mut self, node: usize) -> Result<Option<Level>, Error> {
        let tree = self.tree;
        if tree.nodes[node].children.is_empty() {
            return Ok(None);
        }
        self.check_algorithm(node)?;
        self.evaluate_handling(node);
        Ok(Some(Level {
            node,
            include: self.handling[node] == HierarchyHandling::IncludeChildren,
            pending: tree.nodes[node].children.iter().copied().collect(),
        }))
    }

    /// The nodes one run of layered lays out: the scope's children, and, with
    /// `INCLUDE_CHILDREN`, the children of each node that does not lay out its own.
    ///
    /// Ports the node walk of `ElkGraphImporter.importHierarchicalGraph` and `importFlatGraph`.
    fn run_nodes(&self, scope: usize, include: bool) -> Vec<usize> {
        let mut nodes = Vec::new();
        let mut stack: Vec<usize> = self.tree.nodes[scope]
            .children
            .iter()
            .rev()
            .copied()
            .collect();
        while let Some(node) = stack.pop() {
            nodes.push(node);
            if include && self.handling[node] == HierarchyHandling::IncludeChildren {
                stack.extend(self.tree.nodes[node].children.iter().rev().copied());
            }
        }
        nodes
    }

    /// Lays out one scope with layered and writes the result.
    ///
    /// Lays out one scope with layered and writes the result.
    ///
    /// Ports `LayeredLayoutProvider.layout`, with `ElkGraphImporter.importGraph`'s edge selection
    /// and `calculateMinimumGraphSize`.
    fn run_layered(&mut self, scope: usize, include: bool) -> Result<(), Error> {
        let tree = self.tree;
        let scope_info = &tree.nodes[scope];
        // `transformExternalPort` reads the parent of the node, which the root has not: ELK fails
        // on a port of the root that an edge connects to a child.
        if scope_info.parent.is_none()
            && let Some(position) = scope_info
                .port_facts
                .iter()
                .position(|facts| facts.child_edges > 0)
        {
            return Err(Error::Unsupported {
                element: port_element(&scope_info.node.ports[position].id),
                feature: "an edge between a port of the root graph and a node inside it"
                    .to_string(),
            });
        }
        // The same lines read the parent for a port that holds a label and sits under `INSIDE`
        // port labels: `checkExternalPorts` makes it external even without an edge.
        if scope_info.parent.is_none()
            && scope_info
                .settings
                .options
                .port_label_placement
                .contains(PortLabelPlacement::INSIDE)
            && let Some(port) = scope_info
                .node
                .ports
                .iter()
                .find(|port| !port.labels.is_empty())
        {
            return Err(Error::Unsupported {
                element: port_element(&port.id),
                feature: "a labelled port of the root graph under inside port labels".to_string(),
            });
        }
        let run_nodes = self.run_nodes(scope, include);
        let in_run: HashSet<usize> = run_nodes.iter().copied().collect();
        // The nodes whose insides belong to this run.
        let mut graph_nodes: HashSet<usize> = HashSet::from([scope]);
        for &node in &run_nodes {
            if include
                && self.handling[node] == HierarchyHandling::IncludeChildren
                && !tree.nodes[node].children.is_empty()
            {
                graph_nodes.insert(node);
            }
        }

        // The edges of this run.
        // Visited in the order of `tree.edges`, which reaches the layout.
        let mut candidates: Vec<usize> = graph_nodes
            .iter()
            .flat_map(|&node| tree.edges_by_container[node].iter().copied())
            .collect();
        candidates.sort_unstable();
        // An edge may end at a port of the scope: ELK maps that port to the dummy of an
        // external port. An end at the scope itself has no node to map to.
        let at_scope_port = |node: usize, port: Option<usize>| node == scope && port.is_some();
        let mut edges = Vec::new();
        for edge_index in candidates {
            let edge = &tree.edges[edge_index];
            let element = edge_element(&edge.edge.id);
            let inside_self_loop = self.is_inside_self_loop(edge);
            // elkjs fails on an inside self loop at a port with a JavaScript error, so there is no
            // layout to hold ours to.
            if inside_self_loop && (edge.source_port.is_some() || edge.target_port.is_some()) {
                return Err(Error::Unsupported {
                    element,
                    feature: "an inside self loop at a port".to_string(),
                });
            }
            // `ElkGraphImporter.importFlatGraph` skips an inside self loop and looks for it in the
            // run of its node. That run holds the node's children, not the node, so ELK cannot
            // find the loop's end: `UnsupportedGraphException`. `layoutRecursively` also runs a
            // node that sets `SEPARATE_CHILDREN` on its own when it has inside self loops, with or
            // without children, so the loop fails in an `INCLUDE_CHILDREN` run too.
            if inside_self_loop
                && (!include
                    || tree.nodes[edge.source].settings.hierarchy_handling
                        == Some(HierarchyHandling::SeparateChildren))
            {
                return Err(Error::Unsupported {
                    element,
                    feature: INSIDE_SELF_LOOP_ALONE.to_string(),
                });
            }
            if !include {
                // `ElkGraphImporter.importFlatGraph` takes an edge between two children and
                // leaves edges into other levels unrouted.
                let siblings = tree.nodes[edge.source].parent == Some(scope)
                    && tree.nodes[edge.target].parent == Some(scope);
                // The two cases exclude each other: a node is not its own parent.
                let to_graph = (tree.nodes[edge.source].parent == Some(scope)
                    && edge.target == scope)
                    || (tree.nodes[edge.target].parent == Some(scope) && edge.source == scope);
                let to_graph_port = to_graph
                    && (at_scope_port(edge.source, edge.source_port)
                        || at_scope_port(edge.target, edge.target_port));
                if to_graph && !to_graph_port {
                    return Err(Error::Unsupported {
                        element,
                        feature: UNKNOWN_EDGE_END.to_string(),
                    });
                }
                if !siblings && !to_graph_port {
                    continue;
                }
            } else if !(in_run.contains(&edge.source)
                || at_scope_port(edge.source, edge.source_port))
                || !(in_run.contains(&edge.target) || at_scope_port(edge.target, edge.target_port))
            {
                // ELK's importer cannot find the end: `UnsupportedGraphException`.
                return Err(Error::Unsupported {
                    element,
                    feature: "an edge into a part of the graph laid out on its own".to_string(),
                });
            }
            edges.push(edge_index);
        }

        // `createExternalPortDummy` divides by the size of the node for fixed ratios, which the
        // preprocessor of the compound graph leaves out: ELK fails on it. That happens for a node
        // with children that an edge crosses.
        for &node in &run_nodes {
            let constraints = tree.nodes[node].settings.port_constraints;
            if !graph_nodes.contains(&node) || constraints != Some(PortConstraints::FixedRatio) {
                continue;
            }
            let crossing = edges.iter().find(|&&edge| {
                let (source, target) = (tree.edges[edge].source, tree.edges[edge].target);
                tree.is_descendant(source, node) != tree.is_descendant(target, node)
            });
            if let Some(&edge) = crossing {
                return Err(Error::Unsupported {
                    element: edge_element(&tree.edges[edge].edge.id),
                    feature:
                        "an edge across the border of a node with children and fixed-ratio ports"
                            .to_string(),
                });
            }
        }

        // The options of the scope, and what ELK reads from its parent.
        let mut options = scope_info.settings.options.clone();
        options.hierarchy_handling = self.handling[scope];
        // The graph has the port constraints of its node: `LGraph.copyProperties`.
        let scope_constraints = scope_info
            .settings
            .port_constraints
            .unwrap_or(PortConstraints::Undefined);
        options.port_constraints = scope_constraints;
        let run_direction = importer::resolve_direction(options.direction, options.aspect_ratio);
        // `importGraph` gives the ports of the scope a side first, so the minimum size below
        // sees them on their sides.
        self.define_port_sides(scope, run_direction, scope_constraints);
        let scope_input = self.input_node(scope, scope);
        let scope_cell = importer::cell_node(&scope_input);
        if scope_info.parent.is_some() && !self.sizing[scope].constraints.is_empty() {
            // `ElkGraphImporter.calculateMinimumGraphSize`, for a node laid out on its own.
            let horizontal = !scope_input.parent_settings.direction.is_vertical();
            options.node_size = self.sizing[scope];
            importer::include_minimum_graph_size(&mut options, &scope_cell, horizontal);
            // ELK adds the constraint to the set it shares with the node, so the node keeps it.
            self.sizing[scope]
                .constraints
                .insert(SizeConstraints::MINIMUM_SIZE);
        }
        let root_label_padding = crate::common::cells::inside_node_label_padding(&scope_cell);

        // `importHierarchicalGraph` gives a side to the ports of each node with a graph of its
        // own whose size the importer works out.
        for &node in &run_nodes {
            if include
                && self.handling[node] == HierarchyHandling::IncludeChildren
                && !tree.nodes[node].children.is_empty()
                && !self.sizing[node].constraints.is_empty()
            {
                let constraints = tree.nodes[node]
                    .settings
                    .port_constraints
                    .unwrap_or(PortConstraints::Undefined);
                self.define_port_sides(node, run_direction, constraints);
            }
        }
        let scope_ports = self.input_ports(scope);
        let scope_size = {
            let out = self.out_node_ref(scope);
            LSize {
                width: out.width,
                height: out.height,
            }
        };
        // A fixed ratio divides by the size of the node, both in `createExternalPortDummy` and in
        // `ElkUtil.resizeNode`. With no size, ELK gets infinity or NaN, and the layout of the
        // node's parent goes with it.
        if scope_constraints.is_ratio_fixed()
            && let Some(port) = scope_ports.iter().find(|port| match port.side {
                PortSide::East | PortSide::West => scope_size.height == 0.0,
                _ => scope_size.width == 0.0,
            })
        {
            return Err(Error::Unsupported {
                element: port_element(&port.id),
                feature: "a fixed-ratio port of a node without size".to_string(),
            });
        }

        let input = ElkInputGraph {
            id: scope_info.node.id.clone(),
            options,
            nodes: run_nodes
                .iter()
                .map(|&node| self.input_node(node, scope))
                .collect(),
            edges: edges.iter().map(|&edge| self.input_edge(edge)).collect(),
        };
        let importer::ImportedScope {
            graphs: mut graph,
            edge_containers,
        } = importer::import_scope(
            &input,
            &[],
            root_label_padding,
            importer::ScopePorts {
                ports: &scope_ports,
                node_size: scope_size,
                // `transformExternalPort` reads the label spacings from the parent of the node.
                label_spacings: DummyLabelSpacings {
                    label_port_horizontal: scope_input
                        .parent_settings
                        .options
                        .spacing
                        .label_port_horizontal,
                    label_port_vertical: scope_input
                        .parent_settings
                        .options
                        .spacing
                        .label_port_vertical,
                    label_label: scope_input.parent_settings.options.spacing.label_label,
                },
            },
        )
        .map_err(|error| {
            match error {
                // The minimum node size comes from the caller's options.
                importer::ImportError::NodeSize(_) => Error::Unsolvable {
                    element: node_element(&scope_info.node.id),
                    reason: error.to_string(),
                },
                // A port of the scope without a dummy: ELK cannot find the end of the edge.
                importer::ImportError::ScopePortWithoutDummy { edge_id } => Error::Unsupported {
                    element: edge_element(&edge_id),
                    feature: UNKNOWN_EDGE_END.to_string(),
                },
                // Ids and parents are checked before the import, so the rest is a bug.
                other => Error::Internal {
                    message: other.to_string(),
                },
            }
        })?;
        let result = if include {
            pipeline::execute_ported_compound_processors(&mut graph).map(|_| ())
        } else {
            pipeline::execute_ported_processors(&mut graph).map(|_| ())
        };
        result.map_err(|error| pipeline_error(&scope_info.node.id, error))?;

        let parents: HashMap<String, String> = run_nodes
            .iter()
            .map(|&node| {
                let parent = tree.nodes[node].parent.unwrap_or(scope);
                (
                    tree.nodes[node].node.id.clone(),
                    tree.nodes[parent].node.id.clone(),
                )
            })
            .collect();
        let sized_nodes: HashSet<String> = run_nodes
            .iter()
            .filter(|&&node| !self.sizing[node].constraints.is_empty())
            .map(|&node| tree.nodes[node].node.id.clone())
            .collect();
        let transfer = TransferInput {
            parents: &parents,
            sized_nodes: &sized_nodes,
            edge_containers: &edge_containers,
        };
        let result = export::apply_layout(&mut graph, &transfer)?;
        self.write_result(scope, &edges, result);
        Ok(())
    }

    /// The node as the importer takes it, in a run whose top node is `scope`.
    ///
    /// No ELK counterpart: maps the elkjs JSON tree onto the importer's input.
    fn input_node(&self, node: usize, scope: usize) -> ElkInputNode {
        let info = &self.tree.nodes[node];
        let current = self.out_node_ref(node);
        let parent = info
            .parent
            .filter(|&parent| parent != scope && node != scope)
            .map(|parent| self.tree.nodes[parent].node.id.clone());
        let node_size = self.sizing[node];
        let hierarchy_handling = if info.children.is_empty() {
            info.settings.hierarchy_handling
        } else {
            Some(self.handling[node])
        };
        // What the node's parent sets itself, which `calculateMinimumGraphSize` and the label
        // spacings read. `graph.hasProperty(DIRECTION)` in `NodeLabelAndSizeCalculator.process`
        // is the Java side of the direction. The root has no parent.
        let parent_settings = info.parent.map_or_else(
            || ParentSettings {
                options: LayeredOptions::default(),
                direction: ElkDirection::Undefined,
            },
            |parent| ParentSettings {
                options: self.tree.nodes[parent].settings.options.clone(),
                direction: self.tree.nodes[parent]
                    .settings
                    .direction
                    .unwrap_or(ElkDirection::Undefined),
            },
        );
        ElkInputNode {
            id: info.node.id.clone(),
            width: current.width,
            height: current.height,
            parent,
            direction: info.settings.direction,
            hierarchy_handling,
            layer_constraint: info.settings.layer_constraint,
            // `applyParentNodeLayout` fixes the ports a run has placed.
            port_constraints: if self.placed_ports[node] {
                Some(PortConstraints::FixedPos)
            } else {
                info.settings.port_constraints
            },
            port_alignment: info.settings.port_alignment,
            node_label_placement: info.settings.node_label_placement,
            node_flexibility: info.settings.node_flexibility,
            ports_surrounding: None,
            ports: self.input_ports(node),
            nested_options: (!info.children.is_empty()
                || info.settings.options.inside_self_loops_activate)
                .then(|| Box::new(info.settings.options.clone())),
            labels: current
                .labels
                .iter()
                .zip(&info.labels)
                .enumerate()
                .map(|(index, (label, settings))| input_label(index, label, settings))
                .collect(),
            node_size,
            self_loop_distribution: info.settings.self_loop_distribution,
            self_loop_ordering: info.settings.self_loop_ordering,
            parent_settings,
            port_label_placement: info.settings.options.port_label_placement,
            port_labels_treat_as_group: info.settings.options.port_labels_treat_as_group,
        }
    }

    /// The ports of a node as the importer takes them, with the node as it stands now.
    ///
    /// No ELK counterpart: maps the elkjs JSON tree onto the importer's input.
    fn input_ports(&self, node: usize) -> Vec<ElkInputPort> {
        let info = &self.tree.nodes[node];
        self.out_node_ref(node)
            .ports
            .iter()
            .zip(&self.port_state[node])
            .zip(&info.port_facts)
            .zip(&info.port_labels)
            .map(|(((port, settings), facts), label_settings)| ElkInputPort {
                id: port.id.clone(),
                x: port.x,
                y: port.y,
                width: port.width,
                height: port.height,
                side: settings.side,
                index: settings.index,
                border_offset: settings.border_offset,
                allow_non_flow_ports_to_switch_sides: settings.allow_non_flow_ports_to_switch_sides,
                net_flow: facts.net_flow,
                child_edges: facts.child_edges,
                connected_to_external_nodes: facts.connected_to_external_nodes,
                inside_connections: facts.inside_connections,
                has_edges: facts.has_edges,
                labels: port
                    .labels
                    .iter()
                    .zip(label_settings)
                    .enumerate()
                    .map(|(index, (label, settings))| input_label(index, label, settings))
                    .collect(),
            })
            .collect()
    }

    /// Gives every port of a node a side, as `ElkGraphImporter.ensureDefinedPortSide` does for
    /// the graph `direction` and the port `constraints` of the node's graph. The side stays on
    /// the port: the run of the node's parent reads it.
    fn define_port_sides(
        &mut self,
        node: usize,
        direction: ElkDirection,
        constraints: PortConstraints,
    ) {
        let size = {
            let out = self.out_node_ref(node);
            LSize {
                width: out.width,
                height: out.height,
            }
        };
        let ports = self.input_ports(node);
        for (index, port) in ports.iter().enumerate() {
            let side = if !constraints.is_side_fixed() {
                // Free to choose, so the net flow decides.
                if port.net_flow > 0 {
                    port_side_from_direction(direction)
                } else {
                    port_side_from_direction(direction).opposed()
                }
            } else if port.side == PortSide::Undefined {
                // The side follows from the position, or else from the direction.
                let (port_position, port_size) = port.bounds();
                match calc_port_side(port_position, port_size, size, direction) {
                    PortSide::Undefined => port_side_from_direction(direction),
                    side => side,
                }
            } else {
                port.side
            };
            self.port_state[node][index].side = side;
        }
    }

    /// No ELK counterpart: the id of a node's port.
    fn port_id(&self, node: usize, port: usize) -> String {
        self.tree.nodes[node].node.ports[port].id.clone()
    }

    /// Whether the edge is a self loop that `insideSelfLoops.yo` and the `insideSelfLoops.activate`
    /// of its node route inside the node.
    ///
    /// Ports the `isInsideSelfLoop` tests in `ElkGraphImporter.importFlatGraph` and
    /// `importHierarchicalGraph`.
    fn is_inside_self_loop(&self, edge: &EdgeInfo<'_>) -> bool {
        edge.source == edge.target
            && edge.settings.inside_self_loops_yo
            && self.tree.nodes[edge.source]
                .settings
                .options
                .inside_self_loops_activate
    }

    /// No ELK counterpart: maps the elkjs JSON tree onto the importer's input.
    fn input_edge(&self, edge: usize) -> ElkInputEdge {
        let info = &self.tree.edges[edge];
        ElkInputEdge {
            id: info.edge.id.clone(),
            source: self.tree.nodes[info.source].node.id.clone(),
            target: self.tree.nodes[info.target].node.id.clone(),
            // `ElkGraphImporter.transformEdge` skips labels without text.
            labels: info
                .edge
                .labels
                .iter()
                .zip(&info.labels)
                .enumerate()
                .filter(|(_, (label, _))| !label.text.is_empty())
                .map(|(index, (label, settings))| input_label(index, label, settings))
                .collect(),
            minlen: 1,
            inside_self_loops_yo: info.settings.inside_self_loops_yo,
            model_order: None,
            priority_direction: info.settings.priority_direction,
            priority_shortness: info.settings.priority_shortness,
            priority_straightness: info.settings.priority_straightness,
            container: Some(self.tree.nodes[info.container].node.id.clone()),
            source_port: info.source_port.map(|port| self.port_id(info.source, port)),
            target_port: info.target_port.map(|port| self.port_id(info.target, port)),
        }
    }

    /// Writes one run's result into the output graph.
    ///
    /// Ports the writes of `ElkGraphLayoutTransferrer.applyNodeLayout` into the input graph.
    fn write_result(&mut self, scope: usize, edges: &[usize], result: LayoutResult) {
        for node in &result.nodes {
            let Some(&index) = self.tree.by_id.get(node.id.as_str()) else {
                continue;
            };
            let out = self.out_node(index);
            out.x = node.position.x;
            out.y = node.position.y;
            if let Some(size) = node.size {
                out.width = size.width;
                out.height = size.height;
            }
            // `ElkGraphLayoutTransferrer.applyNodeLayout`: `elkport.setLocation(lport.getPosition())`.
            for &(port, position) in &node.ports {
                if let Some(port) = out.ports.get_mut(port) {
                    port.x = position.x;
                    port.y = position.y;
                }
            }
            for &(label, position, size) in &node.labels {
                if let Some(label) = out.labels.get_mut(label) {
                    label.x = position.x;
                    label.y = position.y;
                    label.width = size.width;
                    label.height = size.height;
                }
            }
            for &(port, label, position, size) in &node.port_labels {
                if let Some(label) = out
                    .ports
                    .get_mut(port)
                    .and_then(|port| port.labels.get_mut(label))
                {
                    label.x = position.x;
                    label.y = position.y;
                    label.width = size.width;
                    label.height = size.height;
                }
            }
        }
        let edge_of: HashMap<&str, usize> = edges
            .iter()
            .map(|&edge| (self.tree.edges[edge].edge.id.as_str(), edge))
            .collect();
        for edge in result.edges {
            let Some(&index) = edge_of.get(edge.id.as_str()) else {
                continue;
            };
            self.routes.insert(
                index,
                EdgeRoute {
                    start: edge.start,
                    bend_points: edge.bend_points,
                    end: edge.end,
                    junction_points: edge.junction_points,
                },
            );
            self.edge_labels.insert(index, edge.labels);
        }
        // `ElkGraphLayoutTransferrer.applyLayout`: the position of each external port.
        for port in &result.scope_ports {
            let out = self.out_node(scope);
            if let Some(out_port) = out.ports.get_mut(port.index) {
                out_port.x = port.position.x;
                out_port.y = port.position.y;
            }
            // The dummy chose the side and the offset, and the port keeps them.
            let state = &mut self.port_state[scope][port.index];
            state.side = port.side;
            state.border_offset = Some(port.border_offset);
        }
        self.apply_parent_node_layout(scope, result.actual_size, result.external_ports);
    }

    /// Ports `ElkGraphLayoutTransferrer.applyParentNodeLayout` for the top graph of a run, with
    /// `ElkUtil.resizeNode(node, width, height, movePorts, true)` and `effectiveMinSizeConstraintFor`.
    ///
    /// A graph with external ports has placed the ports of its node. The node keeps them where
    /// they are, and `FIXED_POS` keeps them there in the layout of the node's parent. Without
    /// external ports, `resizeNode` moves the ports along with the new size.
    fn apply_parent_node_layout(&mut self, scope: usize, actual: LSize, external_ports: bool) {
        if external_ports {
            self.placed_ports[scope] = true;
        }
        let included_port_labels = self.sizing[scope]
            .constraints
            .contains(SizeConstraints::PORT_LABELS);
        let mut new_size = self.sizing[scope].effective_minimum().unwrap_or_default();
        new_size.width = f64::max(new_size.width, actual.width);
        new_size.height = f64::max(new_size.height, actual.height);
        let (old, ports_fixed, direction) = {
            // `node.getParent() == null ? node.getProperty(DIRECTION) : parent's`
            let direction_source = self.tree.nodes[scope].parent.unwrap_or(scope);
            let direction = self.tree.nodes[direction_source]
                .settings
                .direction
                .unwrap_or(ElkDirection::Undefined);
            let ports_fixed = self.placed_ports[scope]
                || self.tree.nodes[scope].settings.port_constraints
                    == Some(PortConstraints::FixedPos);
            let node = self.out_node(scope);
            (
                LSize {
                    width: node.width,
                    height: node.height,
                },
                ports_fixed,
                direction,
            )
        };
        let width_diff = new_size.width - old.width;
        let height_diff = new_size.height - old.height;
        let width_ratio = new_size.width / old.width;
        let height_ratio = new_size.height / old.height;
        if !external_ports {
            for index in 0..self.port_state[scope].len() {
                let mut side = self.port_state[scope][index].side;
                let port = &mut self.out_node(scope).ports[index];
                if side == PortSide::Undefined {
                    let (port_position, port_size) = port.bounds();
                    side = calc_port_side(port_position, port_size, old, direction);
                }
                match side {
                    PortSide::North => {
                        if !ports_fixed {
                            port.x *= width_ratio;
                        }
                    }
                    PortSide::East => {
                        port.x += width_diff;
                        if !ports_fixed {
                            port.y *= height_ratio;
                        }
                    }
                    PortSide::South => {
                        if !ports_fixed {
                            port.x *= width_ratio;
                        }
                        port.y += height_diff;
                    }
                    PortSide::West => {
                        if !ports_fixed {
                            port.y *= height_ratio;
                        }
                    }
                    PortSide::Undefined => {}
                }
                self.port_state[scope][index].side = side;
            }
        }
        let node = self.out_node(scope);
        node.width = new_size.width;
        node.height = new_size.height;
        for label in &mut node.labels {
            let mid_x = label.x + label.width / 2.0;
            let mid_y = label.y + label.height / 2.0;
            let width_percent = mid_x / old.width;
            let height_percent = mid_y / old.height;
            if width_percent + height_percent >= 1.0 {
                if width_percent - height_percent > 0.0 && mid_y >= 0.0 {
                    label.x += width_diff;
                    label.y += height_diff * height_percent;
                } else if width_percent - height_percent < 0.0 && mid_x >= 0.0 {
                    label.x += width_diff * width_percent;
                    label.y += height_diff;
                }
            }
        }
        // `ElkUtil.resizeNode` sets the node's size constraints to fixed. Then
        // `applyParentNodeLayout` gives back `PORT_LABELS` to a node that had it, or the ports of
        // its parent's run would be placed without their labels.
        self.sizing[scope].constraints = if included_port_labels {
            SizeConstraints::PORT_LABELS
        } else {
            SizeConstraints::FIXED
        };
    }

    /// Writes the edge routes and containers.
    ///
    /// Ports `JsonImporter.transferLayout` for edges and `ElkGraphUtil.firstEdgeSection`.
    fn finish(mut self) -> ElkNode {
        let tree = self.tree;
        for (index, info) in tree.edges.iter().enumerate() {
            let container = tree.nodes[info.container].node.id.clone();
            // The shapes the edge was attached to, as elkjs writes their ids. In the old form a
            // port key names a port that `sources` and `targets` do not.
            let shape = |node: usize, port: Option<usize>| {
                let node = tree.nodes[node].node;
                port.map_or(&node.id, |port| &node.ports[port].id).clone()
            };
            let incoming_shape = shape(info.source, info.source_port);
            let outgoing_shape = shape(info.target, info.target_port);
            let mut route = self.routes.remove(&index);
            let mut labels = self.edge_labels.remove(&index).unwrap_or_default();
            // `RecursiveGraphLayoutEngine.postProcessInsideSelfLoops`: the layered run routes an
            // inside self loop relative to its node's top left corner. Once the node has its final
            // place, the loop moves by that place, into the coordinates of its container. elkjs
            // moves the labels of the loop too, which the Java method does not do.
            let inside_self_loop = self.is_inside_self_loop(info);
            let by = inside_self_loop.then(|| {
                let at = self.out_node_ref(info.source);
                LPoint { x: at.x, y: at.y }
            });
            let shift = |p: &mut LPoint| {
                if let Some(by) = by {
                    p.x += by.x;
                    p.y += by.y;
                }
            };
            if let Some(route) = route.as_mut() {
                shift(&mut route.start);
                shift(&mut route.end);
                route.bend_points.iter_mut().for_each(shift);
                route.junction_points.iter_mut().for_each(shift);
            }
            for (_, position, _) in &mut labels {
                shift(position);
            }
            let node = self.out_node(info.given_in);
            let edge = &mut node.edges[info.index];
            edge.container = Some(container);
            if let Some(route) = route {
                // `ElkGraphUtil.firstEdgeSection(edge, true, true)` keeps the first section's
                // id, if any, and drops the others.
                let id = edge
                    .sections
                    .first()
                    .map_or_else(|| format!("{}_s0", edge.id), |section| section.id.clone());
                edge.sections = vec![ElkEdgeSection {
                    id,
                    start_point: point(route.start),
                    end_point: point(route.end),
                    bend_points: route.bend_points.into_iter().map(point).collect(),
                    incoming_shape: Some(incoming_shape),
                    outgoing_shape: Some(outgoing_shape),
                }];
                // `JsonImporter.transferLayoutInt` writes junction points only when there are some.
                // With none, the edge keeps the ones it came with: the layout sets the ELK
                // property to `null`, which leaves the JSON as it was.
                if !route.junction_points.is_empty() {
                    edge.junction_points = route.junction_points.into_iter().map(point).collect();
                }
            }
            for (label, position, size) in labels {
                if let Some(label) = edge.labels.get_mut(label) {
                    label.x = position.x;
                    label.y = position.y;
                    label.width = size.width;
                    label.height = size.height;
                }
            }
        }
        self.out
    }
}

/// No ELK counterpart: converts a point.
fn point(p: LPoint) -> ElkPoint {
    ElkPoint { x: p.x, y: p.y }
}

/// Ports `ElkGraphImporter.transformLabel`.
fn input_label(index: usize, label: &ElkLabel, settings: &LabelSettings) -> ElkInputLabel {
    ElkInputLabel {
        source_index: Some(index),
        text: label.text.clone(),
        width: label.width,
        height: label.height,
        placement: settings.placement,
        inline: settings.inline,
        node_label_placement: settings.node_label_placement,
        x: label.x,
        y: label.y,
    }
}

/// No ELK counterpart: turns a pipeline error into an `Error`.
fn pipeline_error(scope: &str, error: PipelineError) -> Error {
    match error {
        PipelineError::RandomSeed(crate::random::RandomSeedError::ZeroSeed { graph_id }) => {
            Error::ZeroRandomSeed { graph: graph_id }
        }
        PipelineError::UnsupportedProcessor { kind } => Error::Unsupported {
            element: node_element(scope),
            feature: format!("the layered step {kind:?}"),
        },
        PipelineError::UnsupportedCompoundGraph { reason } => Error::Unsupported {
            element: node_element(scope),
            feature: reason.to_string(),
        },
        PipelineError::Intermediate(error) => Error::Unsupported {
            element: node_element(scope),
            feature: error.to_string(),
        },
        // The caller's numbers: the minimum node size, a spacing, a size too large for ELK's
        // `int` arithmetic, or a node size stretch-width cannot normalize.
        error @ (PipelineError::NodeSize(_)
        | PipelineError::NetworkSimplex(NetworkSimplexError::CoordinateOutOfRange)
        | PipelineError::Component(ComponentError::InvalidGeometry)
        | PipelineError::Layering(
            LayeringError::InvalidStretchWidthDimensions
            | LayeringError::StretchWidthRetryLimit { .. },
        )) => Error::Unsolvable {
            element: node_element(scope),
            reason: error.to_string(),
        },
        // A step that is not ported.
        error @ PipelineError::Component(ComponentError::HierarchyEdges) => Error::Unsupported {
            element: node_element(scope),
            feature: error.to_string(),
        },
        // Everything left cannot come from the caller's graph: a cycle after cycle breaking,
        // a bad index, or a layer count that cannot be allocated.
        other => Error::Internal {
            message: other.to_string(),
        },
    }
}
