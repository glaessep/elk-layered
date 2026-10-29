//! The self-loop processors and the classes they share, one file per ELK class.
//!
//! Source: https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/loops

pub(crate) mod ordering;
pub(crate) mod routing;
pub(crate) mod self_hyper_loop;
pub(crate) mod self_hyper_loop_labels;
pub(crate) mod self_loop_holder;
pub(crate) mod self_loop_port;
pub(crate) mod self_loop_port_restorer;
pub(crate) mod self_loop_post_processor;
pub(crate) mod self_loop_pre_processor;
pub(crate) mod self_loop_router;
pub(crate) mod self_loop_type;
#[cfg(test)]
mod tests;

pub use self_loop_port_restorer::restore_self_loop_ports;
pub use self_loop_post_processor::postprocess_self_loops;
pub use self_loop_pre_processor::preprocess_self_loops;
pub use self_loop_router::route_self_loops;
