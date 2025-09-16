use std::{cmp::Ordering, collections::BinaryHeap};

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct OperationIdDiff {
    pub entries: BinaryHeap<OperationIdDiffEntry>,
}

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct OperationIdDiffEntry {
    pub tag: String,
    pub operation_id_before: String,
    pub operation_id_after: String,
}

impl Ord for OperationIdDiffEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.tag
            .cmp(&other.tag)
            .then_with(|| self.operation_id_after.cmp(&other.operation_id_after))
            .then_with(|| self.operation_id_before.cmp(&other.operation_id_before))
    }
}

impl PartialOrd for OperationIdDiffEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
