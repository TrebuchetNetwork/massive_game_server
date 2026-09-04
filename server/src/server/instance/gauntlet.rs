//! Co-op gauntlet wave escalation.
//!
//! The gauntlet is humans + the exhibition model roster (team 1) against a
//! generic bot wave (team 2). Without escalation every match was the same
//! 14-bot Easy wave; the natural progression is a streak: hold the wave and
//! the next one is bigger and sharper, lose and the gauntlet resets to wave
//! 1. The streak survives restarts (`gauntlet_progress.json` in the match
//! store dir) so a run is not wiped by the daily service cycle.
//!
//! Wave size is applied through the existing `target_bot_count` knob (the
//! bot population manager converges on it every tick); the mechanics tier
//! is read by the generic bot AI through [`gauntlet_wave_tier_index`].

use super::*;
use std::sync::atomic::{AtomicU32, AtomicU8};

/// Default per-wave size increment when `MGS_GAUNTLET_WAVE_STEP` is unset.
pub(crate) const DEFAULT_GAUNTLET_WAVE_STEP: usize = 2;
/// Default wave-size ceiling when `MGS_GAUNTLET_WAVE_MAX` is unset:
/// 10 allies + 22 wave bots fills the 32-slot MobileStandard match.
pub(crate) const DEFAULT_GAUNTLET_WAVE_MAX: usize = 22;
/// Streak thresholds for the wave mechanics tier (waves 1-3 Easy, 4-6
/// Normal, 7+ Hard).
const NORMAL_TIER_MIN_STREAK: u32 = 3;
const HARD_TIER_MIN_STREAK: u32 = 6;

const PROGRESS_FILE_NAME: &str = "gauntlet_progress.json";

/// Persisted streak state.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GauntletProgress {
    /// Consecutive waves held; the next wave number is `streak + 1`.
    #[serde(default)]
    pub streak: u32,
    #[serde(default)]
    pub best_streak: u32,
    #[serde(default)]
    pub waves_held: u32,
    #[serde(default)]
    pub waves_lost: u32,
    #[serde(default)]
    pub updated_at_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GauntletWaveTier {
    Easy = 0,
    Normal = 1,
    Hard = 2,
}

impl GauntletWaveTier {
    pub fn from_streak(streak: u32) -> Self {
        if streak >= HARD_TIER_MIN_STREAK {
            GauntletWaveTier::Hard
        } else if streak >= NORMAL_TIER_MIN_STREAK {
            GauntletWaveTier::Normal
        } else {
            GauntletWaveTier::Easy
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            GauntletWaveTier::Easy => "Easy",
            GauntletWaveTier::Normal => "Normal",
            GauntletWaveTier::Hard => "Hard",
        }
    }
}

/// One wave's composition.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GauntletWave {
    /// 1-based wave number (`streak + 1`).
    pub wave_number: u32,
    /// Generic team-2 bots fielded against the alliance.
    pub wave_size: usize,
    /// Mechanics tier label (Easy / Normal / Hard).
    pub tier: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GauntletWaveConfig {
    pub base_size: usize,
    pub step: usize,
    pub max_size: usize,
}

impl GauntletWaveConfig {
    pub fn wave_for_streak(&self, streak: u32) -> GauntletWave {
        let growth = (streak as usize).saturating_mul(self.step);
        let ceiling = self.max_size.max(self.base_size);
        GauntletWave {
            wave_number: streak.saturating_add(1),
            wave_size: self.base_size.saturating_add(growth).min(ceiling),
            tier: GauntletWaveTier::from_streak(streak).label().to_owned(),
        }
    }
}

/// Outcome of one gauntlet match, attached to the match-end summary so the
/// client can narrate the run and telemetry can grade waves by difficulty.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GauntletMatchOutcome {
    pub wave_fought: GauntletWave,
    /// `true` = alliance held, `false` = wave prevailed, `None` = stalemate
    /// (the streak neither grows nor resets).
    pub held: Option<bool>,
    pub streak_after: u32,
    pub best_streak: u32,
    pub new_record: bool,
    pub next_wave: GauntletWave,
}

