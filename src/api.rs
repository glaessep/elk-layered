//! The graph types of the public API, shaped like elkjs's JSON graph.
//!
//! One set of types serves as input and output. [`crate::layout`] takes a graph and returns
//! the same graph with every position filled in. With the `serde` feature, the types read and
//! write the JSON elkjs reads and writes: field names in camel case, missing numbers as zero,
//! missing lists as empty, and empty lists left out.
//!
//! elkjs also reads the options from a legacy `properties` key (`transformProperties` in the
//! importer below). Every type that has `layout_options` takes that key as an alias. elkjs
//! reads `properties` only when `layoutOptions` is missing. A graph with both keys is a serde
//! error here, where elkjs would quietly ignore `properties`.
//!
//! Source: `org.eclipse.elk.graph.json/src/org/eclipse/elk/graph/json/JsonImporter.xtend`,
//! which reads this JSON and writes the layout back into it.

use std::collections::BTreeMap;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// A node: the root of a graph, or a node in it.
///
/// Coordinates are relative to the parent node. The root's `x` and `y` stay as given.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct ElkNode {
    /// Unique among all nodes and ports of the graph.
    pub id: String,
    /// Left side, relative to the parent node.
    #[cfg_attr(feature = "serde", serde(default))]
    pub x: f64,
    /// Top side, relative to the parent node.
    #[cfg_attr(feature = "serde", serde(default))]
    pub y: f64,
    /// Width. Layout may change it for a node with children or with size constraints.
    #[cfg_attr(feature = "serde", serde(default))]
    pub width: f64,
    /// Height. Layout may change it like the width.
    #[cfg_attr(feature = "serde", serde(default))]
    pub height: f64,
    /// The nodes inside this one.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub children: Vec<ElkNode>,
    /// The node's ports. elk-layered does not support ports yet.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub ports: Vec<ElkPort>,
    /// Edges given with this node. Layout moves each to the node ELK chooses to contain it,
    /// and names that node in [`ElkEdge::container`], but keeps it in this list.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub edges: Vec<ElkEdge>,
    /// The node's labels.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub labels: Vec<ElkLabel>,
    /// Layout options by their ELK names, such as `elk.direction`.
    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            alias = "properties",
            skip_serializing_if = "BTreeMap::is_empty",
            deserialize_with = "options::deserialize"
        )
    )]
    pub layout_options: BTreeMap<String, String>,
}

/// A port on a node. elk-layered does not support ports yet; a graph with one is refused.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct ElkPort {
    /// Unique among all nodes and ports of the graph.
    pub id: String,
    /// Left side, relative to the port's node.
    #[cfg_attr(feature = "serde", serde(default))]
    pub x: f64,
    /// Top side, relative to the port's node.
    #[cfg_attr(feature = "serde", serde(default))]
    pub y: f64,
    /// Width.
    #[cfg_attr(feature = "serde", serde(default))]
    pub width: f64,
    /// Height.
    #[cfg_attr(feature = "serde", serde(default))]
    pub height: f64,
    /// The port's labels.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub labels: Vec<ElkLabel>,
    /// Layout options by their ELK names.
    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            alias = "properties",
            skip_serializing_if = "BTreeMap::is_empty",
            deserialize_with = "options::deserialize"
        )
    )]
    pub layout_options: BTreeMap<String, String>,
}

/// An edge.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct ElkEdge {
    /// Unique among all edges of the graph.
    pub id: String,
    /// The ids of the edge's source nodes. elk-layered supports exactly one.
    #[cfg_attr(feature = "serde", serde(default))]
    pub sources: Vec<String>,
    /// The ids of the edge's target nodes. elk-layered supports exactly one.
    #[cfg_attr(feature = "serde", serde(default))]
    pub targets: Vec<String>,
    /// The edge's labels. Their coordinates are relative to [`ElkEdge::container`].
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub labels: Vec<ElkLabel>,
    /// Layout options by their ELK names.
    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            alias = "properties",
            skip_serializing_if = "BTreeMap::is_empty",
            deserialize_with = "options::deserialize"
        )
    )]
    pub layout_options: BTreeMap<String, String>,
    /// The route. Layout writes one section; its coordinates are relative to
    /// [`ElkEdge::container`].
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub sections: Vec<ElkEdgeSection>,
    /// The id of the node whose coordinate system the route uses. Layout sets it.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub container: Option<String>,
    /// Where the edge splits or joins other edges, relative to [`ElkEdge::container`].
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub junction_points: Vec<ElkPoint>,
}

/// One piece of an edge's route.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct ElkEdgeSection {
    /// The section's id. Layout names a new section `<edge id>_s0`.
    pub id: String,
    /// Where the route starts.
    #[cfg_attr(feature = "serde", serde(default))]
    pub start_point: ElkPoint,
    /// Where the route ends.
    #[cfg_attr(feature = "serde", serde(default))]
    pub end_point: ElkPoint,
    /// The bends between start and end.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub bend_points: Vec<ElkPoint>,
    /// The id of the shape the section starts at.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub incoming_shape: Option<String>,
    /// The id of the shape the section ends at.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub outgoing_shape: Option<String>,
}

/// A label of a node, port or edge.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct ElkLabel {
    /// An optional id.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub id: Option<String>,
    /// The text. ELK places only labels with text.
    #[cfg_attr(feature = "serde", serde(default))]
    pub text: String,
    /// Left side, relative to the label's node, or to its edge's container.
    #[cfg_attr(feature = "serde", serde(default))]
    pub x: f64,
    /// Top side, relative like `x`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub y: f64,
    /// Width.
    #[cfg_attr(feature = "serde", serde(default))]
    pub width: f64,
    /// Height.
    #[cfg_attr(feature = "serde", serde(default))]
    pub height: f64,
    /// Layout options by their ELK names.
    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            alias = "properties",
            skip_serializing_if = "BTreeMap::is_empty",
            deserialize_with = "options::deserialize"
        )
    )]
    pub layout_options: BTreeMap<String, String>,
}

/// A point.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ElkPoint {
    /// Horizontal coordinate.
    #[cfg_attr(feature = "serde", serde(default))]
    pub x: f64,
    /// Vertical coordinate.
    #[cfg_attr(feature = "serde", serde(default))]
    pub y: f64,
}

#[cfg(feature = "serde")]
mod options {
    use std::collections::BTreeMap;
    use std::fmt;

    use serde::Deserializer;
    use serde::de::{MapAccess, Visitor};

    /// One option value. elkjs turns a number or a boolean into its string, so this does too.
    struct Value(String);

    impl<'de> serde::Deserialize<'de> for Value {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct ValueVisitor;
            impl Visitor<'_> for ValueVisitor {
                type Value = Value;
                fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str("a string, a number or a boolean")
                }
                fn visit_str<E>(self, v: &str) -> Result<Value, E> {
                    Ok(Value(v.to_string()))
                }
                fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
                    Ok(Value(v.to_string()))
                }
                fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
                    Ok(Value(v.to_string()))
                }
                fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
                    Ok(Value(v.to_string()))
                }
                fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
                    Ok(Value(v.to_string()))
                }
            }
            deserializer.deserialize_any(ValueVisitor)
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<String, String>, D::Error> {
        struct MapVisitor;
        impl<'de> Visitor<'de> for MapVisitor {
            type Value = BTreeMap<String, String>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a map of layout options")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut options = BTreeMap::new();
                while let Some((key, Value(value))) = map.next_entry::<String, Value>()? {
                    options.insert(key, value);
                }
                Ok(options)
            }
        }
        deserializer.deserialize_map(MapVisitor)
    }
}
