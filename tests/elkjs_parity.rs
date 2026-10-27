//! elk-layered gives the same layout as elkjs 0.9.3, number for number.
//!
//! Each case in `tests/elkjs/` is an ELK graph in elkjs's JSON: `<case>.input.json`, and beside
//! it `<case>.expected.json`, what elkjs 0.9.3 returned for it. `oracle/record.mjs` records
//! them. This test lays every input out with `layout` and compares the result with elkjs's,
//! field by field. Every number must be equal, with no tolerance: a box that moves by one bit is
//! a different layout. elkjs's internal `$H` hash ids are the only fields left out.
//!
//! A failure names the case and the first path that differs, such as
//! `children[1].edges[3].sections[0].bendPoints[0].y: elkjs 102.0, elk-layered 102.5`.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// Cases whose feature elk-layered does not have yet, each with the issue that adds it. A case
/// leaves this list in the pull request that makes it pass.
const NOT_YET: &[(&str, &str)] = &[
    ("comp-we", "ports in the input, #4"),
    ("comp-ns", "north and south ports, #5"),
];

fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/elkjs")
}

/// Every input: the hand-written cases in `tests/elkjs/`, then the random ones that
/// `oracle/random.mjs` writes: `tests/elkjs/random/`, `tests/elkjs/random-group-ends/`, whose
/// edges may also end at a group, `tests/elkjs/random-ports/`, whose nodes have ports, and
/// `tests/elkjs/random-group-ports/`, whose groups have ports too.
fn cases() -> Vec<PathBuf> {
    let mut inputs = Vec::new();
    for dir in [
        cases_dir(),
        cases_dir().join("random"),
        cases_dir().join("random-group-ends"),
        cases_dir().join("random-ports"),
        cases_dir().join("random-group-ports"),
    ] {
        let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|_| panic!("{} exists", dir.display()))
            .map(|entry| entry.expect("a readable entry").path())
            .filter(|path| path.to_string_lossy().ends_with(".input.json"))
            .collect();
        found.sort();
        inputs.extend(found);
    }
    inputs
}

/// The case's path below `tests/elkjs/` without the suffix, such as `chain` or `random/r007`.
fn name(input: &Path) -> String {
    let relative = input
        .strip_prefix(cases_dir())
        .expect("a case below tests/elkjs");
    let file = relative.to_string_lossy().replace('\\', "/");
    file.trim_end_matches(".input.json").to_owned()
}

/// Drops elkjs's `$H` fields, which are GWT's object hash ids and no part of the layout.
fn without_hash_ids(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("$H");
            map.values_mut().for_each(without_hash_ids);
        }
        Value::Array(items) => items.iter_mut().for_each(without_hash_ids),
        _ => {}
    }
}

/// The first place two JSON values differ, or `None` when they are equal. Numbers compare as
/// `f64`, bit for bit, so `12` and `12.0` are equal and `12` and `12.000000000000002` are not.
fn first_difference(path: &str, want: &Value, got: &Value) -> Option<String> {
    match (want, got) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            (a.to_bits() != b.to_bits()).then(|| format!("{path}: elkjs {a:?}, elk-layered {b:?}"))
        }
        (Value::Object(a), Value::Object(b)) => {
            for key in a.keys().chain(b.keys()) {
                let next = format!("{path}.{key}");
                match (a.get(key), b.get(key)) {
                    (Some(x), Some(y)) => {
                        if let Some(d) = first_difference(&next, x, y) {
                            return Some(d);
                        }
                    }
                    (Some(_), None) => {
                        return Some(format!("{next}: elkjs has it, elk-layered not"));
                    }
                    (None, Some(_)) => {
                        return Some(format!("{next}: elk-layered has it, elkjs not"));
                    }
                    (None, None) => unreachable!("the key came from one of the two maps"),
                }
            }
            None
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return Some(format!(
                    "{path}: elkjs has {} items, elk-layered {}",
                    a.len(),
                    b.len()
                ));
            }
            a.iter()
                .zip(b)
                .enumerate()
                .find_map(|(i, (x, y))| first_difference(&format!("{path}[{i}]"), x, y))
        }
        _ => (want != got).then(|| format!("{path}: elkjs {want}, elk-layered {got}")),
    }
}

