//! The graph types of the public API, shaped like elkjs's JSON graph.
//!
//! One set of types serves as input and output. [`crate::layout`] takes a graph and returns
//! the same graph with every position filled in. With the `serde` feature, the types read and
//! write the JSON elkjs reads and writes: field names in camel case, missing numbers as zero,
//! missing lists as empty, and empty lists left out.
//!
//! Parse the JSON with an exact number reader. JavaScript reads every number to the nearest
//! double. Plain `serde_json` may read one bit off, and one bit in the input can change the
//! layout, so it no longer matches elkjs. Turn on the `float_roundtrip` feature of `serde_json`,
//! or use another parser that reads to the nearest double. This crate sets the feature only for
//! its own tests.
//!
//! elkjs also reads the options from a legacy `properties` key (`transformProperties` in the
//! importer below). Every type that has `layout_options` takes that key as an alias. elkjs
//! reads `properties` only when `layoutOptions` is missing. A graph with both keys is a serde
//! error here, where elkjs would quietly ignore `properties`.
//!
//! elkjs reads an edge in an old form too: a single `source` and `target` string, with an
//! optional `sourcePort` and `targetPort`. elkjs picks the old form only when neither `sources`
//! nor `targets` is present. Here, `source` and `target` read as one-element `sources` and
//! `targets`, and the port keys go to [`ElkEdge::old_form`]. Layout then follows
//! `transformPrimitiveEdge`: `source` and `target` must name nodes, a port key names a port of
//! that node, and a port key that names no port leaves the edge at the node. The JSON writer
//! writes such an edge in the old form again, as elkjs writes its layout into the input. In the new form,
//! elkjs ignores both port keys, as this crate does. A graph that gives both `source` and
//! `sources` is a serde error, and `source` next to `targets` reads as one source.
//!
//! Source: `org.eclipse.elk.graph.json/src/org/eclipse/elk/graph/json/JsonImporter.xtend`,
//! which reads this JSON and writes the layout back into it.

use std::collections::BTreeMap;

use crate::graph::{LPoint, LSize, port_box};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// A node: the root of a graph, or a node in it.
///
/// Coordinates are relative to the parent node. The root's `x` and `y` stay as given.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct ElkNode {
    /// Unique among all nodes of the graph. A port may have the same id: an edge end that names
    /// both means the node.
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
    /// The node's ports.
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

/// A port on a node.
///
/// Layout writes the port's `x` and `y`, relative to its node, and places its labels unless
/// the port label placement of its node is fixed. A label on a port of a node with children,
/// and an edge between a port of the root and a node inside it, are refused for now.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct ElkPort {
    /// Unique among all ports of the graph. A node may have the same id: an edge end that names
    /// both means the node.
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

impl ElkPort {
    /// Where the port's top left corner lies, relative to its node, and the port's width and
    /// height.
    pub(crate) fn bounds(&self) -> (LPoint, LSize) {
        port_box(self.x, self.y, self.width, self.height)
    }
}

/// An edge.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[cfg_attr(feature = "serde", serde(try_from = "EdgeWire", into = "EdgeOut"))]
pub struct ElkEdge {
    /// Unique among all edges of the graph.
    pub id: String,
    /// The ids of the edge's source nodes or ports. elk-layered supports exactly one.
    ///
    /// The JSON may also give the old form: a single `source` string, or a `sourcePort`
    /// (`transformPrimitiveEdge` in the importer).
    #[cfg_attr(
        feature = "serde",
        serde(default, deserialize_with = "ends::deserialize")
    )]
    pub sources: Vec<String>,
    /// The ids of the edge's target nodes or ports. elk-layered supports exactly one.
    ///
    /// The JSON may also give the old form: a single `target` string, or a `targetPort`.
    #[cfg_attr(
        feature = "serde",
        serde(default, deserialize_with = "ends::deserialize")
    )]
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
    /// The port keys of an edge in elkjs's old JSON form, or `None` for the new form. When it is
    /// set, `sources` and `targets` name nodes only, as `transformPrimitiveEdge` reads them. The
    /// JSON reader sets it, and the JSON writer then writes the edge in the old form again.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub old_form: Option<OldFormPorts>,
}

/// The `sourcePort` and `targetPort` keys of an edge in elkjs's old JSON form.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OldFormPorts {
    /// The port of the source node the edge starts at. `None`, or an id that is no port, leaves
    /// the edge at the node.
    pub source_port: Option<String>,
    /// The port of the target node the edge ends at, read the same way.
    pub target_port: Option<String>,
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

