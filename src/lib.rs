//! Eclipse ELK's layered layout algorithm, in Rust.
//!
//! [`layout`] takes a graph shaped like elkjs's JSON graph and returns the same graph with
//! every position filled in, exactly as elkjs 0.9.3 returns it. With the `serde` feature, the
//! graph types read and write that JSON.
//!
//! ```
//! use elk_layered::{ElkEdge, ElkNode, layout};
//!
//! let node = |id: &str| ElkNode {
//!     id: id.into(),
//!     width: 60.0,
//!     height: 30.0,
//!     ..Default::default()
//! };
//! let graph = ElkNode {
//!     id: "root".into(),
//!     children: vec![node("a"), node("b")],
//!     edges: vec![ElkEdge {
//!         id: "e".into(),
//!         sources: vec!["a".into()],
//!         targets: vec!["b".into()],
//!         ..Default::default()
//!     }],
//!     ..Default::default()
//! };
//! let laid_out = layout(&graph).unwrap();
//! assert_eq!((laid_out.children[1].x, laid_out.children[1].y), (92.0, 12.0));
//! ```
//!
//! The port follows ELK's Java at commit `62d5909`:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/LayeredPhases.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/GraphConfigurator.java
//! - https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/graph

mod api;
mod common;
mod components;
mod compound;
mod configurator;
mod error;
mod export;
mod graph;
mod importer;
mod intermediate;
mod layout;
mod layout_options;
mod options;
mod p3order;
mod p5edges;
mod phase;
mod pipeline;
mod random;
mod selfloop;

#[cfg(test)]
mod snapshot_tests;

pub use api::{ElkEdge, ElkEdgeSection, ElkLabel, ElkNode, ElkPoint, ElkPort, OldFormPorts};
pub use error::Error;
pub use layout::layout;

/// The layered graph and its phases are not public: `layout` is the only way in. This must
/// fail to compile.
///
/// ```compile_fail
/// use elk_layered::LGraph;
/// ```
const _INTERNALS_ARE_NOT_PUBLIC: () = ();
