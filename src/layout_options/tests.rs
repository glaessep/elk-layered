use super::*;

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

fn node(pairs: &[(&str, &str)]) -> Result<NodeSettings, OptionProblem> {
    node_settings(&map(pairs)).map_err(|error| error.problem)
}

// Mutation: drop the dot check in `is_id_suffix`.
#[test]
fn an_id_suffix_is_the_id_or_the_part_after_a_dot() {
    let id = "org.eclipse.elk.layered";
    assert!(is_id_suffix(id, id));
    assert!(is_id_suffix(id, "layered"));
    assert!(is_id_suffix(id, "elk.layered"));
    assert!(!is_id_suffix(id, "ayered"));
    assert!(!is_id_suffix(id, "xorg.eclipse.elk.layered"));
    assert!(!is_id_suffix(id, ""));
}

// Mutation: drop the dot check in `resolve`.
#[test]
fn a_key_resolves_by_its_id_or_a_suffix_after_a_dot() {
    let direction = Some(("org.eclipse.elk.direction", PARENTS));
    assert_eq!(resolve("org.eclipse.elk.direction"), direction);
    assert_eq!(resolve("elk.direction"), direction);
    assert_eq!(resolve("eclipse.elk.direction"), direction);
    assert_eq!(
        resolve("spacing.nodeNode").map(|r| r.0),
        Some("org.eclipse.elk.spacing.nodeNode")
    );
    assert_eq!(
        resolve("nodeNode").map(|r| r.0),
        Some("org.eclipse.elk.spacing.nodeNode")
    );
    // `direction` also ends `layered.priority.direction`, so elkjs cannot resolve it.
    assert_eq!(resolve("direction"), None);
    assert_eq!(resolve("irection"), None);
    assert_eq!(resolve("lk.direction"), None);
    assert_eq!(resolve("elk.Direction"), None);
    assert_eq!(resolve(""), None);
}

// Mutation: compare with `==` instead of `eq_ignore_ascii_case` in `parse_bool`.
#[test]
fn booleans_ignore_case_and_nothing_else() {
    assert_eq!(parse_bool("TRUE"), Ok(true));
    assert_eq!(parse_bool("False"), Ok(false));
    for value in ["yes", "1", " true", ""] {
        assert_eq!(
            parse_bool(value),
            Err(OptionProblem::InvalidValue),
            "{value}"
        );
    }
}

// Mutation: let `parse_int` accept any `i64`.
#[test]
fn integers_follow_integer_value_of() {
    assert_eq!(parse_int("+3"), Ok(3));
    assert_eq!(parse_int("-2147483648"), Ok(i32::MIN));
    for value in ["2147483648", "3.0", "", "+", " 3", "0x1"] {
        assert_eq!(
            parse_int(value),
            Err(OptionProblem::InvalidValue),
            "{value}"
        );
    }
}

// Mutation: drop the `[dDfF]` suffix handling in `parse_double`.
#[test]
fn doubles_follow_the_gwt_syntax() {
    for (value, want) in [
        ("1e1", 10.0),
        (".5", 0.5),
        ("5.", 5.0),
        ("2d", 2.0),
        ("-1.5F", -1.5),
        (" 3 ", 3.0),
        ("+4E-1", 0.4),
    ] {
        assert_eq!(parse_double(value), Ok(want), "{value}");
    }
    for value in [
        "NaN", "Infinity", "abc", "1e", ".", "", "1,5", "0x10", "1e999",
    ] {
        assert_eq!(
            parse_double(value),
            Err(OptionProblem::InvalidValue),
            "{value}"
        );
    }
}

// Mutation: clamp the ordinal to the last value in `parse_enum`.
#[test]
fn an_enum_is_its_exact_name_or_its_ordinal() {
    let values = [("A", 1), ("B_C", 2)];
    assert_eq!(parse_enum("B_C", &values), Ok(2));
    assert_eq!(parse_enum("0", &values), Ok(1));
    for value in ["b_c", "2", "-1", "", " A"] {
        assert_eq!(
            parse_enum(value, &values),
            Err(OptionProblem::InvalidValue),
            "{value}"
        );
    }
}

// Mutation: split EnumSet values at brackets and commas, not at blanks.
#[test]
fn an_enum_set_splits_at_brackets_commas_and_blanks() {
    let parse = |value| {
        parse_flags(
            value,
            &SizeConstraints::NAMES,
            SizeConstraints::FIXED,
            |a, b| a | b,
        )
    };
    let both = SizeConstraints::NODE_LABELS | SizeConstraints::MINIMUM_SIZE;
    assert_eq!(parse("[NODE_LABELS, MINIMUM_SIZE]"), Ok(both));
    assert_eq!(parse("NODE_LABELS MINIMUM_SIZE"), Ok(both));
    assert_eq!(parse("[ NODE_LABELS,,MINIMUM_SIZE ]"), Ok(both));
    assert_eq!(parse(""), Ok(SizeConstraints::FIXED));
    assert_eq!(parse("[]"), Ok(SizeConstraints::FIXED));
    assert_eq!(
        parse("[NODE_LABELS, OTHER]"),
        Err(OptionProblem::InvalidValue)
    );
}

