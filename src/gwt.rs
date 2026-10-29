//! Where elkjs behaves differently from ELK's Java on a JVM.
//!
//! elkjs 0.9.3 is ELK's Java compiled to JavaScript by GWT. GWT emulates the Java runtime, but not
//! everywhere the way a JVM does. This crate follows elkjs, so every such place is a function here,
//! and the code that needs it calls it. Each function says what the JVM does, what elkjs does,
//! and where elkjs 0.9.3 shows it.
//!
//! The emulated classes are in GWT's `user/super/com/google/gwt/emul/java`, compiled into
//! `elkjs/lib/elk-worker.js`.
//!
//! One difference has no function here, because it lives in the data structure that meets it:
//! a JavaScript array has no bounds check. Java throws `ArrayIndexOutOfBoundsException` for an
//! index past the end; elkjs reads `undefined`, which turns a sum into `NaN`, or grows the array.
//! `BinaryIndexedTree` in `phase/ordering/counting` follows elkjs there.

use std::cmp::Ordering;

/// `Double.floatValue()` and a `(float)` cast, as elkjs runs them: they change nothing.
///
/// On a JVM a `float` has 24 bits of mantissa, so `(float) 0.1` is `0.10000000149011612`. GWT keeps
/// every `float` as a JavaScript number, which is a double. Its `Double.floatValue()` returns the
/// value itself (`$floatValue` in `elk-worker.js`), and it drops `(float)` casts: elkjs computes
/// `(float) Math.sqrt(totalArea)` in `SimpleRowGraphPlacer` as plain `Math.sqrt(totalArea)`.
pub(crate) fn float_value(value: f64) -> f64 {
    value
}

/// `Math.round(double)`, as JavaScript's `Math.round` computes it: the nearest integer, and a tie
/// goes up, toward positive infinity.
///
/// GWT compiles `Math.round` to `Math.round` of JavaScript. A JVM rounds the same numbers, except
/// that it returns a `long`. The difference this crate cares about is the way it rounds: `x + 0.5`
/// and then `floor` is not the same. It rounds `0.49999999999999994` to 1, because the sum rounds
/// up to 1.0, and an odd integer above 2^52 to the next even one. JavaScript gives 0 and the
/// integer itself.
pub(crate) fn math_round(value: f64) -> f64 {
    let floor = value.floor();
    let rounded = if value - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    };
    // JavaScript rounds -0.5 up to -0 and keeps the sign of a zero.
    if rounded == 0.0 {
        0.0_f64.copysign(value)
    } else {
        rounded
    }
}

/// What a GWT `HashSet` or `HashMap` finds a `double` key by, or `None` when it never finds it.
///
/// A JVM hashes a `Double` by its bits, so `0.0` and `-0.0` are two keys. GWT's `Double.hashCode`
/// is `(int) value`, so they fall in one bucket, and an `equals` that compares with `==` finds them
/// equal. elkjs's `KVector.equals` does that, so a point with `-0.0` is the point with `0.0`. A
/// `NaN` is never equal to itself, so a key with a `NaN` is never found.
pub(crate) fn double_key(value: f64) -> Option<u64> {
    if value.is_nan() {
        return None;
    }
    // Adding 0.0 turns -0.0 into 0.0 and changes no other number.
    Some((value + 0.0).to_bits())
}

/// Whether Java's `!=` sees two boxed `Integer` values as different objects.
///
/// `Integer.valueOf` hands out one cached object for each value from -128 to 127. Any other value
/// is a new object each time, so `!=` is true even when the values are equal. GWT keeps the same
/// cache (`Integer$BoxedValues` in `elk-worker.js`), so elkjs and a JVM agree here. The rule lives
/// here because it looks like a bug, and ELK's `CoffmanGrahamLayerer` depends on it.
pub(crate) fn boxed_integers_differ(first: i64, second: i64) -> bool {
    first != second || !(-128..=127).contains(&first)
}

