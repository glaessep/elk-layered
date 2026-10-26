//! `layout` never panics, never hangs, and gives the same answer twice, on any graph a caller
//! can build.
//!
//! The test builds thousands of random graphs from a fixed seed. About half are valid; the rest
//! carry one defect each, such as a duplicate id, a dangling edge, a NaN size or an option value
//! ELK does not know. Each graph is laid out twice, on a worker thread with a time limit. A run
//! passes when both results are equal and neither run panicked or ran out of time. An `Ok` and an
//! `Error` both pass: this test is about the shape of the answer, not about the layout.
//!
//! A cycle in the parent chain cannot be built: an `ElkNode` owns its children, so the type
//! rules it out.
//!
//! A failure names the graph's number and prints the graph, so the case can be replayed.

use std::collections::BTreeMap;
use std::sync::mpsc;
use std::time::Duration;

use elk_layered::{ElkEdge, ElkLabel, ElkNode, ElkPort, layout};

/// How many graphs one run of the test builds.
const GRAPHS: u64 = 2000;

/// How long one layout may take before the test calls it a hang. A valid graph here lays out in
/// well under a millisecond in a release build and in a few milliseconds in a debug build.
const TIME_LIMIT: Duration = Duration::from_secs(10);

/// xorshift64*: a small generator with a fixed seed. Its numbers only pick the graphs.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// A number from `lo` to `hi`, both included.
    fn int(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo + 1)
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.int(0, items.len() as u64 - 1) as usize]
    }

    fn size(&mut self) -> f64 {
        self.int(10, 240) as f64 / 2.0
    }
}

/// Options on the root, with values ELK knows. Each is set now and then.
const ROOT_OPTIONS: &[(&str, &[&str])] = &[
    (
        "elk.direction",
        &["RIGHT", "LEFT", "DOWN", "UP", "UNDEFINED"],
    ),
    (
        "elk.hierarchyHandling",
        &["INCLUDE_CHILDREN", "SEPARATE_CHILDREN", "INHERIT"],
    ),
    (
        "elk.layered.considerModelOrder.strategy",
        &["NONE", "NODES_AND_EDGES", "PREFER_EDGES", "PREFER_NODES"],
    ),
    (
        "elk.layered.cycleBreaking.strategy",
        &["GREEDY", "DEPTH_FIRST", "MODEL_ORDER"],
    ),
    (
        "elk.layered.layering.strategy",
        &[
            "NETWORK_SIMPLEX",
            "LONGEST_PATH",
            "LONGEST_PATH_SOURCE",
            "COFFMAN_GRAHAM",
            "STRETCH_WIDTH",
            "MIN_WIDTH",
        ],
    ),
    (
        "elk.layered.crossingMinimization.strategy",
        &["LAYER_SWEEP", "NONE"],
    ),
    (
        "elk.layered.nodePlacement.strategy",
        &["SIMPLE", "BRANDES_KOEPF", "NETWORK_SIMPLEX"],
    ),
    ("elk.edgeRouting", &["ORTHOGONAL"]),
    ("elk.randomSeed", &["1", "7", "-3", "2147483647"]),
    ("elk.spacing.nodeNode", &["0", "5", "20", "80"]),
    (
        "elk.layered.spacing.nodeNodeBetweenLayers",
        &["0", "10", "60"],
    ),
    ("elk.separateConnectedComponents", &["true", "false"]),
    (
        "elk.padding",
        &[
            "[top=0,left=0,bottom=0,right=0]",
            "[top=20,left=5,bottom=1,right=9]",
        ],
    ),
    ("elk.aspectRatio", &["0.5", "1.6", "4"]),
    ("elk.layered.thoroughness", &["1", "7", "20"]),
];

