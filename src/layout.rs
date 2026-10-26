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

use crate::api::{ElkEdge, ElkEdgeSection, ElkLabel, ElkNode, ElkPoint};
use crate::common::networksimplex::NetworkSimplexError;
use crate::components::ComponentError;
use crate::error::Error;
use crate::export::{self, LayoutResult, TransferInput};
use crate::graph::{LPoint, LSize};
use crate::importer::{self, ElkInputEdge, ElkInputGraph, ElkInputLabel, ElkInputNode};
use crate::layout_options::{
    self, EdgeSettings, LabelSettings, NodeSettings, OptionError, OptionProblem,
};
use crate::options::{ElkDirection, HierarchyHandling, LayeredOptions, SizeConstraints};
use crate::p2layers::LayeringError;
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
/// that uses ports, an option elk-layered does not support or cannot read, an id used twice
/// or not found, or a size that is no finite, non-negative number.
///
/// Ports `RecursiveGraphLayoutEngine.layout` with the JSON round trip of elkjs's `JsonImporter`.
pub fn layout(graph: &ElkNode) -> Result<ElkNode, Error> {
    let tree = Tree::new(graph)?;
    let mut work = Work::new(graph, &tree);
    work.layout_recursively(0)?;
    Ok(work.finish())
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
}

/// One edge of the input.
struct EdgeInfo<'a> {
    edge: &'a ElkEdge,
    /// The node whose `edges` list holds it, and its index there.
    given_in: usize,
    index: usize,
    source: usize,
    target: usize,
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

/// The most levels a node may sit below the root.
const MAX_NESTING_DEPTH: usize = 1300;

