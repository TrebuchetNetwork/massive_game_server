use seahash::hash;

pub type ShardId = usize;

#[derive(Debug, Clone)]
pub struct RendezvousShardRouter {
    shard_ids: Vec<ShardId>,
}

impl RendezvousShardRouter {
    pub fn new(shard_count: usize) -> Self {
        let count = shard_count.max(1);
        let mut shard_ids = Vec::with_capacity(count);
        for id in 0..count {
            shard_ids.push(id);
        }
        Self { shard_ids }
    }

    pub fn shard_count(&self) -> usize {
        self.shard_ids.len()
    }

    pub fn assign(&self, key: &str) -> ShardId {
        self.assign_with_replication(key, 1)
            .into_iter()
            .next()
            .unwrap_or(0)
    }

    pub fn assign_with_replication(&self, key: &str, replicas: usize) -> Vec<ShardId> {
        let replica_count = replicas.max(1).min(self.shard_ids.len());
        let mut weighted = self
            .shard_ids
            .iter()
            .map(|shard_id| (*shard_id, rendezvous_score(key, *shard_id)))
            .collect::<Vec<_>>();
        weighted.sort_by(|left, right| right.1.cmp(&left.1));
        weighted
            .into_iter()
            .take(replica_count)
            .map(|(shard_id, _)| shard_id)
            .collect()
    }
}

#[inline]
fn rendezvous_score(key: &str, shard_id: ShardId) -> u64 {
    let mut input = String::with_capacity(key.len() + 16);
    input.push_str(key);
    input.push('#');
    input.push_str(&shard_id.to_string());
    hash(input.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_assignment_for_same_key() {
        let router = RendezvousShardRouter::new(8);
        let first = router.assign("match:abc");
        let second = router.assign("match:abc");
        assert_eq!(first, second);
    }

    #[test]
    fn replication_returns_unique_shards() {
        let router = RendezvousShardRouter::new(6);
        let replicas = router.assign_with_replication("match:xyz", 3);
        assert_eq!(replicas.len(), 3);
        let mut dedup = replicas.clone();
        dedup.sort_unstable();
        dedup.dedup();
        assert_eq!(dedup.len(), 3);
    }
}