/// A random graph that ELK can lay out, or that fails only on a feature `layout` reports as not
/// ported: 3 to 15 leaves, up to three groups nested up to two deep, edges between any two
/// leaves, self-loops and parallel edges among them, and labels on nodes, groups and edges.
fn valid_graph(rng: &mut Rng) -> ElkNode {
    let mut options = BTreeMap::from([("elk.algorithm".to_string(), "layered".to_string())]);
    for (key, values) in ROOT_OPTIONS {
        if rng.chance(25) {
            options.insert(key.to_string(), rng.pick(values).to_string());
        }
    }
    let mut root = ElkNode {
        id: "root".to_string(),
        layout_options: options,
        ..ElkNode::default()
    };

    // Paths from the root to each group, as child indices.
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for g in 0..rng.int(0, 3) {
        let parent = if groups.is_empty() || rng.chance(50) {
            Vec::new()
        } else {
            groups[rng.int(0, groups.len() as u64 - 1) as usize].clone()
        };
        if parent.len() >= 2 {
            continue;
        }
        let mut group = ElkNode {
            id: format!("g{g}"),
            ..ElkNode::default()
        };
        if rng.chance(40) {
            group.labels.push(label(rng, "Group title"));
            group.layout_options.insert(
                "elk.nodeLabels.placement".to_string(),
                "[H_LEFT, V_TOP, INSIDE]".to_string(),
            );
            group.layout_options.insert(
                "elk.nodeSize.constraints".to_string(),
                "[NODE_LABELS]".to_string(),
            );
        }
        let holder = at(&mut root, &parent);
        holder.children.push(group);
        let mut path = parent;
        path.push(holder.children.len() - 1);
        groups.push(path);
    }

    let mut leaves = Vec::new();
    for n in 0..rng.int(3, 15) {
        let parent = if !groups.is_empty() && rng.chance(60) {
            groups[rng.int(0, groups.len() as u64 - 1) as usize].clone()
        } else {
            Vec::new()
        };
        let mut node = ElkNode {
            id: format!("n{n}"),
            width: rng.size(),
            height: rng.size(),
            ..ElkNode::default()
        };
        if rng.chance(15) {
            node.labels.push(label(rng, "A"));
            node.layout_options.insert(
                "elk.nodeLabels.placement".to_string(),
                "[H_CENTER, V_CENTER, INSIDE]".to_string(),
            );
        }
        at(&mut root, &parent).children.push(node);
        leaves.push(format!("n{n}"));
    }

    for e in 0..rng.int(0, 2 * leaves.len() as u64) {
        let source = leaves[rng.int(0, leaves.len() as u64 - 1) as usize].clone();
        let target = leaves[rng.int(0, leaves.len() as u64 - 1) as usize].clone();
        let mut edge = ElkEdge {
            id: format!("e{e}"),
            sources: vec![source],
            targets: vec![target],
            ..ElkEdge::default()
        };
        if rng.chance(25) {
            let mut l = label(rng, "label");
            let placement = rng.pick(&["CENTER", "HEAD", "TAIL"]);
            l.layout_options.insert(
                "elk.edgeLabels.placement".to_string(),
                placement.to_string(),
            );
            edge.labels.push(l);
        }
        // Edges sit in the root or in a group; ELK moves each to where its ends meet.
        let holder = if !groups.is_empty() && rng.chance(30) {
            groups[rng.int(0, groups.len() as u64 - 1) as usize].clone()
        } else {
            Vec::new()
        };
        at(&mut root, &holder).edges.push(edge);
    }
    root
}

fn label(rng: &mut Rng, text: &str) -> ElkLabel {
    ElkLabel {
        text: text.to_string(),
        width: rng.size() / 2.0,
        height: 14.0,
        ..ElkLabel::default()
    }
}

fn at<'a>(root: &'a mut ElkNode, path: &[usize]) -> &'a mut ElkNode {
    path.iter().fold(root, |node, &i| &mut node.children[i])
}

/// The path to every node in the graph, the root's empty path first.
fn all_paths(node: &ElkNode) -> Vec<Vec<usize>> {
    let mut out = vec![Vec::new()];
    let mut stack = vec![(node, Vec::new())];
    while let Some((node, path)) = stack.pop() {
        for (i, child) in node.children.iter().enumerate() {
            let mut child_path = path.clone();
            child_path.push(i);
            out.push(child_path.clone());
            stack.push((child, child_path));
        }
    }
    out
}