/// Why `lay_out` gave no layout. A broken test file is no refusal of the graph.
enum LayOutError {
    /// The test could not read the input or write the result.
    Test(String),
    /// `layout` refused the graph.
    Layout(elk_layered::Error),
}

fn lay_out(input: &Path) -> Result<Value, LayOutError> {
    let text = std::fs::read_to_string(input).map_err(|e| LayOutError::Test(e.to_string()))?;
    let graph: elk_layered::ElkNode =
        serde_json::from_str(&text).map_err(|e| LayOutError::Test(format!("read: {e}")))?;
    let laid = elk_layered::layout(&graph).map_err(LayOutError::Layout)?;
    serde_json::to_value(&laid).map_err(|e| LayOutError::Test(format!("write: {e}")))
}

fn expected(case: &str) -> Value {
    let path = cases_dir().join(format!("{case}.expected.json"));
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("{case}: no {}", path.display()));
    let mut want: Value = serde_json::from_str(&text).unwrap();
    without_hash_ids(&mut want);
    want
}

/// What `layout` must answer for each exception elkjs throws: the `feature` of the
/// `Error::Unsupported` it returns. A new kind of exception needs a deliberate entry here.
const REFUSALS: &[(&str, &str)] = &[(
    "UnsupportedGraphException",
    // `run_layered` in src/layout.rs: an edge between a node and its own children, which the
    // importer of the child level cannot find.
    "an edge between a node and its own children laid out on their own",
)];

/// The `feature` that `layout` must give for the exception that `refusal` names, or `None` when
/// the table has no entry for it.
fn expected_feature(refusal: &Value) -> Option<&'static str> {
    let text = refusal.as_str()?;
    REFUSALS
        .iter()
        .find(|(exception, _)| text.contains(exception))
        .map(|(_, feature)| *feature)
}

/// How the result of `layout` differs from what elkjs gave, or `None` when it does not.
///
/// elkjs refuses some graphs, such as one with an edge into a part laid out on its own: ELK's
/// `UnsupportedGraphException`. `record.mjs` then writes `{"elkjsError": ...}`, and `layout` must
/// refuse the graph with `Error::Unsupported` and the feature that `REFUSALS` gives for that
/// exception. A read or write failure of the test, or any other error, is not that refusal. The
/// two messages are not compared: they come from two programs.
fn difference(want: &Value, got: Result<Value, LayOutError>) -> Option<String> {
    match (want.get("elkjsError"), got) {
        (
            Some(refusal),
            Err(LayOutError::Layout(elk_layered::Error::Unsupported { feature, .. })),
        ) if expected_feature(refusal) == Some(feature.as_str()) => None,
        (
            Some(refusal),
            Err(LayOutError::Layout(elk_layered::Error::Unsupported { feature, .. })),
        ) => Some(format!(
            "elkjs refuses it ({refusal}), elk-layered refuses it for another reason: {feature}"
        )),
        (Some(refusal), Err(LayOutError::Layout(error))) => Some(format!(
            "elkjs refuses it ({refusal}), elk-layered fails with another error: {error}"
        )),
        (Some(refusal), Err(LayOutError::Test(error))) => Some(format!(
            "elkjs refuses it ({refusal}), but the test failed first: {error}"
        )),
        (Some(refusal), Ok(_)) => Some(format!(
            "elkjs refuses it ({refusal}), elk-layered lays it out"
        )),
        (None, Err(LayOutError::Layout(error))) => Some(format!("layout: {error}")),
        (None, Err(LayOutError::Test(error))) => Some(error),
        (None, Ok(got)) => first_difference("", want, &got),
    }
}

