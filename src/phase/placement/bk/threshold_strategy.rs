//! Port of ELK's `ThresholdStrategy`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p4nodes/bk/ThresholdStrategy.java

use std::collections::{HashSet, VecDeque};

use crate::graph::{EdgeId, LGraph};
use crate::options::EdgeStraighteningStrategy;
use crate::phase::placement::bk::bk_aligned_layout::{
    BKAlignedLayout, HDirection, VDirection, calculate_delta, check_space_above, check_space_below,
    port_anchor_y, port_position_y, shift_block,
};
use crate::phase::placement::bk::bk_node_placer::edge_opposite_node;
use crate::phase::placement::bk::neighborhood_information::{
    NeighborhoodInformation, is_in_layer_edge_index,
};

#[derive(Debug)]
pub(crate) enum ThresholdStrategy {
    Null,
    Simple(SimpleThresholdStrategy),
}

impl ThresholdStrategy {
    pub(crate) fn for_graph(graph: &LGraph) -> Self {
        match graph.options.node_placement_bk_edge_straightening {
            EdgeStraighteningStrategy::None => Self::Null,
            EdgeStraighteningStrategy::ImproveStraightness => {
                Self::Simple(SimpleThresholdStrategy::default())
            }
        }
    }

    pub(crate) fn calculate_threshold(
        &mut self,
        graph: &LGraph,
        ni: &NeighborhoodInformation,
        layout: &mut BKAlignedLayout,
        old_thresh: f64,
        block_root: usize,
        current_node: usize,
    ) -> f64 {
        match self {
            Self::Null => match layout.vdir {
                VDirection::Up => f64::INFINITY,
                VDirection::Down => f64::NEG_INFINITY,
            },
            Self::Simple(strategy) => strategy.calculate_threshold(
                graph,
                ni,
                layout,
                old_thresh,
                block_root,
                current_node,
            ),
        }
    }

    pub(crate) fn finish_block(&mut self, root: usize) {
        if let Self::Simple(strategy) = self {
            strategy.block_finished.insert(root);
        }
    }

