//! Port of ELK's `SelfLoopPort`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/loops/SelfLoopPort.java

use crate::graph::{PortId, SelfHyperLoop};

pub(crate) fn self_loop_port_net_flow(hyper_loop: &SelfHyperLoop, port: PortId) -> isize {
    let incoming = hyper_loop
        .edges
        .iter()
        .filter(|edge| edge.target_port == port)
        .count() as isize;
    let outgoing = hyper_loop
        .edges
        .iter()
        .filter(|edge| edge.source_port == port)
        .count() as isize;
    incoming - outgoing
}
