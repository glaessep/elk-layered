use super::*;

#[test]
fn next_int_matches_java_random_for_seed_one() {
    let mut random = JavaRandom::new(1);

    assert_eq!(random.next_int(3), Some(0));
    assert_eq!(random.next_int(3), Some(1));
    assert_eq!(random.next_int(3), Some(1));
    assert_eq!(random.next_int(10), Some(3));
}

#[test]
fn scalar_methods_match_java_random_for_seed_one() {
    let mut random = JavaRandom::new(1);

    assert!(random.next_bool());
    assert_eq!(random.next_float().to_bits(), 0x3dcdc4e0);

    let mut random = JavaRandom::new(1);
    assert_eq!(random.next_long(), -4964420948893066024);
}

#[test]
fn explicit_java_seed_zero_is_deterministic() {
    let mut random = JavaRandom::new(0);

    assert_eq!(random.next_int(3), Some(0));
    assert_eq!(random.next_int(3), Some(1));
    assert_eq!(random.next_int(10), Some(9));

    let mut random = JavaRandom::new(0);
    assert_eq!(random.next_long(), -4962768465676381896);
}