// Mutation: start `parse_spacing` from ELK's default padding of 12.
#[test]
fn a_padding_names_its_sides_and_leaves_the_rest_zero() {
    let padding = |top, right, bottom, left| ElkPadding {
        top,
        right,
        bottom,
        left,
    };
    assert_eq!(
        parse_spacing("[top=1,left=2,bottom=3,right=4]"),
        Ok(padding(1.0, 4.0, 3.0, 2.0))
    );
    assert_eq!(
        parse_spacing("(top=5; left = 6)"),
        Ok(padding(5.0, 0.0, 0.0, 6.0))
    );
    assert_eq!(parse_spacing("[top=1,]"), Ok(padding(1.0, 0.0, 0.0, 0.0)));
    assert_eq!(parse_spacing("[other=1]"), Ok(padding(0.0, 0.0, 0.0, 0.0)));
    assert_eq!(parse_spacing("[]"), Ok(padding(0.0, 0.0, 0.0, 0.0)));
    for value in ["[top]", "[top=x]", "[,top=1]", "[top=1=2=3]"] {
        assert_eq!(
            parse_spacing(value),
            Err(OptionProblem::InvalidValue),
            "{value}"
        );
    }
}

// Mutation: let `parse_vector` accept any number of numbers.
#[test]
fn a_vector_has_exactly_two_numbers() {
    assert_eq!(parse_vector("(1, 2)"), Ok((1.0, 2.0)));
    assert_eq!(parse_vector("3;4"), Ok((3.0, 4.0)));
    assert_eq!(parse_vector("{5,6}"), Ok((5.0, 6.0)));
    for value in ["(1)", "()", "(1,2,3)", "(a,b)"] {
        assert_eq!(
            parse_vector(value),
            Err(OptionProblem::InvalidValue),
            "{value}"
        );
    }
}

// Mutation: let `parse_node_label_placement` skip the validity check.
#[test]
fn a_node_label_placement_must_be_valid() {
    let settings = node(&[("elk.nodeLabels.placement", "[H_LEFT, V_TOP, INSIDE]")]).unwrap();
    assert_eq!(
        settings.node_label_placement.location(),
        crate::options::NodeLabelLocation::InsideTopLeft
    );
    assert_eq!(
        node(&[("elk.nodeLabels.placement", "[H_LEFT, H_RIGHT]")]).err(),
        Some(OptionProblem::InvalidValue)
    );
    assert_eq!(
        node(&[("elk.nodeLabels.placement", "[INSIDE, OUTSIDE]")]).err(),
        Some(OptionProblem::InvalidValue)
    );
}

// Mutation: drop `settings.options.node_size = settings.node_size` in `node_settings`.
#[test]
fn node_size_options_reach_the_node_and_its_graph() {
    let settings = node(&[
        ("elk.nodeSize.constraints", "[NODE_LABELS]"),
        ("elk.nodeSize.options", "[ASYMMETRICAL]"),
        ("elk.nodeSize.minimum", "(3, 4)"),
    ])
    .unwrap();
    assert_eq!(settings.node_size.constraints, SizeConstraints::NODE_LABELS);
    assert!(
        settings
            .node_size
            .options
            .contains(SizeOptions::ASYMMETRICAL)
    );
    assert!(
        !settings
            .node_size
            .options
            .contains(SizeOptions::DEFAULT_MINIMUM_SIZE)
    );
    assert_eq!(
        settings.node_size.minimum,
        LSize {
            width: 3.0,
            height: 4.0
        }
    );
    assert_eq!(settings.options.node_size, settings.node_size);
}

// Mutation: let `H_RIGHT` win over `H_CENTER` for the content alignment.
#[test]
fn content_alignment_prefers_the_center() {
    let settings = node(&[("elk.contentAlignment", "[H_RIGHT, H_CENTER, V_BOTTOM]")]).unwrap();
    assert_eq!(
        settings.options.horizontal_content_alignment,
        ContentAlignment::Center
    );
    assert_eq!(
        settings.options.vertical_content_alignment,
        ContentAlignment::End
    );
    let settings = node(&[("elk.contentAlignment", "[]")]).unwrap();
    assert_eq!(
        settings.options.horizontal_content_alignment,
        ContentAlignment::Start
    );
}

