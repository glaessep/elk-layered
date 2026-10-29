//! Port of ELK's `ISweepPortDistributor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/ISweepPortDistributor.java
//!
//! An enum stands for the interface and dispatches to the distributors.

use crate::graph::{LGraph, NodeId, OtherGraphs};
use crate::phase::ordering::abstract_barycenter_port_distributor::BarycenterPortDistributor;
use crate::phase::ordering::greedy_port_distributor::GreedyPortDistributor;
use crate::phase::ordering::layer_sweep_crossing_minimizer::CrossMinType;
use crate::random::JavaRandom;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PortDistributorKind {
    NodeRelative,
    LayerTotal,
}

fn create_port_distributor_kind(
    cross_min_type: CrossMinType,
    random: &mut JavaRandom,
) -> PortDistributorKind {
    if cross_min_type == CrossMinType::TwoSidedGreedySwitch || random.next_bool() {
        PortDistributorKind::NodeRelative
    } else {
        PortDistributorKind::LayerTotal
    }
}

/// `GraphInfoHolder.portDistributor`, which `ISweepPortDistributor.create` makes once for each
/// graph: a `GreedyPortDistributor` for the two-sided greedy switch and a barycenter one for the
/// other two types.
#[derive(Debug, Clone)]
pub(crate) enum PortDistributor {
    Greedy(GreedyPortDistributor),
    Barycenter(BarycenterPortDistributor),
    /// The barycenter type hands its distributor to `BarycenterHeuristic`, which distributes the
    /// ports itself.
    InHeuristic,
}

impl PortDistributor {
    /// `ISweepPortDistributor.create`. It draws from `random` as ELK does: nothing for the
    /// two-sided type, and one boolean for the others, which picks the kind of barycenter
    /// distributor.
    pub(crate) fn create(
        cross_min_type: CrossMinType,
        random: &mut JavaRandom,
        current_order: &[Vec<NodeId>],
    ) -> Self {
        if cross_min_type == CrossMinType::TwoSidedGreedySwitch {
            Self::Greedy(GreedyPortDistributor::default())
        } else {
            let kind = create_port_distributor_kind(cross_min_type, random);
            Self::Barycenter(BarycenterPortDistributor::new(kind, current_order))
        }
    }

    /// Hands the barycenter distributor to the heuristic of the barycenter type, which owns it.
    ///
    /// Panics if the distributor is not a barycenter one that is still here. That is a bug in the
    /// caller, never caller input: only the barycenter type hands it over, once.
    pub(crate) fn hand_to_heuristic(&mut self) -> BarycenterPortDistributor {
        match std::mem::replace(self, Self::InHeuristic) {
            Self::Barycenter(distributor) => distributor,
            _ => {
                panic!("only a barycenter port distributor that is still here goes to a heuristic")
            }
        }
    }

    pub(crate) fn distribute_ports_while_sweeping(
        &mut self,
        graph: &mut LGraph,
        others: OtherGraphs<'_>,
        order: &[Vec<NodeId>],
        current_index: usize,
        forward: bool,
    ) -> bool {
        match self {
            Self::Greedy(distributor) => distributor.distribute_ports_while_sweeping(
                graph,
                others,
                order,
                current_index,
                forward,
            ),
            Self::Barycenter(distributor) => {
                distributor.distribute_ports_while_sweeping(graph, order, current_index, forward);
                // `AbstractBarycenterPortDistributor` cannot tell whether it improved anything.
                false
            }
            Self::InHeuristic => false,
        }
    }
}
