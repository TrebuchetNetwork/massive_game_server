// massive_game_server/server/src/memory/numa.rs

use core_affinity::CoreId;
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Default)]
pub struct NumaTopology {
    // node_id -> core_ids
    pub nodes: BTreeMap<usize, Vec<usize>>,
}

impl NumaTopology {
    pub fn from_env() -> Self {
        // Format: "0:0,1,2;1:3,4,5"
        let raw = std::env::var("MGS_NUMA_NODE_MAP").unwrap_or_default();
        if raw.trim().is_empty() {
            return Self::single_node_from_local_cores();
        }

        let mut nodes: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for node_spec in raw.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            let mut parts = node_spec.split(':');
            let node_id = parts
                .next()
                .and_then(|p| p.trim().parse::<usize>().ok())
                .unwrap_or(0);
            let cores: Vec<usize> = parts
                .next()
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter_map(|core| core.parse::<usize>().ok())
                .collect();
            if !cores.is_empty() {
                nodes.insert(node_id, cores);
            }
        }

        if nodes.is_empty() {
            Self::single_node_from_local_cores()
        } else {
            Self { nodes }
        }
    }

    fn single_node_from_local_cores() -> Self {
        let cores = core_affinity::get_core_ids()
            .unwrap_or_default()
            .into_iter()
            .map(|core| core.id)
            .collect::<Vec<_>>();
        let mut nodes = BTreeMap::new();
        nodes.insert(0, cores);
        Self { nodes }
    }

    pub fn recommended_node_for_shard(&self, shard: usize) -> usize {
        if self.nodes.is_empty() {
            return 0;
        }
        let node_ids = self.nodes.keys().copied().collect::<Vec<_>>();
        node_ids[shard % node_ids.len()]
    }

    pub fn pin_current_thread_to_node(&self, node_id: usize) -> bool {
        let Some(core_ids) = core_affinity::get_core_ids() else {
            return false;
        };
        let Some(node_cores) = self.nodes.get(&node_id) else {
            return false;
        };
        if node_cores.is_empty() {
            return false;
        }
        static NODE_PIN_CURSOR: OnceLock<Mutex<BTreeMap<usize, usize>>> = OnceLock::new();
        let candidate_core_id = {
            let cursor_map = NODE_PIN_CURSOR.get_or_init(|| Mutex::new(BTreeMap::new()));
            let mut cursor_guard = cursor_map.lock().expect("NUMA pin cursor mutex poisoned");
            let cursor = cursor_guard.entry(node_id).or_insert(0);
            let selected = node_cores[*cursor % node_cores.len()];
            *cursor = cursor.wrapping_add(1);
            selected
        };
        let Some(core) = core_ids.into_iter().find(|id| id.id == candidate_core_id) else {
            return false;
        };
        core_affinity::set_for_current(CoreId { id: core.id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_nodes_round_robin() {
        let topology = NumaTopology {
            nodes: [(0usize, vec![0, 1]), (1usize, vec![2, 3])]
                .into_iter()
                .collect(),
        };
        assert_eq!(topology.recommended_node_for_shard(0), 0);
        assert_eq!(topology.recommended_node_for_shard(1), 1);
        assert_eq!(topology.recommended_node_for_shard(2), 0);
    }
}
