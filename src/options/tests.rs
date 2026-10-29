use super::*;

#[test]
fn layered_default_padding_is_twelve() {
    let options = LayeredOptions::default();

    assert_eq!(options.padding, ElkPadding::uniform(12.0));
}

#[test]
fn layered_base_spacing_scales_every_spacing_from_the_node_spacing() {
    let options = LayeredOptions {
        spacing: SpacingOptions::layered_base_value(40.0),
        ..LayeredOptions::default()
    };

    assert_eq!(options.spacing.node_node, 40.0);
    assert_eq!(options.spacing.node_self_loop, 20.0);
    assert_eq!(options.spacing.edge_edge, 20.0);
    assert_eq!(options.spacing.edge_node, 20.0);
    assert_eq!(options.spacing.edge_label, 4.0);
    assert_eq!(options.spacing.label_label, 0.0);
    assert_eq!(options.spacing.label_node, 10.0);
    assert_eq!(options.spacing.label_port_horizontal, 2.0);
    assert_eq!(options.spacing.label_port_vertical, 2.0);
    assert_eq!(options.spacing.port_port, 20.0);
    assert_eq!(options.spacing.edge_edge_between_layers, 20.0);
    assert_eq!(options.spacing.edge_node_between_layers, 20.0);
    assert_eq!(options.spacing.node_node_between_layers, 40.0);
}

#[test]
fn edge_label_side_selection_transposes_symmetrically() {
    use EdgeLabelSideSelection::*;

    assert_eq!(AlwaysUp.transpose(), AlwaysDown);
    assert_eq!(AlwaysDown.transpose(), AlwaysUp);
    assert_eq!(DirectionUp.transpose(), DirectionDown);
    assert_eq!(DirectionDown.transpose(), DirectionUp);
    assert_eq!(SmartUp.transpose(), SmartDown);
    assert_eq!(SmartDown.transpose(), SmartUp);
}
