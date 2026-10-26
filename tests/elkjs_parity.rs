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

fn cases() -> Vec<PathBuf> {
    let mut inputs: Vec<PathBuf> = std::fs::read_dir(cases_dir())
        .expect("tests/elkjs exists")
        .map(|entry| entry.expect("a readable entry").path())
        .filter(|path| path.to_string_lossy().ends_with(".input.json"))
        .collect();
    inputs.sort();
    inputs
}

fn name(input: &Path) -> String {
    let file = input.file_name().unwrap().to_string_lossy();
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
                    (Some(_), None) => return Some(format!("{next}: elkjs has it, elk-layered not")),
                    (None, Some(_)) => return Some(format!("{next}: elk-layered has it, elkjs not")),
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

fn lay_out(input: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(input).map_err(|e| e.to_string())?;
    let graph: elk_layered::ElkNode =
        serde_json::from_str(&text).map_err(|e| format!("read: {e}"))?;
    let laid = elk_layered::layout(&graph).map_err(|e| format!("layout: {e}"))?;
    serde_json::to_value(&laid).map_err(|e| format!("write: {e}"))
}

fn expected(case: &str) -> Value {
    let path = cases_dir().join(format!("{case}.expected.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{case}: no {}", path.display()));
    let mut want: Value = serde_json::from_str(&text).unwrap();
    without_hash_ids(&mut want);
    want
}

#[test]
fn every_case_lays_out_exactly_as_elkjs_does() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for input in cases() {
        let case = name(&input);
        if NOT_YET.iter().any(|(n, _)| *n == case) {
            continue;
        }
        match lay_out(&input) {
            Ok(got) => {
                if let Some(d) = first_difference("", &expected(&case), &got) {
                    failures.push(format!("{case}: {d}"));
                }
            }
            Err(e) => failures.push(format!("{case}: {e}")),
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
#[test]
fn no_case_waits_for_a_feature_that_already_works() {
    for (case, issue) in NOT_YET {
        let input = cases_dir().join(format!("{case}.input.json"));
        assert!(input.exists(), "{case} is on NOT_YET but has no input");
        if let Ok(got) = lay_out(&input) {
            assert!(
                first_difference("", &expected(case), &got).is_some(),
                "{case} now matches elkjs: take it off NOT_YET ({issue})"
            );
        }
    }
}

/// The differ itself must see a one-bit change, or the suite proves nothing.
#[test]
fn the_comparison_sees_a_one_bit_difference() {
    let want: Value = serde_json::json!({"x": 12.0, "a": [{"y": 1}]});
    let got: Value = serde_json::json!({"x": 12.000000000000002, "a": [{"y": 1}]});
    assert!(first_difference("", &want, &got).is_some());
    assert!(first_difference("", &want, &want).is_none());
}