/// Public snapshot for the live scoreboard endpoint.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GauntletStatus {
    pub wave_number: u32,
    pub wave_size: usize,
    pub tier: String,
    pub streak: u32,
    pub best_streak: u32,
    pub waves_held: u32,
    pub waves_lost: u32,
}

static PROGRESS: ParkingLotRwLock<GauntletProgress> =
    ParkingLotRwLock::new(GauntletProgress {
        streak: 0,
        best_streak: 0,
        waves_held: 0,
        waves_lost: 0,
        updated_at_ms: 0,
    });
/// Mirrors `PROGRESS.streak` for lock-free reads from the AI hot path.
static WAVE_TIER: AtomicU8 = AtomicU8::new(GauntletWaveTier::Easy as u8);
/// Wave-1 size resolved at bootstrap (env override or configured target
/// minus allies); later waves grow from it.
static BASE_WAVE_SIZE: AtomicU32 = AtomicU32::new(0);

/// Current wave mechanics tier as `GauntletWaveTier as u8` (0 Easy, 1
/// Normal, 2 Hard). Read every bot tick, so kept atomic.
pub(crate) fn gauntlet_wave_tier_index() -> u8 {
    WAVE_TIER.load(AtomicOrdering::Relaxed)
}

fn progress_file_path(store_dir: &Path) -> PathBuf {
    store_dir.join(PROGRESS_FILE_NAME)
}

fn load_progress(store_dir: &Path) -> GauntletProgress {
    let path = progress_file_path(store_dir);
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
            warn!(
                "gauntlet progress at {} is unreadable ({}); starting from wave 1",
                path.display(),
                err
            );
            GauntletProgress::default()
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => GauntletProgress::default(),
        Err(err) => {
            warn!(
                "failed to read gauntlet progress at {}: {}; starting from wave 1",
                path.display(),
                err
            );
            GauntletProgress::default()
        }
    }
}

fn save_progress(store_dir: &Path, progress: &GauntletProgress) -> Result<(), String> {
    fs::create_dir_all(store_dir).map_err(|err| err.to_string())?;
    let path = progress_file_path(store_dir);
    let tmp = path.with_extension("json.tmp");
    let payload = serde_json::to_vec_pretty(progress).map_err(|err| err.to_string())?;
    fs::write(&tmp, payload).map_err(|err| err.to_string())?;
    fs::rename(&tmp, &path).map_err(|err| err.to_string())
}

/// Resolves the wave-1 size, loads persisted progress and returns the bot
/// target the server should start with. Called once from the constructor
/// before the initial bot spawn; a no-op passthrough outside the gauntlet.
pub(super) fn bootstrap_gauntlet(store_dir: &Path, configured_target_bot_count: usize) -> usize {
    if !coop_gauntlet_enabled() {
        return configured_target_bot_count;
    }
    let allies = gauntlet_ally_bots();
    let base = gauntlet_wave_base()
        .unwrap_or_else(|| configured_target_bot_count.saturating_sub(allies))
        .max(1);
    BASE_WAVE_SIZE.store(base as u32, AtomicOrdering::Relaxed);

    let progress = load_progress(store_dir);
    let wave = gauntlet_wave_config().wave_for_streak(progress.streak);
    WAVE_TIER.store(
        GauntletWaveTier::from_streak(progress.streak) as u8,
        AtomicOrdering::Relaxed,
    );
    info!(
        "Co-op gauntlet resumed at wave {} ({} {} bots vs {} allies; streak={}, best={})",
        wave.wave_number, wave.wave_size, wave.tier, allies, progress.streak, progress.best_streak
    );
    *PROGRESS.write() = progress;
    allies.saturating_add(wave.wave_size)
}

pub(super) fn gauntlet_wave_config() -> GauntletWaveConfig {
    GauntletWaveConfig {
        base_size: BASE_WAVE_SIZE.load(AtomicOrdering::Relaxed).max(1) as usize,
        step: gauntlet_wave_step(),
        max_size: gauntlet_wave_max(),
    }
}

