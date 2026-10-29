//! Where elkjs behaves differently from ELK's Java on a JVM.
//!
//! elkjs 0.9.3 is ELK's Java compiled to JavaScript by GWT. GWT emulates the Java runtime, but not
//! everywhere the way a JVM does. This crate follows elkjs, so every such place is a function here,
//! and the code that needs it calls it. Each function says what the JVM does, what elkjs does,
//! and where elkjs 0.9.3 shows it.
//!
//! The emulated classes are in GWT's `user/super/com/google/gwt/emul/java`, compiled into
//! `elkjs/lib/elk-worker.js`.

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
    if value - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
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
    let trimmed = text.trim_matches(|c: char| c.is_whitespace());
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

#[cfg(test)]
mod tests {
    use super::*;

    // Mutation: make `float_value` round to `f32`, as a JVM does.
    #[test]
    fn a_float_keeps_every_bit_of_its_double() {
        for value in [0.1, 1.0 / 3.0, 2.0_f64.sqrt() * 1e10, 0.0, -0.0, f64::MAX] {
            assert_eq!(float_value(value).to_bits(), value.to_bits(), "{value}");
        }
        // A JVM would round these.
        assert_ne!(f64::from(0.1_f64 as f32), 0.1);
        assert_eq!(float_value(0.1), 0.1);
        assert!(float_value(f64::NAN).is_nan());
    }

    // Mutation: round with `(value + 0.5).floor()`.
    #[test]
    fn math_round_rounds_like_javascript() {
        assert_eq!(math_round(2.5), 3.0);
        assert_eq!(math_round(-2.5), -2.0);
        assert_eq!(math_round(-2.6), -3.0);
        assert_eq!(math_round(1.4), 1.0);
        assert_eq!(math_round(0.0), 0.0);
        // `(x + 0.5).floor()` gives 1 here: the sum rounds up to 1.0.
        assert_eq!(math_round(0.499_999_999_999_999_94), 0.0);
        // Above 2^52 every double is an integer. The sum with 0.5 rounds to the even neighbour.
        let odd = 4_503_599_627_370_497.0;
        assert_eq!(math_round(odd), odd);
        assert!(math_round(f64::NAN).is_nan());
        assert_eq!(math_round(f64::INFINITY), f64::INFINITY);
    }

    // Mutation: hash `-0.0` by its own bits.
    #[test]
    fn a_double_key_treats_both_zeros_as_one_and_never_finds_nan() {
        assert_eq!(double_key(-0.0), double_key(0.0));
        assert_ne!((-0.0_f64).to_bits(), 0.0_f64.to_bits());
        assert_eq!(double_key(1.5), Some(1.5_f64.to_bits()));
        assert_ne!(double_key(1.5), double_key(-1.5));
        assert_eq!(double_key(f64::NAN), None);
        assert_eq!(double_key(f64::INFINITY), Some(f64::INFINITY.to_bits()));
    }

    // Mutation: compare boxed integers by value only.
    #[test]
    fn boxed_integers_outside_the_cache_always_differ() {
        assert!(!boxed_integers_differ(127, 127));
        assert!(!boxed_integers_differ(-128, -128));
        assert!(!boxed_integers_differ(0, 0));
        assert!(boxed_integers_differ(128, 128));
        assert!(boxed_integers_differ(-129, -129));
        assert!(boxed_integers_differ(3, 4));
        assert!(boxed_integers_differ(500, 4));
    }

    // Mutation: stop sinking on a tie in `priority_queue_poll`, as a JVM does.
    #[test]
    fn a_poll_moves_an_equal_child_up() {
        // Values are (key, name); only the key is compared.
        let by_key = |a: (u8, char), b: (u8, char)| a.0.cmp(&b.0);
        let mut heap = Vec::new();
        for value in [(1, 'a'), (1, 'b'), (1, 'c')] {
            priority_queue_offer(&mut heap, value, by_key);
        }
        assert_eq!(heap, vec![(1, 'a'), (1, 'b'), (1, 'c')]);
        assert_eq!(priority_queue_poll(&mut heap, by_key), Some((1, 'a')));
        // A JVM would keep 'c' on top here and poll it next.
        assert_eq!(heap, vec![(1, 'b'), (1, 'c')]);
        assert_eq!(priority_queue_poll(&mut heap, by_key), Some((1, 'b')));
        assert_eq!(priority_queue_poll(&mut heap, by_key), Some((1, 'c')));
        assert_eq!(priority_queue_poll(&mut heap, by_key), None);
    }

    // Mutation: let `priority_queue_offer` move a value up past an equal parent.
    #[test]
    fn an_offer_orders_values_and_stops_at_an_equal_parent() {
        let by_key = |a: (u8, char), b: (u8, char)| a.0.cmp(&b.0);
        let mut heap = Vec::new();
        for value in [(5, 'a'), (3, 'b'), (3, 'c'), (1, 'd'), (4, 'e')] {
            priority_queue_offer(&mut heap, value, by_key);
        }
        assert_eq!(heap[0], (1, 'd'));
        let mut heap2 = vec![(3, 'x')];
        priority_queue_offer(&mut heap2, (3, 'y'), by_key);
        assert_eq!(heap2, vec![(3, 'x'), (3, 'y')]);
        let mut polled = Vec::new();
        while let Some(value) = priority_queue_poll(&mut heap, by_key) {
            polled.push(value.0);
        }
        assert_eq!(polled, vec![1, 3, 3, 4, 5]);
    }

    // Mutation: sort the keys instead of keeping their first order.
    #[test]
    fn enum_keys_keep_the_order_they_were_first_put_in() {
        #[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
        enum Side {
            North,
            East,
            South,
        }
        let keys = [
            Side::South,
            Side::North,
            Side::South,
            Side::East,
            Side::North,
        ];
        assert_eq!(
            enum_keys_in_insertion_order(keys),
            vec![Side::South, Side::North, Side::East]
        );
        assert_eq!(enum_keys_in_insertion_order(Vec::<Side>::new()), Vec::new());
    }

    // Mutation: skip the syntax check and let Rust parse the string; Rust takes `infinity`.
    #[test]
    fn double_value_of_takes_the_gwt_syntax_only() {
        for (text, want) in [
            ("1", 1.0),
            (" 2.5 ", 2.5),
            ("+.5", 0.5),
            ("-3.", -3.0),
            ("1e3", 1000.0),
            ("1.5E-2", 0.015),
            ("7d", 7.0),
            ("7F", 7.0),
        ] {
            assert_eq!(double_value_of(text), Some(want), "{text}");
        }
        assert!(double_value_of("NaN").is_some_and(f64::is_nan));
        assert_eq!(double_value_of("-Infinity"), Some(f64::NEG_INFINITY));
        for text in [
            "", ".", "e3", "1e", "0x1p3", "1,5", "abc", "1.2.3", "--1", "infinity",
        ] {
            assert_eq!(double_value_of(text), None, "{text}");
        }
    }
}