// Mutation: drop `options.explicit_spacings.insert(flag)` in `spacing`.
#[test]
fn a_spacing_is_marked_explicit_and_the_base_value_is_kept() {
    let settings = node(&[
        ("elk.spacing.nodeNode", "7"),
        ("elk.layered.spacing.baseValue", "40"),
    ])
    .unwrap();
    assert_eq!(settings.options.spacing.node_node, 7.0);
    assert!(
        settings
            .options
            .explicit_spacings
            .contains(ExplicitSpacings::NODE_NODE)
    );
    assert!(
        !settings
            .options
            .explicit_spacings
            .contains(ExplicitSpacings::EDGE_EDGE)
    );
    assert_eq!(settings.options.spacing_base_value, Some(40.0));
    let applied = settings.options.with_spacing_base_value_applied();
    assert_eq!(applied.spacing.node_node, 7.0);
    assert_eq!(applied.spacing.edge_edge, 20.0);
}

// Mutation: ignore `allowNonFlowPortsToSwitchSides` on a port, or read "false" as true.
#[test]
fn a_port_reads_allow_non_flow_ports_to_switch_sides() {
    let key = "elk.layered.allowNonFlowPortsToSwitchSides";
    assert!(
        !port_settings(&map(&[]))
            .unwrap()
            .allow_non_flow_ports_to_switch_sides
    );
    assert!(
        port_settings(&map(&[(key, "true")]))
            .unwrap()
            .allow_non_flow_ports_to_switch_sides
    );
    assert!(
        !port_settings(&map(&[(key, "false")]))
            .unwrap()
            .allow_non_flow_ports_to_switch_sides
    );
    // A value that is no boolean is an error, and a node cannot set the option.
    assert_eq!(
        port_settings(&map(&[(key, "maybe")]))
            .map_err(|e| e.problem)
            .err(),
        Some(OptionProblem::InvalidValue)
    );
    assert_eq!(
        node(&[(key, "true")]).err(),
        Some(OptionProblem::Unsupported(WRONG_TARGET))
    );
}

// Mutation: drop the target check in `resolve_all`.
#[test]
fn an_option_must_target_its_kind_of_element() {
    let wrong = Some(OptionProblem::Unsupported(WRONG_TARGET));
    assert_eq!(node(&[("elk.edgeLabels.placement", "HEAD")]).err(), wrong);
    assert_eq!(
        edge_settings(&map(&[("elk.direction", "DOWN")]))
            .map_err(|e| e.problem)
            .err(),
        wrong
    );
    assert_eq!(
        label_settings(&map(&[("elk.padding", "[]")]))
            .map_err(|e| e.problem)
            .err(),
        wrong
    );
}

// Mutation: let `refuse_true` accept true.
#[test]
fn options_with_unported_values_are_refused_only_for_those_values() {
    assert!(node(&[("elk.layered.mergeEdges", "false")]).is_ok());
    assert!(matches!(
        node(&[("elk.layered.mergeEdges", "true")]),
        Err(OptionProblem::Unsupported(_))
    ));
    assert!(node(&[("elk.layered.nodePlacement.strategy", "SIMPLE")]).is_ok());
    for (key, value) in [
        ("elk.layered.nodePlacement.strategy", "LINEAR_SEGMENTS"),
        ("elk.layered.nodePlacement.strategy", "INTERACTIVE"),
        ("elk.layered.layering.strategy", "INTERACTIVE"),
        ("elk.layered.crossingMinimization.strategy", "INTERACTIVE"),
        (
            "elk.layered.nodePlacement.networkSimplex.nodeFlexibility",
            "NODE_SIZE",
        ),
        ("elk.port.side", "WEST"),
    ] {
        assert!(
            matches!(node(&[(key, value)]), Err(OptionProblem::Unsupported(_))),
            "{key} = {value}"
        );
    }
}

// Mutation: drop the duplicate check in `resolve_all`.
#[test]
fn two_keys_for_one_option_are_refused() {
    assert_eq!(
        node(&[("elk.direction", "UP"), ("org.eclipse.elk.direction", "UP")]).err(),
        Some(OptionProblem::Duplicate {
            other: "elk.direction".into()
        })
    );
}

// Mutation: drop the `HEAD` entry of the edge label placements.
#[test]
fn edge_and_label_options_are_read() {
    let edge = edge_settings(&map(&[
        ("elk.layered.priority.direction", "3"),
        ("elk.layered.priority.shortness", "-1"),
        ("org.eclipse.elk.layered.priority.straightness", "+2"),
        ("elk.insideSelfLoops.yo", "false"),
    ]))
    .unwrap();
    assert_eq!(
        edge,
        EdgeSettings {
            priority_direction: 3,
            priority_shortness: -1,
            priority_straightness: 2,
            inside_self_loops_yo: false,
        }
    );
    let label = label_settings(&map(&[
        ("elk.edgeLabels.placement", "HEAD"),
        ("elk.edgeLabels.inline", "true"),
        ("elk.nodeLabels.placement", "[H_CENTER, V_CENTER, INSIDE]"),
    ]))
    .unwrap();
    assert_eq!(label.placement, EdgeLabelPlacement::Head);
    assert!(label.inline);
    assert!(label.node_label_placement.is_some());
    assert_eq!(label_settings(&map(&[])).unwrap(), LabelSettings::default());
}

