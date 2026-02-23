use crate::scaling::router::{classify_mmr_band, RendezvousShardRouter, ShardId};
use dashmap::DashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct MatchShardAssignment {
    pub match_id: String,
    pub primary_shard: ShardId,
    pub replica_shards: Vec<ShardId>,
    pub mmr_band: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HorizontalScalingCoordinator {
    router: Arc<RendezvousShardRouter>,
    assignments: Arc<DashMap<String, MatchShardAssignment>>,
    replica_count: usize,
}

impl HorizontalScalingCoordinator {
    pub fn new(shard_count: usize, replica_count: usize) -> Self {
        Self {
            router: Arc::new(RendezvousShardRouter::new(shard_count)),
            assignments: Arc::new(DashMap::new()),
            replica_count: replica_count.max(1),
        }
    }

    pub fn shard_count(&self) -> usize {
        self.router.shard_count()
    }

    pub fn assignment_for_match(&self, match_id: &str) -> MatchShardAssignment {
        if let Some(existing) = self.assignments.get(match_id) {
            return existing.clone();
        }

        let replicas = self
            .router
            .assign_with_replication(match_id, self.replica_count.max(1));
        let primary = replicas.first().copied().unwrap_or(0);
        let assignment = MatchShardAssignment {
            match_id: match_id.to_owned(),
            primary_shard: primary,
            replica_shards: replicas,
            mmr_band: None,
        };
        self.assignments
            .insert(match_id.to_owned(), assignment.clone());
        assignment
    }

    pub fn assignment_for_match_with_mmr(&self, match_id: &str, mmr: f32) -> MatchShardAssignment {
        self.assignment_for_match_with_band(match_id, classify_mmr_band(mmr))
    }

    pub fn assignment_for_match_with_band(
        &self,
        match_id: &str,
        mmr_band: &str,
    ) -> MatchShardAssignment {
        let normalized_band = mmr_band.trim().to_ascii_lowercase();
        let cache_key = format!("{}|{}", match_id, normalized_band);
        if let Some(existing) = self.assignments.get(cache_key.as_str()) {
            return existing.clone();
        }

        let replicas = self.router.assign_with_mmr_replication(
            match_id,
            match normalized_band.as_str() {
                "rookie" => 0.0,
                "bronze" => 180.0,
                "silver" => 350.0,
                "gold" => 700.0,
                "elite" => 1200.0,
                _ => 350.0,
            },
            self.replica_count.max(1),
        );
        let primary = replicas.first().copied().unwrap_or(0);
        let assignment = MatchShardAssignment {
            match_id: match_id.to_owned(),
            primary_shard: primary,
            replica_shards: replicas,
            mmr_band: Some(normalized_band),
        };
        self.assignments.insert(cache_key, assignment.clone());
        assignment
    }

    pub fn is_local_owner(&self, match_id: &str, local_shard: ShardId) -> bool {
        let assignment = self.assignment_for_match(match_id);
        assignment.primary_shard == local_shard
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_stable_match_assignment() {
        let coordinator = HorizontalScalingCoordinator::new(5, 2);
        let first = coordinator.assignment_for_match("match-a");
        let second = coordinator.assignment_for_match("match-a");
        assert_eq!(first.primary_shard, second.primary_shard);
        assert_eq!(first.replica_shards, second.replica_shards);
    }

    #[test]
    fn local_owner_is_derived_from_primary_shard() {
        let coordinator = HorizontalScalingCoordinator::new(4, 2);
        let assignment = coordinator.assignment_for_match("match-b");
        assert!(coordinator.is_local_owner("match-b", assignment.primary_shard));
    }

    #[test]
    fn mmr_band_assignment_is_stable() {
        let coordinator = HorizontalScalingCoordinator::new(6, 2);
        let first = coordinator.assignment_for_match_with_mmr("match-x", 720.0);
        let second = coordinator.assignment_for_match_with_band("match-x", "gold");
        assert_eq!(first.primary_shard, second.primary_shard);
        assert_eq!(first.mmr_band.as_deref(), Some("gold"));
        assert_eq!(second.mmr_band.as_deref(), Some("gold"));
    }
}
