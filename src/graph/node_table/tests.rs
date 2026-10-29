use super::*;

fn nodes(indexes: &[usize]) -> IdVec<NodeId, NodeId> {
    indexes.iter().map(|index| NodeId::new(*index)).collect()
}

// Mutation: in `row_mut`, return the row of node 0 for every node.
#[test]
fn a_row_exists_only_after_it_was_written() {
    let mut table = NodeTable::<Vec<usize>>::default();
    assert_eq!(table.get(NodeId::new(2)), None);

    table.row_mut(NodeId::new(2)).push(7);

    assert_eq!(table.get(NodeId::new(2)), Some(&vec![7]));
    // The rows before it were not written.
    assert_eq!(table.get(NodeId::new(0)), None);
    assert_eq!(table.get(NodeId::new(3)), None);
}

// Mutation: in `row_mut`, create a default row for every node before the one asked for.
#[test]
fn writing_a_row_leaves_the_rows_before_it_missing() {
    let mut table = NodeTable::<Vec<usize>>::default();
    table.row_mut(NodeId::new(1));

    assert_eq!(table.get(NodeId::new(1)), Some(&vec![]));
    assert_eq!(table.get(NodeId::new(0)), None);
}

// Mutation: in `get_or_default`, answer the default row also for a node that has a row.
#[test]
fn get_or_default_answers_the_row_or_the_default() {
    let mut table = NodeTable::<Vec<usize>>::default();
    table.row_mut(NodeId::new(1)).push(4);

    assert_eq!(table.get_or_default(NodeId::new(1)), vec![4]);
    assert_eq!(table.get_or_default(NodeId::new(0)), Vec::<usize>::new());
    assert_eq!(table.get_or_default(NodeId::new(9)), Vec::<usize>::new());
}

// Mutation: in `subset`, copy the rows in the order of the table, not in the order of `nodes`.
#[test]
fn subset_takes_the_rows_of_the_nodes_in_their_order() {
    let mut table = NodeTable::<Vec<usize>>::default();
    for index in 0..4 {
        table.row_mut(NodeId::new(index)).push(index * 10);
    }

    let subset = table.subset(&nodes(&[3, 1]));

    assert_eq!(subset.get(NodeId::new(0)), Some(&vec![30]));
    assert_eq!(subset.get(NodeId::new(1)), Some(&vec![10]));
    assert_eq!(subset.get(NodeId::new(2)), None);
}

// Mutation: in `subset`, give a node without a row a default row.
#[test]
fn subset_keeps_a_node_without_a_row_without_one() {
    let mut table = NodeTable::<Vec<usize>>::default();
    table.row_mut(NodeId::new(0)).push(5);

    let subset = table.subset(&nodes(&[0, 9, 0]));

    assert_eq!(subset.get(NodeId::new(0)), Some(&vec![5]));
    assert_eq!(subset.get(NodeId::new(1)), None);
    assert_eq!(subset.get(NodeId::new(2)), Some(&vec![5]));
}

// Mutation: in `absorb`, put each row at its own index, not at `node_map[local]`.
#[test]
fn absorb_moves_each_row_to_the_node_the_map_names() {
    let mut part = NodeTable::<Vec<usize>>::default();
    part.row_mut(NodeId::new(0)).push(10);
    part.row_mut(NodeId::new(2)).push(12);
    let mut table = NodeTable::<Vec<usize>>::default();
    table.row_mut(NodeId::new(5)).push(99);

    table.absorb(&nodes(&[3, 4, 1]), part);

    assert_eq!(table.get(NodeId::new(3)), Some(&vec![10]));
    assert_eq!(table.get(NodeId::new(1)), Some(&vec![12]));
    assert_eq!(table.get(NodeId::new(4)), None);
    // A row that is already here stays.
    assert_eq!(table.get(NodeId::new(5)), Some(&vec![99]));
}

// Mutation: in `absorb`, move a missing row too, as a default row.
#[test]
fn absorb_leaves_a_node_without_a_row_without_one() {
    let mut part = NodeTable::<Vec<usize>>::default();
    part.row_mut(NodeId::new(1)).push(1);
    let mut table = NodeTable::<Vec<usize>>::default();

    table.absorb(&nodes(&[7, 8]), part);

    assert_eq!(table.get(NodeId::new(7)), None);
    assert_eq!(table.get(NodeId::new(8)), Some(&vec![1]));
}
