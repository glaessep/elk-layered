//! Port of ELK's `BinaryIndexedTree`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/counting/BinaryIndexedTree.java

/// `BinaryIndexedTree`.
///
/// elkjs does not check array bounds. Java would throw `ArrayIndexOutOfBoundsException` for an
/// index past the end; elkjs reads `undefined` and adds `NaN` to the sum, or grows the array. Both
/// happen when a counter reads the stale position of a port it did not number in this call. This
/// tree follows elkjs: `add` counts an index past the end in `size` and in nothing else, and `rank`
/// of such an index is `None`, which stands for `NaN`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryIndexedTree {
    binary_sums: Vec<usize>,
    nums_per_index: Vec<usize>,
    size: usize,
}

impl BinaryIndexedTree {
    pub fn new(max_num: usize) -> Self {
        Self {
            binary_sums: vec![0; max_num + 1],
            nums_per_index: vec![0; max_num],
            size: 0,
        }
    }

    pub fn add(&mut self, index: usize) {
        self.size += 1;
        if let Some(entries) = self.nums_per_index.get_mut(index) {
            *entries += 1;
        }
        let mut i = index + 1;
        while i < self.binary_sums.len() {
            self.binary_sums[i] += 1;
            i += i & i.wrapping_neg();
        }
    }

    /// The sum of the entries before `index`, or `None` when elkjs would get `NaN`.
    pub fn rank(&self, index: usize) -> Option<usize> {
        let mut i = index;
        let mut sum = Some(0usize);
        while i > 0 {
            sum = match (sum, self.binary_sums.get(i)) {
                (Some(sum), Some(entries)) => Some(sum + entries),
                _ => None,
            };
            i -= i & i.wrapping_neg();
        }
        sum
    }

    pub fn size(&self) -> usize {
        self.size
    }

    /// `BinaryIndexedTree.clear`: removes every entry.
    pub fn clear(&mut self) {
        self.binary_sums.fill(0);
        self.nums_per_index.fill(0);
        self.size = 0;
    }

    pub fn remove_all(&mut self, index: usize) {
        let Some(num_entries) = self.nums_per_index.get_mut(index) else {
            return;
        };
        let num_entries = std::mem::take(num_entries);
        if num_entries == 0 {
            return;
        }

        self.size -= num_entries;
        let mut i = index + 1;
        while i < self.binary_sums.len() {
            self.binary_sums[i] -= num_entries;
            i += i & i.wrapping_neg();
        }
    }
}