/// `PriorityQueue.offer` as GWT implements it: the value goes to the end and moves up while it is
/// smaller than its parent.
pub(crate) fn priority_queue_offer<T: Copy>(
    heap: &mut Vec<T>,
    value: T,
    mut compare: impl FnMut(T, T) -> Ordering,
) {
    let mut cursor = heap.len();
    heap.push(value);
    while cursor > 0 {
        let parent = (cursor - 1) / 2;
        if compare(heap[parent], value) != Ordering::Greater {
            break;
        }
        heap[cursor] = heap[parent];
        cursor = parent;
    }
    heap[cursor] = value;
}

/// `PriorityQueue.poll` as GWT implements it, or `None` for an empty queue.
///
/// Both GWT and a JVM move the last value to the top and let it sink. They differ on a tie: a JVM
/// stops when the value is not greater than the smaller child, GWT's `mergeHeaps` stops only when
/// it is smaller. So with equal values, elkjs moves the child up and polls another value next.
pub(crate) fn priority_queue_poll<T: Copy>(
    heap: &mut Vec<T>,
    mut compare: impl FnMut(T, T) -> Ordering,
) -> Option<T> {
    let result = *heap.first()?;
    let value = heap.pop()?;
    if !heap.is_empty() {
        let mut cursor = 0;
        while cursor * 2 + 1 < heap.len() {
            let mut child = cursor * 2 + 1;
            if child + 1 < heap.len() && compare(heap[child + 1], heap[child]) == Ordering::Less {
                child += 1;
            }
            if compare(value, heap[child]) == Ordering::Less {
                break;
            }
            heap[cursor] = heap[child];
            cursor = child;
        }
        heap[cursor] = value;
    }
    Some(result)
}

/// `Arrays.sort` and `Collections.sort` with a comparator, as GWT implements them: insertion sort
/// below seven elements, then a merge sort that swaps two buffers.
///
/// A JVM uses TimSort. With a comparator that is a total order, both give the one stable order.
/// They differ when the comparator is not one: ELK's model-order comparators answer from a cache
/// that can contradict itself, and `PortListSorter.CMP_FIXED_ORDER_AND_FIXED_POS` compares port
/// indices only when both ports have one. GWT then makes the comparisons in its own schedule, and
/// this function makes the same ones (`insertionSort`, `mergeSort_0` and `merge_1` in
/// `elk-worker.js`). It never panics, where Rust's `sort_by` may for such a comparator.
pub(crate) fn sort<T: Copy>(order: &mut [T], mut compare: impl FnMut(T, T) -> Ordering) {
    fn merge_sort<T: Copy>(
        source: &mut [T],
        destination: &mut [T],
        compare: &mut impl FnMut(T, T) -> Ordering,
    ) {
        if destination.len() < 7 {
            for index in 1..destination.len() {
                let mut current = index;
                while current > 0
                    && compare(destination[current - 1], destination[current]) == Ordering::Greater
                {
                    destination.swap(current - 1, current);
                    current -= 1;
                }
            }
            return;
        }

        let middle = destination.len() / 2;
        let (source_left, source_right) = source.split_at_mut(middle);
        let (destination_left, destination_right) = destination.split_at_mut(middle);
        merge_sort(destination_left, source_left, compare);
        merge_sort(destination_right, source_right, compare);
        if compare(source[middle - 1], source[middle]) != Ordering::Greater {
            destination.copy_from_slice(source);
            return;
        }

        let mut left = 0;
        let mut right = middle;
        for item in destination {
            if right >= source.len()
                || (left < middle && compare(source[left], source[right]) != Ordering::Greater)
            {
                *item = source[left];
                left += 1;
            } else {
                *item = source[right];
                right += 1;
            }
        }
    }

    let mut source = order.to_vec();
    merge_sort(&mut source, order, &mut compare);
}

/// A GWT `HashSet` of objects that hash by identity, such as ELK's nodes and ports: it iterates
/// in the order the elements were first added.
///
/// GWT gives each object a new identity hash, and keeps the entries of a set in a JavaScript
/// `Map` keyed by that hash, which iterates in insertion order. A JVM's `HashSet` iterates by
/// bucket, and Rust's `HashSet` in an order that changes from run to run.
#[derive(Debug, Clone)]
pub(crate) struct LinkedSet<T> {
    order: Vec<T>,
    members: std::collections::HashSet<T>,
}

