//! How self loops get their routes, slots and label places.
//!
//! Source: https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/loops/routing

pub(crate) mod label_placer;
pub(crate) mod orthogonal_self_loop_router;
pub(crate) mod routing_director;
pub(crate) mod routing_slot_assigner;