// Mutations, each tried alone. A mutation undoes one fix, and these cases then differ from elkjs.
// 1. In `label_cell_size` (src/intermediate.rs), pass `!horizontal_layout`, so a cell stacks its
//    labels the wrong way: 47 cases, such as random/r000, random/r002, random-group-ends/r002.
// 2. In `update_end_label_node_margin`, read the right and bottom margin from the far corner,
//    `(min_x + width) - margin.left - node_size.width`: 174 cases, such as labels, random/r000,
//    random/r008.
// 3. In `remove_end_label_overlaps`, never drop a cell from the scanline
//    (`if false && rects[first].x + ...`), so every cell overlaps every other and they all stack:
//    7 cases, such as random/r030, random/r057, random-group-ends/r004.
// 4. In `hide_ports` (src/selfloops.rs), split off no hidden ports
//    (`node.ports.split_off(node.ports.len())`), so they stay on the node: 12 cases, such as
//    random/r000, random/r038, random-group-ends/r029.
// 5. In `create_east_port_side_dummy` and `create_west_port_side_dummy`, call
//    `set_dummy_node_properties` instead of `set_inverted_port_dummy_long_edge_ports`: 1 case,
//    random/r091.
// 6. In `parent_port_net_flows` (src/compound.rs), vote by port type alone
//    (`votes_output = !leaves_port`): 14 cases, such as random-group-ends/r004,
//    random-group-ends/r025, random-group-ends/r033.
// 7. In `minimize_two_sided_greedy_switch` (src/p3order/sweep.rs), drop the
//    `graph.random.next_long()`: 6 cases, random/r017, random/r063, random/r144, random/r145,
//    random/r169 and random-group-ends/r082.
// 8. In `minimize_one_sided_greedy_switch` (src/p3order/sweep.rs), drop the
//    `graph.random.next_long()`: 1 case, greedy-switch-one-sided. It has two connected
//    components and sets the one-sided greedy switch, so the draws decide each component's sweep
//    direction.
// 9. In the same function, drop the `graph.random.next_bool()` that comes right after: the same
//    1 case, greedy-switch-one-sided.
// 10. In the `to_graph` refusal of src/layout.rs, return `Error::Internal` instead of
//    `Error::Unsupported`: all 26 cases that elkjs refuses, such as random-group-ends/r034.
// 11. In the `to_graph` refusal of src/layout.rs, change the `feature` text, for example to "an
//    edge into a part of the graph laid out on its own". The same 26 cases then fail on the
//    feature, not on the error kind.
#[test]
fn every_case_lays_out_exactly_as_elkjs_does() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for input in cases() {
        let case = name(&input);
        if NOT_YET.iter().any(|(n, _)| *n == case) {
            continue;
        }
        if let Some(d) = difference(&expected(&case), lay_out(&input)) {
            failures.push(format!("{case}: {d}"));
        }
        checked += 1;
    }
    assert!(checked > 0, "no case was checked");
    assert!(
        failures.is_empty(),
        "{} of {checked} cases differ from elkjs:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// A case on the not-yet list that already passes should leave the list, so the list never hides
/// a case that works.
// Mutation: add "chain" to NOT_YET. The test then asks to take it off again.
#[test]
fn no_case_waits_for_a_feature_that_already_works() {
    for (case, issue) in NOT_YET {
        let input = cases_dir().join(format!("{case}.input.json"));
        assert!(input.exists(), "{case} is on NOT_YET but has no input");
        assert!(
            difference(&expected(case), lay_out(&input)).is_some(),
            "{case} now matches elkjs: take it off NOT_YET ({issue})"
        );
    }
}

/// The differ itself must see a one-bit change, or the suite proves nothing.
// Mutation: compare the numbers with `==` on `f32`. The one-bit change then goes unseen.
#[test]
fn the_comparison_sees_a_one_bit_difference() {
    let want: Value = serde_json::json!({"x": 12.0, "a": [{"y": 1}]});
    let got: Value = serde_json::json!({"x": 12.000000000000002, "a": [{"y": 1}]});
    assert!(first_difference("", &want, &got).is_some());
    assert!(first_difference("", &want, &want).is_none());
}
