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

// Mutation: round with `(value + 0.5).floor()`. A second mutation drops the sign of a zero.
#[test]
fn math_round_rounds_like_javascript() {
    assert_eq!(math_round(2.5), 3.0);
    assert_eq!(math_round(-2.5), -2.0);
    assert_eq!(math_round(-2.6), -3.0);
    assert_eq!(math_round(1.4), 1.0);
    assert_eq!(math_round(0.0), 0.0);
    // A zero keeps the sign of the input, as in JavaScript: -0.4 and -0.5 round to -0.
    for value in [-0.4, -0.5, -0.0] {
        assert_eq!(math_round(value).to_bits(), (-0.0_f64).to_bits(), "{value}");
    }
    assert_eq!(math_round(0.4).to_bits(), 0.0_f64.to_bits());
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

// Mutation: iterate a `LinkedSet` in sorted order, or let `insert` add a value twice.
#[test]
fn a_linked_set_keeps_the_order_values_were_first_added() {
    let mut set = LinkedSet::default();
    assert!(set.insert(5));
    assert!(set.insert(2));
    assert!(!set.insert(5));
    set.extend([9, 2, 1]);
    assert!(set.contains(&9));
    assert!(!set.contains(&3));
    assert_eq!(set.to_vec(), vec![5, 2, 9, 1]);
    assert_eq!(set.into_iter().collect::<Vec<_>>(), vec![5, 2, 9, 1]);
    assert_eq!(LinkedSet::<u8>::default().into_iter().count(), 0);
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

// Mutation: skip the syntax check and let Rust parse the string; Rust takes `infinity`. A
// second mutation trims with `char::is_whitespace`, which refuses U+FEFF and takes U+0085.
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
    // JavaScript's whitespace, not Rust's.
    assert_eq!(double_value_of("\u{feff}1\u{3000}"), Some(1.0));
    assert_eq!(double_value_of("\u{2028}1"), Some(1.0));
    for text in [
        "", ".", "e3", "1e", "0x1p3", "1,5", "abc", "1.2.3", "--1", "infinity", "1\u{85}",
    ] {
        assert_eq!(double_value_of(text), None, "{text}");
    }
}