pub(crate) fn gauntlet_status() -> Option<GauntletStatus> {
    if !coop_gauntlet_enabled() {
        return None;
    }
    let progress = PROGRESS.read().clone();
    let wave = gauntlet_wave_config().wave_for_streak(progress.streak);
    Some(GauntletStatus {
        wave_number: wave.wave_number,
        wave_size: wave.wave_size,
        tier: wave.tier,
        streak: progress.streak,
        best_streak: progress.best_streak,
        waves_held: progress.waves_held,
        waves_lost: progress.waves_lost,
    })
}

/// Pure streak transition: held grows the streak, a loss resets it, a
/// stalemate leaves it alone.
pub(super) fn advance_progress(
    progress: &mut GauntletProgress,
    held: Option<bool>,
    now_ms: u64,
) -> bool {
    let mut new_record = false;
    match held {
        Some(true) => {
            progress.streak = progress.streak.saturating_add(1);
            progress.waves_held = progress.waves_held.saturating_add(1);
            if progress.streak > progress.best_streak {
                progress.best_streak = progress.streak;
                new_record = true;
            }
        }
        Some(false) => {
            progress.streak = 0;
            progress.waves_lost = progress.waves_lost.saturating_add(1);
        }
        None => {}
    }
    progress.updated_at_ms = now_ms;
    new_record
}

/// Decides the gauntlet verdict from final team scores: team 1 (alliance)
/// against team 2 (wave).
pub(super) fn gauntlet_verdict(team_scores: &HashMap<u8, i32>) -> Option<bool> {
    let alliance = team_scores.get(&1).copied().unwrap_or(0);
    let wave = team_scores.get(&2).copied().unwrap_or(0);
    match alliance.cmp(&wave) {
        std::cmp::Ordering::Greater => Some(true),
        std::cmp::Ordering::Less => Some(false),
        std::cmp::Ordering::Equal => None,
    }
}

impl MassiveGameServer {
    /// Wave the alliance is currently facing (or about to face).
    pub(super) fn current_gauntlet_wave(&self) -> GauntletWave {
        gauntlet_wave_config().wave_for_streak(PROGRESS.read().streak)
    }

