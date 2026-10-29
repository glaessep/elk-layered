//! Brandes and Köpf's node placement, one file per ELK class.
//!
//! Source: https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p4nodes/bk

pub(crate) mod bk_aligned_layout;
pub(crate) mod bk_aligner;
pub(crate) mod bk_compactor;
pub(crate) mod bk_node_placer;
pub(crate) mod neighborhood_information;
#[cfg(test)]
mod tests;
pub(crate) mod threshold_strategy;

pub use bk_node_placer::place_nodes_brandes_koepf;