    pub(crate) fn post_process(
        &mut self,
        graph: &LGraph,
        ni: &NeighborhoodInformation,
        layout: &mut BKAlignedLayout,
    ) {
        if let Self::Simple(strategy) = self {
            strategy.post_process(graph, ni, layout);
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct SimpleThresholdStrategy {
    block_finished: HashSet<usize>,
    post_processables_queue: VecDeque<Postprocessable>,
    post_processables_stack: Vec<Postprocessable>,
}

impl SimpleThresholdStrategy {
    fn calculate_threshold(
        &mut self,
        graph: &LGraph,
        ni: &NeighborhoodInformation,
        layout: &mut BKAlignedLayout,
        old_thresh: f64,
        block_root: usize,
        current_node: usize,
    ) -> f64 {
        let is_root = block_root == current_node;
        let is_last = layout.align[current_node] == block_root;
        if !(is_root || is_last) {
            return old_thresh;
        }

        let mut threshold = old_thresh;
        if is_root {
            threshold = self.get_bound(graph, ni, layout, block_root, true);
        }
        if threshold.is_infinite() && is_last {
            threshold = self.get_bound(graph, ni, layout, current_node, false);
        }
        threshold
    }

    fn get_bound(
        &mut self,
        graph: &LGraph,
        ni: &NeighborhoodInformation,
        layout: &mut BKAlignedLayout,
        block_node: usize,
        is_root: bool,
    ) -> f64 {
        let invalid = match layout.vdir {
            VDirection::Up => f64::INFINITY,
            VDirection::Down => f64::NEG_INFINITY,
        };
        let pick = self.pick_edge(graph, ni, layout, Postprocessable::new(block_node, is_root));

        match pick.edge {
            None if pick.has_edges => {
                self.post_processables_queue.push_back(pick);
                invalid
            }
            Some(edge) => {
                let left = graph.edges[edge].source();
                let right = graph.edges[edge].target();
                let threshold = if is_root {
                    let root_port = if layout.hdir == HDirection::Right {
                        right
                    } else {
                        left
                    };
                    let other_port = if layout.hdir == HDirection::Right {
                        left
                    } else {
                        right
                    };
                    let other_node = other_port.node;
                    let root_node = root_port.node;
                    let other_root = layout.root[ni.node_id[other_node]];
                    layout.y[other_root].unwrap_or(0.0)
                        + layout.inner_shift[ni.node_id[other_node]]
                        + port_position_y(graph, other_port)
                        + port_anchor_y(graph, other_port)
                        - layout.inner_shift[ni.node_id[root_node]]
                        - port_position_y(graph, root_port)
                        - port_anchor_y(graph, root_port)
                } else {
                    let root_port = if layout.hdir == HDirection::Left {
                        right
                    } else {
                        left
                    };
                    let other_port = if layout.hdir == HDirection::Left {
                        left
                    } else {
                        right
                    };
                    let other_node = other_port.node;
                    let root_node = root_port.node;
                    let other_root = layout.root[ni.node_id[other_node]];
                    layout.y[other_root].unwrap_or(0.0)
                        + layout.inner_shift[ni.node_id[other_node]]
                        + port_position_y(graph, other_port)
                        + port_anchor_y(graph, other_port)
                        - layout.inner_shift[ni.node_id[root_node]]
                        - port_position_y(graph, root_port)
                        - port_anchor_y(graph, root_port)
                };

                layout.straightened[layout.root[ni.node_id[left.node]]] = true;
                layout.straightened[layout.root[ni.node_id[right.node]]] = true;
                threshold
            }
            None => invalid,
        }
    }

    fn pick_edge(
        &self,
        graph: &LGraph,
        ni: &NeighborhoodInformation,
        layout: &BKAlignedLayout,
        mut postprocessable: Postprocessable,
    ) -> Postprocessable {
        let free_node = ni.id_to_node[postprocessable.free];
        let edges = if postprocessable.is_root {
            if layout.hdir == HDirection::Right {
                graph.node_incoming_edges(free_node)
            } else {
                graph.node_outgoing_edges(free_node)
            }
        } else if layout.hdir == HDirection::Left {
            graph.node_incoming_edges(free_node)
        } else {
            graph.node_outgoing_edges(free_node)
        };

        let mut has_edges = false;
        for edge in edges {
            let only_dummies = layout.only_dummies[layout.root[postprocessable.free]];
            if !only_dummies && is_in_layer_edge_index(graph, edge) {
                continue;
            }
            if layout.straightened[layout.root[postprocessable.free]] {
                continue;
            }
            has_edges = true;

            let Some(other) = edge_opposite_node(&graph.edges[edge], free_node) else {
                continue;
            };
            if ni.node_id.get(other).copied().unwrap_or(usize::MAX) == usize::MAX {
                continue;
            }
            let other_root = layout.root[ni.node_id[other]];
            if self.block_finished.contains(&other_root) {
                postprocessable.has_edges = true;
                postprocessable.edge = Some(edge);
                return postprocessable;
            }
        }

        postprocessable.has_edges = has_edges;
        postprocessable.edge = None;
        postprocessable
    }

    fn post_process(
        &mut self,
        graph: &LGraph,
        ni: &NeighborhoodInformation,
        layout: &mut BKAlignedLayout,
    ) {
        while let Some(pp) = self.post_processables_queue.pop_front() {
            let pick = self.pick_edge(graph, ni, layout, pp);
            let Some(edge) = pick.edge else {
                continue;
            };

            let only_dummies = layout.only_dummies[layout.root[pick.free]];
            if !only_dummies && is_in_layer_edge_index(graph, edge) {
                continue;
            }

            if !self.process(graph, ni, layout, pick) {
                self.post_processables_stack.push(Postprocessable {
                    edge: Some(edge),
                    ..pick
                });
            }
        }

        while let Some(pp) = self.post_processables_stack.pop() {
            self.process(graph, ni, layout, pp);
        }
    }

    fn process(
        &self,
        graph: &LGraph,
        ni: &NeighborhoodInformation,
        layout: &mut BKAlignedLayout,
        pp: Postprocessable,
    ) -> bool {
        let Some(edge) = pp.edge else {
            return false;
        };
        let free_node = ni.id_to_node[pp.free];
        let edge_data = &graph.edges[edge];
        let (fix, block) = if edge_data.source().node == free_node {
            (edge_data.target(), edge_data.source())
        } else {
            (edge_data.source(), edge_data.target())
        };

        let delta = calculate_delta(graph, ni, layout, fix, block);
        if delta > 0.0 && delta < f64::MAX {
            let available_space = check_space_above(graph, ni, layout, block.node, delta);
            let block_root = ni.id_to_node[layout.root[ni.node_id[block.node]]];
            shift_block(ni, layout, block_root, -available_space);
            available_space > 0.0
        } else if delta < 0.0 && -delta < f64::MAX {
            let available_space = check_space_below(graph, ni, layout, block.node, -delta);
            let block_root = ni.id_to_node[layout.root[ni.node_id[block.node]]];
            shift_block(ni, layout, block_root, available_space);
            available_space > 0.0
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Postprocessable {
    free: usize,
    is_root: bool,
    has_edges: bool,
    edge: Option<EdgeId>,
}

impl Postprocessable {
    fn new(free: usize, is_root: bool) -> Self {
        Self {
            free,
            is_root,
            has_edges: false,
            edge: None,
        }
    }
}