impl<'a> Tree<'a> {
    /// Ports `JsonImporter.transform`: nodes in JSON order, then edges depth first, each moved by `ElkGraphUtil.updateContainment`.
    fn new(root: &'a ElkNode) -> Result<Self, Error> {
        let mut nodes: Vec<NodeInfo<'a>> = Vec::new();
        let mut by_id: HashMap<&'a str, usize> = HashMap::new();
        // Preorder, so a node's index is smaller than its children's.
        let mut stack: Vec<(&'a ElkNode, Option<usize>, Vec<usize>)> =
            vec![(root, None, Vec::new())];
        while let Some((node, parent, path)) = stack.pop() {
            let element = node_element(&node.id);
            // The graph types clone and drop their children recursively, and a debug build in
            // a 2 MB thread overflows its stack at about 1600 levels. Refuse earlier, with an
            // error, rather than abort.
            if path.len() > MAX_NESTING_DEPTH {
                return Err(Error::Unsupported {
                    element,
                    feature: format!("a graph nested deeper than {MAX_NESTING_DEPTH} levels"),
                });
            }
            if by_id.insert(node.id.as_str(), nodes.len()).is_some() {
                return Err(Error::DuplicateId {
                    id: node.id.clone(),
                });
            }
            if !node.ports.is_empty() {
                return Err(Error::Unsupported {
                    element,
                    feature: "ports".to_string(),
                });
            }
            check_number(&element, "x", node.x, false)?;
            check_number(&element, "y", node.y, false)?;
            check_number(&element, "width", node.width, true)?;
            check_number(&element, "height", node.height, true)?;
            let settings = layout_options::node_settings(&node.layout_options)
                .map_err(|error| option_error(element.clone(), error))?;
            let labels = label_settings(&node.labels, &element)?;
            let index = nodes.len();
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
                let find = |id: &String| {
                    by_id
                        .get(id.as_str())
                        .copied()
                        .ok_or_else(|| Error::UnknownId {
                            edge: edge.id.clone(),
                            id: id.clone(),
                        })
                };
                // Check every end before refusing a hyperedge, so an unknown id wins.
                let sources = edge
                    .sources
                    .iter()
                    .map(find)
                    .collect::<Result<Vec<_>, _>>()?;
                let targets = edge
                    .targets
                    .iter()
                    .map(find)
                    .collect::<Result<Vec<_>, _>>()?;
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
                    source: sources[0],
                    target: targets[0],
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
        Ok(tree)
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

    /// Source: `ElkGraphUtil.findBestEdgeContainment` for one source and one target.
    ///
    /// Ports `ElkGraphUtil.findBestEdgeContainment`.
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
        if self.nodes[target].parent == self.nodes[common].parent {
            return self.nodes[target].parent.unwrap_or(common);
        }
        self.lowest_common_ancestor(common, target)
    }

    /// Source: `ElkGraphUtil.findLowestCommonAncestor`, which counts a node as its own
    /// ancestor.
    ///
    /// Ports `ElkGraphUtil.findLowestCommonAncestor`.
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
    /// Whether ELK has fixed the node's size: `resizeNode` sets the constraints to fixed.
    size_fixed: Vec<bool>,
    /// Size constraints and minimum size as ELK keeps them on the node, which
    /// `calculateMinimumGraphSize` may change.
    sizing: Vec<crate::options::NodeSizing>,
    /// The node's hierarchy handling once ELK has resolved `INHERIT`.
    handling: Vec<HierarchyHandling>,
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
            size_fixed: vec![false; tree.nodes.len()],
            sizing: tree
                .nodes
                .iter()
                .map(|info| info.settings.node_size)
                .collect(),
            handling,
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

    /// Source: `RecursiveGraphLayoutEngine.evaluateHierarchyHandlingInheritance`.
    ///
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

    /// Source: `RecursiveGraphLayoutEngine.layoutRecursively`.
    ///
    /// Ports `RecursiveGraphLayoutEngine.layoutRecursively`, without top-down layout and inside self-loops.
    /// Java recurses once per nesting level. This keeps the same visiting order on a work
    /// list, so a deeply nested graph cannot overflow Rust's stack.
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
    /// Source: `LayeredLayoutProvider.layout`.
    ///
    /// Ports `LayeredLayoutProvider.layout`, with `ElkGraphImporter.importGraph`'s edge selection and `calculateMinimumGraphSize`.
    fn run_layered(&mut self, scope: usize, include: bool) -> Result<(), Error> {
        let tree = self.tree;
        let scope_info = &tree.nodes[scope];
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
        let mut edges = Vec::new();
        for edge_index in candidates {
            let edge = &tree.edges[edge_index];
            let element = edge_element(&edge.edge.id);
            if !include {
                // `ElkGraphImporter.importFlatGraph` takes an edge between two children and
                // leaves edges into other levels unrouted.
                let siblings = tree.nodes[edge.source].parent == Some(scope)
                    && tree.nodes[edge.target].parent == Some(scope);
                // The two cases exclude each other: a node is not its own parent.
                let to_graph = (tree.nodes[edge.source].parent == Some(scope)
                    && edge.target == scope)
                    || (tree.nodes[edge.target].parent == Some(scope) && edge.source == scope);
                if to_graph {
                    return Err(Error::Unsupported {
                        element,
                        feature:
                            "an edge between a node and its own children laid out on their own"
                                .to_string(),
                    });
                }
                if !siblings {
                    continue;
                }
            } else if !in_run.contains(&edge.source) || !in_run.contains(&edge.target) {
                // ELK's importer cannot find the end: `UnsupportedGraphException`.
                return Err(Error::Unsupported {
                    element,
                    feature: "an edge into a part of the graph laid out on its own".to_string(),
                });
            }
            edges.push(edge_index);
        }

        // The options of the scope, and what ELK reads from its parent.
        let parent_raw = scope_info
            .parent
            .map_or_else(LayeredOptions::default, |parent| {
                tree.nodes[parent].settings.options.clone()
            });
        let mut options = scope_info.settings.options.clone();
        options.hierarchy_handling = self.handling[scope];
        let scope_cell = importer::cell_node(&self.input_node(scope, scope), &parent_raw);
        if scope_info.parent.is_some() && !self.sizing[scope].constraints.is_empty() {
            // `ElkGraphImporter.calculateMinimumGraphSize`, for a node laid out on its own.
            let horizontal = !self.parent_direction(scope).is_vertical();
            options.node_size = self.sizing[scope];
            importer::include_minimum_graph_size(&mut options, &scope_cell, horizontal);
            // ELK adds the constraint to the set it shares with the node, so the node keeps it.
            self.sizing[scope]
                .constraints
                .insert(SizeConstraints::MINIMUM_SIZE);
        }
        let root_label_padding = crate::common::cells::inside_node_label_padding(&scope_cell);

        let input = ElkInputGraph {
            id: scope_info.node.id.clone(),
            options,
            nodes: run_nodes
                .iter()
                .map(|&node| self.input_node(node, scope))
                .collect(),
            edges: edges.iter().map(|&edge| self.input_edge(edge)).collect(),
        };
        let mut graph =
            importer::import_scope(&input, &[], root_label_padding).map_err(|error| {
                match error {
                    // The minimum node size comes from the caller's options.
                    importer::ImportError::NodeSize(_) => Error::Unsolvable {
                        element: node_element(&scope_info.node.id),
                        reason: error.to_string(),
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
            .filter(|&&node| {
                let constraints = if self.size_fixed[node] {
                    SizeConstraints::FIXED
                } else {
                    self.sizing[node].constraints
                };
                !constraints.is_empty()
            })
            .map(|&node| tree.nodes[node].node.id.clone())
            .collect();
        let edge_containers: HashMap<String, String> = edges
            .iter()
            .map(|&edge| {
                (
                    tree.edges[edge].edge.id.clone(),
                    tree.nodes[tree.edges[edge].container].node.id.clone(),
                )
            })
            .collect();
        let transfer = TransferInput {
            parents: &parents,
            sized_nodes: &sized_nodes,
            edge_containers: &edge_containers,
        };
        let result = export::apply_layout(&graph, &transfer)?;
        self.write_result(scope, &edges, result);
        Ok(())
    }

    /// The direction the node's parent sets itself, or `Undefined`.
    ///
    /// No ELK counterpart: `graph.hasProperty(DIRECTION)` in `NodeLabelAndSizeCalculator.process`.
    fn parent_direction(&self, node: usize) -> ElkDirection {
        self.tree.nodes[node]
            .parent
            .and_then(|parent| self.tree.nodes[parent].settings.direction)
            .unwrap_or(ElkDirection::Undefined)
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
        let mut node_size = self.sizing[node];
        if self.size_fixed[node] {
            node_size.constraints = SizeConstraints::FIXED;
        }
        let hierarchy_handling = if info.children.is_empty() {
            info.settings.hierarchy_handling
        } else {
            Some(self.handling[node])
        };
        ElkInputNode {
            id: info.node.id.clone(),
            width: current.width,
            height: current.height,
            parent,
            direction: info.settings.direction,
            hierarchy_handling,
            layer_constraint: info.settings.layer_constraint,
            port_constraints: info.settings.port_constraints,
            port_alignment: info.settings.port_alignment,
            node_label_placement: info.settings.node_label_placement,
            node_flexibility: info.settings.node_flexibility,
            ports_surrounding: None,
            nested_options: (!info.children.is_empty())
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
        }
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
            inside_self_loops_yo: false,
            model_order: None,
            priority_direction: info.settings.priority_direction,
            priority_shortness: info.settings.priority_shortness,
            priority_straightness: info.settings.priority_straightness,
            container: Some(self.tree.nodes[info.container].node.id.clone()),
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
            for &(label, position, size) in &node.labels {
                if let Some(label) = out.labels.get_mut(label) {
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
                },
            );
            self.edge_labels.insert(index, edge.labels);
        }
        self.apply_parent_node_layout(scope, result.actual_size);
    }

    /// Source: `ElkGraphLayoutTransferrer.applyParentNodeLayout` for the top graph of a run,
    /// with `ElkUtil.resizeNode(node, width, height, true, true)`. Without ports, there is
    /// no port to move.
    ///
    /// Ports `ElkGraphLayoutTransferrer.applyParentNodeLayout` with `ElkUtil.resizeNode` and `effectiveMinSizeConstraintFor`.
    fn apply_parent_node_layout(&mut self, scope: usize, actual: LSize) {
        let mut sizing = self.sizing[scope];
        if self.size_fixed[scope] {
            sizing.constraints = SizeConstraints::FIXED;
        }
        let mut new_size = sizing.effective_minimum().unwrap_or_default();
        new_size.width = f64::max(new_size.width, actual.width);
        new_size.height = f64::max(new_size.height, actual.height);
        let node = self.out_node(scope);
        let old = LSize {
            width: node.width,
            height: node.height,
        };
        let width_diff = new_size.width - old.width;
        let height_diff = new_size.height - old.height;
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
        self.size_fixed[scope] = true;
    }

    /// Writes the edge routes and containers.
    ///
    /// Ports `JsonImporter.transferLayout` for edges and `ElkGraphUtil.firstEdgeSection`.
    fn finish(mut self) -> ElkNode {
        let tree = self.tree;
        for (index, info) in tree.edges.iter().enumerate() {
            let container = tree.nodes[info.container].node.id.clone();
            let route = self.routes.remove(&index);
            let labels = self.edge_labels.remove(&index).unwrap_or_default();
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
                    incoming_shape: edge.sources.first().cloned(),
                    outgoing_shape: edge.targets.first().cloned(),
                }];
                // The layout sets junction points, or clears them.
                edge.junction_points.clear();
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
        error @ PipelineError::Component(
            ComponentError::ExternalPortGroups | ComponentError::HierarchyEdges,
        ) => Error::Unsupported {
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
