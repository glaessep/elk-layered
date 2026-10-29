use super::*;
use crate::graph::{NodeId, PortType};

fn ports(names: &[&str]) -> NodePorts {
    let mut ports = NodePorts::default();
    for name in names {
        ports.push(LPort::new(*name, NodeId::new(0), PortType::Input));
    }
    ports
}

fn names(ports: &NodePorts) -> Vec<&str> {
    ports.iter().map(|port| port.id.as_str()).collect()
}

// Mutation: have `set_order` move the ports between their slots instead of rewriting the list.
#[test]
fn a_port_keeps_its_id_when_the_order_changes() {
    let mut ports = ports(&["a", "b", "c"]);

    assert!(ports.set_order(port_ids([2, 0, 1]).to_vec()));

    assert_eq!(names(&ports), ["c", "a", "b"]);
    assert_eq!(ports[PortId::new(0)].id, "a");
    assert_eq!(ports[PortId::new(2)].id, "c");
    assert_eq!(ports.position(PortId::new(0)), Some(1));
    assert_eq!(ports.id_at(0), Some(PortId::new(2)));
}

// Mutation: make `set_order` return `false` for an order that is already the list's order.
#[test]
fn an_order_that_is_already_there_is_accepted_and_changes_nothing() {
    let mut ports = ports(&["a", "b", "c"]);
    assert!(ports.set_order(port_ids([2, 0, 1]).to_vec()));

    assert!(ports.set_order(port_ids([2, 0, 1]).to_vec()));

    assert_eq!(names(&ports), ["c", "a", "b"]);
}

// Mutation: swap in the new order before `set_order` has checked it.
#[test]
fn a_refused_order_leaves_the_list_as_it_was() {
    let mut ports = ports(&["a", "b", "c"]);

    assert!(!ports.set_order(port_ids([0, 1, 1]).to_vec()));
    assert!(!ports.set_order(port_ids([0, 1]).to_vec()));
    assert!(!ports.set_order(port_ids([0, 1, 7]).to_vec()));

    assert_eq!(names(&ports), ["a", "b", "c"]);
}

// Mutation: visit the slots in slot order, or visit a port that is off the list.
#[test]
fn for_each_listed_mut_visits_the_listed_ports_in_the_nodes_order() {
    let mut ports = ports(&["a", "b", "c", "d"]);
    ports.unlist(&port_ids([1]));
    assert!(ports.set_order(port_ids([3, 2, 0]).to_vec()));

    let mut visited = Vec::new();
    ports.for_each_listed_mut(|port| {
        visited.push(port.id.clone());
        port.id.push('!');
    });

    assert_eq!(visited, ["d", "c", "a"]);
    assert_eq!(names(&ports), ["d!", "c!", "a!"]);
    assert_eq!(ports[PortId::new(1)].id, "b");
}

// Mutation: drop the port in `unlist`, or give its slot to the next port that is pushed.
#[test]
fn a_port_off_the_list_keeps_its_slot_and_its_id() {
    let mut ports = ports(&["a", "b", "c"]);

    ports.unlist(&port_ids([1]));
    let d = ports.push(LPort::new("d", NodeId::new(0), PortType::Input));

    assert_eq!(names(&ports), ["a", "c", "d"]);
    assert_eq!(d, PortId::new(3));
    assert_eq!(ports.slot_count(), 4);
    assert_eq!(ports.position(PortId::new(1)), None);
    assert_eq!(
        ports.get(PortId::new(1)).map(|port| port.id.as_str()),
        Some("b")
    );
    // A listed order may not name the port that is off the list.
    assert!(!ports.set_order(port_ids([0, 1, 2]).to_vec()));
    assert!(ports.set_order(port_ids([3, 2, 0]).to_vec()));
    assert_eq!(names(&ports), ["d", "c", "a"]);
}

// Mutation: list a port twice, or list an id that has no slot.
#[test]
fn relist_puts_ports_back_at_the_end_and_skips_listed_and_unknown_ids() {
    let mut ports = ports(&["a", "b", "c"]);
    ports.unlist(&port_ids([0, 2]));

    ports.relist(&port_ids([2, 1, 0, 9]));

    assert_eq!(names(&ports), ["b", "c", "a"]);
}

// Mutation: read the first port from the first slot instead of the first place of the list.
#[test]
fn first_and_last_follow_the_list_not_the_slots() {
    let mut ports = ports(&["a", "b", "c"]);
    assert!(ports.set_order(port_ids([2, 1, 0]).to_vec()));

    assert_eq!(ports.first().map(|port| port.id.as_str()), Some("c"));
    assert_eq!(ports.last().map(|port| port.id.as_str()), Some("a"));
    ports.first_mut().unwrap().id = "z".to_string();
    assert_eq!(ports[PortId::new(2)].id, "z");
}