impl<T> Default for LinkedSet<T> {
    fn default() -> Self {
        Self {
            order: Vec::new(),
            members: std::collections::HashSet::new(),
        }
    }
}

impl<T: Copy + Eq + std::hash::Hash> LinkedSet<T> {
    /// Adds `value` at the end, unless the set holds it. Returns whether it was added.
    pub(crate) fn insert(&mut self, value: T) -> bool {
        let added = self.members.insert(value);
        if added {
            self.order.push(value);
        }
        added
    }

    /// The elements in the order they were first added, without the lookup table.
    pub(crate) fn to_vec(&self) -> Vec<T> {
        self.order.clone()
    }

    /// Whether the set holds `value`.
    pub(crate) fn contains(&self, value: &T) -> bool {
        self.members.contains(value)
    }
}

impl<T: Copy + Eq + std::hash::Hash> Extend<T> for LinkedSet<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, values: I) {
        for value in values {
            self.insert(value);
        }
    }
}

impl<T> IntoIterator for LinkedSet<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.order.into_iter()
    }
}

/// The keys of a GWT `HashMap` whose keys are enum constants, in the order elkjs iterates them:
/// the order they were first put in.
///
/// An enum constant hashes by its identity. A JVM's `HashMap` iterates its buckets, so the order
/// depends on those hashes and differs from run to run. GWT keeps the entries of each hash in a
/// JavaScript `Map`, which iterates in insertion order, and enum constants never share a hash. So
/// elkjs iterates such keys in the order they were first added.
pub(crate) fn enum_keys_in_insertion_order<T: Copy + PartialEq>(
    keys: impl IntoIterator<Item = T>,
) -> Vec<T> {
    let mut distinct = Vec::new();
    for key in keys {
        if !distinct.contains(&key) {
            distinct.push(key);
        }
    }
    distinct
}

/// `Double.valueOf(String)` as GWT parses it, or `None` for a string GWT refuses.
///
/// GWT checks the string against
/// `^\s*[+-]?(NaN|Infinity|((\d+\.?\d*)|(\.\d+))([eE][+-]?\d+)?[dDfF]?)\s*$` and then reads it with
/// JavaScript's `parseFloat`. A JVM also accepts hexadecimal numbers such as `0x1p3`, which GWT
/// refuses.
pub(crate) fn double_value_of(text: &str) -> Option<f64> {
    let trimmed = text.trim_matches(is_javascript_whitespace);
    let (negative, body) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let special = match body {
        "NaN" => Some(f64::NAN),
        "Infinity" => Some(f64::INFINITY),
        _ => None,
    };
    if let Some(value) = special {
        return Some(if negative { -value } else { value });
    }
    let body = body.strip_suffix(['d', 'D', 'f', 'F']).unwrap_or(body);
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(at) => (&body[..at], Some(&body[at + 1..])),
        None => (body, None),
    };
    let (whole, fraction) = match mantissa.find('.') {
        Some(at) => (&mantissa[..at], Some(&mantissa[at + 1..])),
        None => (mantissa, None),
    };
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok = digits(whole)
        && fraction.is_none_or(digits)
        && (!whole.is_empty() || fraction.is_some_and(|f| !f.is_empty()));
    let exponent_ok = exponent.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && digits(e)
    });
    if !mantissa_ok || !exponent_ok {
        return None;
    }
    let number = trimmed
        .strip_suffix(['d', 'D', 'f', 'F'])
        .unwrap_or(trimmed);
    number.parse::<f64>().ok()
}

/// What JavaScript's `\s` matches, which GWT's regex for `Double.valueOf` uses. It is not Rust's
/// `char::is_whitespace`: it takes U+FEFF and leaves out U+0085.
fn is_javascript_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

#[cfg(test)]
mod tests;
