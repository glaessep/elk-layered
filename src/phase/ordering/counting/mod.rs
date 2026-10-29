//! The crossing counters of the layer sweep, one file per ELK class.
//!
//! Source: https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/counting

pub(crate) mod all_crossings_counter;
pub(crate) mod binary_indexed_tree;
pub(crate) mod crossings_counter;
pub(crate) mod hyperedge_crossings_counter;
#[cfg(test)]
mod tests;

pub use crossings_counter::CrossingsCounter;