// Mutation: read `elk.edgeLabels.inline` as true whatever the value is.
#[test]
fn a_label_is_not_inline_when_the_option_says_false() {
    let label = label_settings(&map(&[("elk.edgeLabels.inline", "false")])).unwrap();
    assert!(!label.inline);
}

// Mutation: read `elk.edgeLabels.inline` with `parse_bool(value).unwrap_or(false)`, so a bad
// value counts as false.
#[test]
fn a_bad_inline_value_is_an_invalid_value_error() {
    let error = label_settings(&map(&[("elk.edgeLabels.inline", "yes")])).unwrap_err();
    assert_eq!(
        error,
        OptionError {
            key: "elk.edgeLabels.inline".to_string(),
            value: "yes".to_string(),
            problem: OptionProblem::InvalidValue,
        }
    );
}

// Mutation: ignore `elk.algorithm` in `apply_node_option`.
#[test]
fn every_supported_node_option_is_read() {
    let settings = node(&[
        ("elk.algorithm", "layered"),
        ("elk.hierarchyHandling", "INCLUDE_CHILDREN"),
        ("elk.layered.layering.layerConstraint", "FIRST"),
        ("elk.portConstraints", "FIXED_ORDER"),
        ("elk.portAlignment.default", "BEGIN"),
        ("elk.layered.edgeRouting.selfLoopDistribution", "EQUALLY"),
        ("elk.layered.edgeRouting.selfLoopOrdering", "SEQUENCED"),
        (
            "elk.layered.nodePlacement.networkSimplex.nodeFlexibility",
            "PORT_POSITION",
        ),
        ("elk.randomSeed", "5"),
        ("elk.aspectRatio", "2"),
        ("elk.layered.thoroughness", "3"),
    ])
    .unwrap();
    assert_eq!(settings.algorithm.as_deref(), Some("layered"));
    assert_eq!(
        settings.hierarchy_handling,
        Some(HierarchyHandling::IncludeChildren)
    );
    assert_eq!(settings.layer_constraint, Some(LayerConstraint::First));
    assert_eq!(settings.port_constraints, Some(PortConstraints::FixedOrder));
    assert_eq!(settings.port_alignment, Some(PortAlignment::Begin));
    assert_eq!(
        settings.self_loop_distribution,
        SelfLoopDistributionStrategy::Equally
    );
    assert_eq!(
        settings.self_loop_ordering,
        SelfLoopOrderingStrategy::Sequenced
    );
    assert_eq!(settings.node_flexibility, NodeFlexibility::PortPosition);
    assert_eq!(settings.options.random_seed, 5);
    assert_eq!(settings.options.aspect_ratio, 2.0);
    assert_eq!(settings.options.thoroughness, 3);
    assert!(matches!(
        node(&[("elk.layered.thoroughness", "-1")]),
        Err(OptionProblem::InvalidValue)
    ));
}

// Mutation: leave `insideSelfLoops.activate` out of the node's options, so `true` stays false.
#[test]
fn a_node_reads_inside_self_loops_activate() {
    let off = node(&[]).unwrap();
    assert!(!off.options.inside_self_loops_activate);
    let on = node(&[("elk.insideSelfLoops.activate", "true")]).unwrap();
    assert!(on.options.inside_self_loops_activate);
    let off = node(&[("org.eclipse.elk.insideSelfLoops.activate", "FALSE")]).unwrap();
    assert!(!off.options.inside_self_loops_activate);
}

// Mutation: ignore the parse error of `insideSelfLoops.activate`.
#[test]
fn inside_self_loops_activate_refuses_a_value_that_is_no_boolean() {
    assert!(matches!(
        node(&[("elk.insideSelfLoops.activate", "maybe")]),
        Err(OptionProblem::InvalidValue)
    ));
}

// Mutation: read `insideSelfLoops.yo` into no field of the edge's settings, so `true` stays
// false.
#[test]
fn an_edge_reads_inside_self_loops_yo() {
    assert!(!edge_settings(&map(&[])).unwrap().inside_self_loops_yo);
    let on = edge_settings(&map(&[("elk.insideSelfLoops.yo", "true")])).unwrap();
    assert!(on.inside_self_loops_yo);
}

// Mutation: ignore the parse error of `insideSelfLoops.yo`.
#[test]
fn inside_self_loops_yo_refuses_a_value_that_is_no_boolean() {
    let error = edge_settings(&map(&[("elk.insideSelfLoops.yo", "maybe")])).unwrap_err();
    assert_eq!(error.problem, OptionProblem::InvalidValue);
}