/// What `ElkEdge` reads from JSON. It tells `source` from `sources`, and has the two old port
/// keys, because elkjs reads those keys only in the old form.
#[cfg(feature = "serde")]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EdgeWire {
    id: String,
    #[serde(default, deserialize_with = "ends::deserialize_some")]
    sources: Option<Vec<String>>,
    #[serde(default, deserialize_with = "ends::deserialize_some")]
    source: Option<Vec<String>>,
    #[serde(default, deserialize_with = "ends::deserialize_some")]
    targets: Option<Vec<String>>,
    #[serde(default, deserialize_with = "ends::deserialize_some")]
    target: Option<Vec<String>>,
    #[serde(default)]
    source_port: Option<String>,
    #[serde(default)]
    target_port: Option<String>,
    #[serde(default)]
    labels: Vec<ElkLabel>,
    #[serde(
        default,
        alias = "properties",
        deserialize_with = "options::deserialize"
    )]
    layout_options: BTreeMap<String, String>,
    #[serde(default)]
    sections: Vec<ElkEdgeSection>,
    #[serde(default)]
    container: Option<String>,
    #[serde(default)]
    junction_points: Vec<ElkPoint>,
}

#[cfg(feature = "serde")]
impl TryFrom<EdgeWire> for ElkEdge {
    type Error = String;

    /// Ports the old form of `JsonImporter.transformPrimitiveEdge`, which elkjs picks when neither
    /// `sources` nor `targets` is present. The port keys stay apart from the node ids, so that
    /// layout can check them as elkjs does.
    fn try_from(wire: EdgeWire) -> Result<Self, Self::Error> {
        let old_form = wire.sources.is_none() && wire.targets.is_none();
        let id = wire.id;
        let end = |plural: Option<Vec<String>>, single: Option<Vec<String>>, name: &str| match (
            plural, single,
        ) {
            (Some(_), Some(_)) => Err(format!(
                "edge \"{id}\": both \"{name}s\" and \"{name}\" are given"
            )),
            (Some(ids), None) => Ok(ids),
            (None, single) => Ok(single.unwrap_or_default()),
        };
        let sources = end(wire.sources, wire.source, "source")?;
        let targets = end(wire.targets, wire.target, "target")?;
        Ok(Self {
            id: id.clone(),
            sources,
            targets,
            labels: wire.labels,
            layout_options: wire.layout_options,
            sections: wire.sections,
            container: wire.container,
            junction_points: wire.junction_points,
            old_form: old_form.then_some(OldFormPorts {
                source_port: wire.source_port,
                target_port: wire.target_port,
            }),
        })
    }
}

/// What `ElkEdge` writes to JSON. elkjs writes its layout into the input object, so an edge read
/// in the old form comes back with the keys it was read with. Writing it in the new form would
/// lose its port keys, and reading the result again would attach the edge elsewhere.
#[cfg(feature = "serde")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EdgeOut {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    sources: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<OldEnd>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    targets: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<OldEnd>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_port: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    labels: Vec<ElkLabel>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    layout_options: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    sections: Vec<ElkEdgeSection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    container: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    junction_points: Vec<ElkPoint>,
}

/// An old-form `source` or `target`: the one id it was read as, or a list if it was one.
#[cfg(feature = "serde")]
#[derive(Serialize)]
#[serde(untagged)]
enum OldEnd {
    One(String),
    Many(Vec<String>),
}

#[cfg(feature = "serde")]
impl From<ElkEdge> for EdgeOut {
    fn from(edge: ElkEdge) -> Self {
        let old_end = |mut ids: Vec<String>| match ids.len() {
            0 => None,
            1 => ids.pop().map(OldEnd::One),
            _ => Some(OldEnd::Many(ids)),
        };
        let (sources, source, source_port, targets, target, target_port) = match edge.old_form {
            None => (
                Some(edge.sources),
                None,
                None,
                Some(edge.targets),
                None,
                None,
            ),
            Some(ports) => (
                None,
                old_end(edge.sources),
                ports.source_port,
                None,
                old_end(edge.targets),
                ports.target_port,
            ),
        };
        Self {
            id: edge.id,
            sources,
            source,
            source_port,
            targets,
            target,
            target_port,
            labels: edge.labels,
            layout_options: edge.layout_options,
            sections: edge.sections,
            container: edge.container,
            junction_points: edge.junction_points,
        }
    }
}

#[cfg(feature = "serde")]
mod ends {
    use std::fmt;

    use serde::Deserializer;
    use serde::de::{SeqAccess, Visitor};

    /// Like [`deserialize`], for a key that may be missing.
    pub(super) fn deserialize_some<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Vec<String>>, D::Error> {
        deserialize(deserializer).map(Some)
    }

    /// A list of ids, or a single id string for the old `source` and `target` keys.
    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<String>, D::Error> {
        struct EndsVisitor;
        impl<'de> Visitor<'de> for EndsVisitor {
            type Value = Vec<String>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an id or a list of ids")
            }
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
                Ok(vec![v.to_string()])
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut ids = Vec::new();
                while let Some(id) = seq.next_element::<String>()? {
                    ids.push(id);
                }
                Ok(ids)
            }
        }
        deserializer.deserialize_any(EndsVisitor)
    }
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