/// A graph with one defect from the list below. Some defects are only odd, not wrong (an empty
/// id, a size of zero); `layout` may then lay the graph out.
fn broken_graph(rng: &mut Rng) -> ElkNode {
    let mut graph = valid_graph(rng);
    let defect = rng.int(0, 16);
    let paths = all_paths(&graph);
    let path = paths[rng.int(0, paths.len() as u64 - 1) as usize].clone();
    let node = at(&mut graph, &path);
    match defect {
        0 => node.id = "root".to_string(),
        1 => node.id = String::new(),
        2 => node.width = f64::NAN,
        3 => node.height = -rng.size(),
        4 => node.width = f64::INFINITY,
        5 => node.x = f64::NEG_INFINITY,
        6 => node.height = 1e300,
        7 => node.width = 0.0,
        8 => node.edges.push(ElkEdge {
            id: "dangling".to_string(),
            sources: vec!["n0".to_string()],
            targets: vec!["nowhere".to_string()],
            ..ElkEdge::default()
        }),
        9 => node.edges.push(ElkEdge {
            id: "half".to_string(),
            sources: vec!["n0".to_string()],
            ..ElkEdge::default()
        }),
        10 => node.edges.push(ElkEdge {
            id: "hyper".to_string(),
            sources: vec!["n0".to_string(), "n1".to_string()],
            targets: vec!["n2".to_string()],
            ..ElkEdge::default()
        }),
        11 => node.edges.push(ElkEdge {
            // An edge from a group, or the root, to a node inside it.
            id: "inward".to_string(),
            sources: vec![node.id.clone()],
            targets: vec!["n0".to_string()],
            ..ElkEdge::default()
        }),
        12 => {
            node.layout_options.insert(
                rng.pick(&[
                    "elk.direction",
                    "elk.spacing.nodeNode",
                    "elk.padding",
                    "elk.randomSeed",
                ])
                .to_string(),
                rng.pick(&["SIDEWAYS", "-1", "NaN", "", "[top=x]", "0", "1e999"])
                    .to_string(),
            );
        }
        13 => {
            node.layout_options
                .insert("elk.noSuchOption".to_string(), "1".to_string());
        }
        14 => node.ports.push(ElkPort {
            id: "p".to_string(),
            width: 8.0,
            height: 8.0,
            ..ElkPort::default()
        }),
        15 => node.labels.push(ElkLabel {
            text: "nan".to_string(),
            width: f64::NAN,
            height: -1.0,
            ..ElkLabel::default()
        }),
        _ => {
            // Values ELK knows and `layout` reports as not ported.
            let (key, value) = [
                ("elk.edgeRouting", "POLYLINE"),
                ("elk.edgeRouting", "SPLINES"),
                ("elk.layered.nodePlacement.strategy", "LINEAR_SEGMENTS"),
                ("elk.layered.layering.strategy", "BF_MODEL_ORDER"),
                ("elk.layered.layering.strategy", "DF_MODEL_ORDER"),
                ("elk.layered.cycleBreaking.strategy", "INTERACTIVE"),
            ][rng.int(0, 5) as usize];
            node.layout_options
                .insert(key.to_string(), value.to_string());
        }
    }
    graph
}

/// The result as text, so two results compare equal even where they hold a NaN.
fn answer(graph: &ElkNode) -> String {
    match layout(graph) {
        Ok(laid) => format!("Ok({laid:?})"),
        Err(error) => format!("Err({error})"),
    }
}

/// Lays `graph` out twice on a worker thread, and fails the test if a run panics, takes longer
/// than [`TIME_LIMIT`], or the two runs differ. Returns whether the layout succeeded.
fn check(number: u64, graph: ElkNode) -> bool {
    let (send, receive) = mpsc::channel();
    let input = graph.clone();
    std::thread::spawn(move || {
        let first = answer(&input);
        let second = answer(&input);
        let _ = send.send((first, second));
    });
    match receive.recv_timeout(TIME_LIMIT) {
        Ok((first, second)) => {
            assert_eq!(
                first, second,
                "graph {number} gave two different answers. The graph: {graph:?}"
            );
            first.starts_with("Ok")
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("graph {number} took longer than {TIME_LIMIT:?}. The graph: {graph:?}")
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("graph {number} made layout panic. The graph: {graph:?}")
        }
    }
}

// Mutation: read an edge end in src/layout.rs with `by_id.get(id).unwrap()` instead of
// returning `Error::UnknownId`. Graph 14, with its dangling edge, then makes layout panic.
#[test]
fn layout_never_panics_hangs_or_wavers() {
    let mut rng = Rng(0x0e1c_1a7e_4ed0_0001);
    let (mut valid, mut laid_out) = (0, 0);
    for number in 0..GRAPHS {
        if rng.chance(50) {
            valid += 1;
            laid_out += u64::from(check(number, valid_graph(&mut rng)));
        } else {
            check(number, broken_graph(&mut rng));
        }
    }
    // A valid graph fails only on a feature `layout` reports as not ported. If most of them fail,
    // the test no longer reaches the layout steps it is meant to shake.
    assert!(
        laid_out * 10 >= valid * 9,
        "only {laid_out} of {valid} valid graphs were laid out"
    );
}