    /// Records the result of a finished gauntlet match, persists the streak
    /// and re-targets the bot population for the next wave.
    pub(super) fn record_gauntlet_match_result(
        &self,
        team_scores: &HashMap<u8, i32>,
    ) -> GauntletMatchOutcome {
        let config = gauntlet_wave_config();
        let held = gauntlet_verdict(team_scores);
        let now_ms = self.get_server_timestamp_ms();

        let (wave_fought, progress_after, new_record) = {
            let mut progress = PROGRESS.write();
            let wave_fought = config.wave_for_streak(progress.streak);
            let new_record = advance_progress(&mut progress, held, now_ms);
            (wave_fought, progress.clone(), new_record)
        };
        let next_wave = config.wave_for_streak(progress_after.streak);

        WAVE_TIER.store(
            GauntletWaveTier::from_streak(progress_after.streak) as u8,
            AtomicOrdering::Relaxed,
        );
        let next_target = gauntlet_ally_bots().saturating_add(next_wave.wave_size) as u64;
        self.target_bot_count.store(next_target, AtomicOrdering::Relaxed);

        info!(
            "Gauntlet wave {} ({} {}) {}: streak={} best={} -> next wave {} ({} {} bots, bot target {})",
            wave_fought.wave_number,
            wave_fought.wave_size,
            wave_fought.tier,
            match held {
                Some(true) => "HELD",
                Some(false) => "LOST",
                None => "stalemate",
            },
            progress_after.streak,
            progress_after.best_streak,
            next_wave.wave_number,
            next_wave.wave_size,
            next_wave.tier,
            next_target
        );

        let store_dir = Arc::clone(&self.replay.match_store_dir);
        tokio::task::spawn_blocking(move || {
            if let Err(err) = save_progress(store_dir.as_path(), &progress_after) {
                warn!("failed to persist gauntlet progress: {}", err);
            }
        });

        GauntletMatchOutcome {
            wave_fought,
            held,
            streak_after: PROGRESS.read().streak,
            best_streak: PROGRESS.read().best_streak,
            new_record,
            next_wave,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> GauntletWaveConfig {
        GauntletWaveConfig {
            base_size: 14,
            step: 2,
            max_size: 22,
        }
    }

    #[test]
    fn wave_size_grows_by_step_and_caps() {
        let cfg = config();
        assert_eq!(cfg.wave_for_streak(0).wave_size, 14);
        assert_eq!(cfg.wave_for_streak(0).wave_number, 1);
        assert_eq!(cfg.wave_for_streak(3).wave_size, 20);
        assert_eq!(cfg.wave_for_streak(4).wave_size, 22);
        assert_eq!(cfg.wave_for_streak(40).wave_size, 22);
        assert_eq!(cfg.wave_for_streak(40).wave_number, 41);
    }

    #[test]
    fn wave_max_never_undercuts_base() {
        let cfg = GauntletWaveConfig {
            base_size: 14,
            step: 2,
            max_size: 4,
        };
        assert_eq!(cfg.wave_for_streak(0).wave_size, 14);
        assert_eq!(cfg.wave_for_streak(9).wave_size, 14);
    }

    #[test]
    fn tier_escalates_with_streak() {
        assert_eq!(GauntletWaveTier::from_streak(0), GauntletWaveTier::Easy);
        assert_eq!(GauntletWaveTier::from_streak(2), GauntletWaveTier::Easy);
        assert_eq!(GauntletWaveTier::from_streak(3), GauntletWaveTier::Normal);
        assert_eq!(GauntletWaveTier::from_streak(5), GauntletWaveTier::Normal);
        assert_eq!(GauntletWaveTier::from_streak(6), GauntletWaveTier::Hard);
        assert_eq!(config().wave_for_streak(6).tier, "Hard");
    }

    #[test]
    fn progress_grows_resets_and_tracks_record() {
        let mut progress = GauntletProgress::default();
        assert!(advance_progress(&mut progress, Some(true), 1));
        assert!(advance_progress(&mut progress, Some(true), 2));
        assert_eq!(progress.streak, 2);
        assert_eq!(progress.best_streak, 2);
        assert_eq!(progress.waves_held, 2);

        assert!(!advance_progress(&mut progress, None, 3));
        assert_eq!(progress.streak, 2, "stalemate keeps the streak");

        assert!(!advance_progress(&mut progress, Some(false), 4));
        assert_eq!(progress.streak, 0);
        assert_eq!(progress.best_streak, 2);
        assert_eq!(progress.waves_lost, 1);

        assert!(!advance_progress(&mut progress, Some(true), 5));
        assert!(!advance_progress(&mut progress, Some(true), 6));
        assert!(
            advance_progress(&mut progress, Some(true), 7),
            "beating the previous best is a record"
        );
        assert_eq!(progress.best_streak, 3);
        assert_eq!(progress.updated_at_ms, 7);
    }

    #[test]
    fn verdict_reads_alliance_vs_wave_scores() {
        let mut scores = HashMap::new();
        assert_eq!(gauntlet_verdict(&scores), None);
        scores.insert(1u8, 12);
        assert_eq!(gauntlet_verdict(&scores), Some(true));
        scores.insert(2u8, 15);
        assert_eq!(gauntlet_verdict(&scores), Some(false));
        scores.insert(1u8, 15);
        assert_eq!(gauntlet_verdict(&scores), None);
    }

    #[test]
    fn progress_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!(
            "mgs-gauntlet-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        assert_eq!(load_progress(&dir), GauntletProgress::default());
        let progress = GauntletProgress {
            streak: 4,
            best_streak: 7,
            waves_held: 20,
            waves_lost: 3,
            updated_at_ms: 99,
        };
        save_progress(&dir, &progress).expect("save");
        assert_eq!(load_progress(&dir), progress);
        let _ = fs::remove_dir_all(&dir);
    }
}
