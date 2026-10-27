//! The error [`crate::layout`] returns.

/// Why a graph could not be laid out.
///
/// `element` fields describe where the problem is, such as `node "g"`, `edge "e0"` or
/// `label 1 of edge "e0"`.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The graph uses something elk-layered does not support yet, such as a port with a label.
    #[error("{element}: {feature} is not supported")]
    Unsupported {
        /// Where the graph uses it.
        element: String,
        /// What it is.
        feature: String,
    },
    /// Two nodes or ports, or two edges, share an id.
    #[error("the id \"{id}\" is used more than once")]
    DuplicateId {
        /// The id.
        id: String,
    },
    /// An edge names a node or port the graph does not have.
    #[error("edge \"{edge}\" names \"{id}\", which is no node or port of the graph")]
    UnknownId {
        /// The edge's id.
        edge: String,
        /// The id it names.
        id: String,
    },
    /// An edge has no source or no target.
    #[error("edge \"{edge}\" needs a source and a target")]
    MissingEdgeEnd {
        /// The edge's id.
        edge: String,
    },
    /// A size or a coordinate is not a finite number, or a size is negative.
    #[error("{element}: {field} is {value}, which is no valid size")]
    InvalidSize {
        /// Where the number is.
        element: String,
        /// Which number it is, such as `width`.
        field: &'static str,
        /// The number.
        value: f64,
    },
    /// `randomSeed` is 0 on a graph that is laid out. ELK then seeds from the clock, which
    /// would make the layout differ from run to run.
    #[error("node \"{graph}\": elk.randomSeed 0 asks for a seed from the clock")]
    ZeroRandomSeed {
        /// The id of the node whose graph has the seed.
        graph: String,
    },
    /// A layout option key names no ELK option, or names more than one.
    #[error("{element}: \"{key}\" is no ELK layout option")]
    UnknownOption {
        /// Where the option is.
        element: String,
        /// The key.
        key: String,
    },
    /// A layout option is an ELK option elk-layered does not support, at least not on this
    /// element or with this value.
    #[error("{element}: the option \"{key}\" = \"{value}\" is not supported: {reason}")]
    UnsupportedOption {
        /// Where the option is.
        element: String,
        /// The key.
        key: String,
        /// The value.
        value: String,
        /// Why.
        reason: String,
    },
    /// A layout option's value cannot be read.
    #[error("{element}: the option \"{key}\" has a value that cannot be read: \"{value}\"")]
    InvalidOptionValue {
        /// Where the option is.
        element: String,
        /// The key.
        key: String,
        /// The value.
        value: String,
    },
    /// Two keys name the same layout option.
    #[error("{element}: \"{key}\" and \"{other}\" set the same option")]
    DuplicateOption {
        /// Where the options are.
        element: String,
        /// One key.
        key: String,
        /// The other key.
        other: String,
    },
    /// The graph's numbers are outside what the layout can compute, such as a negative
    /// spacing or a node size too large for ELK's `int` arithmetic.
    #[error("{element}: {reason}")]
    Unsolvable {
        /// Where the numbers are. This names the graph the layout was working on.
        element: String,
        /// What cannot be computed.
        reason: String,
    },
    /// The layout failed inside the algorithm. This points at a bug in elk-layered.
    #[error("layout failed: {message}")]
    Internal {
        /// What went wrong.
        message: String,
    },
}
