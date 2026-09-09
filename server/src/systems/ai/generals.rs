//! Generals: a command-level competition class.
//!
//! A fighter model pilots one ship through the 50KiB WASM strategy ABI. A
//! *general* instead commands a whole team: an OpenRouter model looks at the
//! battlefield (a rendered image plus a text briefing) every few seconds and
//! issues one mass call — attack, retreat, hold, push or defend the
//! objective — which every ship on its side then acts on.
//!
//! The server owns only the order: validation, expiry, and application onto
//! the existing commander waypoint channel that the bot AI already follows.
//! The model call itself lives outside the tick in a worker, so a slow or
//! failed completion can never stall the simulation; a stale order simply
//! expires and the team reverts to its own judgement.

use crate::core::types::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// How long an order stands before the team reverts to its own judgement.
/// Long enough to cross the map, short enough that a dead worker cannot pin
/// a team to a stale call.
pub const GENERAL_ORDER_TTL_MS: u64 = 12_000;
/// Rationale text is echoed to players, so keep it to a headline.
pub const GENERAL_RATIONALE_MAX_CHARS: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneralPosture {
    /// Commit the team to the target: press the attack.
    MassAttack,
    /// Break contact and regroup on the target.
    Retreat,
    /// Hold current ground; no repositioning order.
    Hold,
    /// Push the objective (enemy flag / contested zone).
    PushObjective,
    /// Fall back onto our own objective and hold it.
    DefendObjective,
}

impl GeneralPosture {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().replace([' ', '-'], "_").as_str() {
            "mass_attack" | "attack" | "push" | "charge" => Some(Self::MassAttack),
            "retreat" | "fall_back" | "regroup" | "withdraw" => Some(Self::Retreat),
            "hold" | "hold_position" | "stand" => Some(Self::Hold),
            "push_objective" | "push_flag" | "capture" => Some(Self::PushObjective),
            "defend_objective" | "defend_flag" | "defend" => Some(Self::DefendObjective),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::MassAttack => "mass_attack",
            Self::Retreat => "retreat",
            Self::Hold => "hold",
            Self::PushObjective => "push_objective",
            Self::DefendObjective => "defend_objective",
        }
    }

    /// Short line shown to players when the order lands.
    pub fn headline(self) -> &'static str {
        match self {
            Self::MassAttack => "MASS ATTACK",
            Self::Retreat => "RETREAT",
            Self::Hold => "HOLD",
            Self::PushObjective => "PUSH THE OBJECTIVE",
            Self::DefendObjective => "DEFEND THE OBJECTIVE",
        }
    }

    /// Whether the order repositions the team. `Hold` deliberately issues no
    /// waypoint: it means "stop being ordered around", not "freeze".
    pub fn moves_the_team(self) -> bool {
        !matches!(self, Self::Hold)
    }

    /// Whether the order is strong enough to redirect a fighter that is
    /// running its own strategy. A general may pull its ships out of a losing
    /// fight or throw them into a push; it may not micromanage them.
    pub fn overrides_fighter_movement(self) -> bool {
        matches!(self, Self::Retreat | Self::MassAttack)
    }
}

/// One standing order for one team.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralOrder {
    pub team_id: u8,
    pub posture: GeneralPosture,
    /// Order target in world coordinates. Stored as scalars because the
    /// engine's `Vec2` is not serde-serialisable.
    pub target_x: f32,
    pub target_y: f32,
    /// OpenRouter id of the commanding model, e.g. "anthropic/claude-opus-5".
    pub model_id: String,
    /// Display name shown to players.
    pub model_name: String,
    /// The general's own one-line reasoning, echoed to spectators.
    pub rationale: String,
    pub issued_at_ms: u64,
    pub expires_at_ms: u64,
    /// Sequence number, so clients can tell a re-issued order from a repeat.
    pub sequence: u64,
}

impl GeneralOrder {
    pub fn target(&self) -> Vec2 {
        Vec2::new(self.target_x, self.target_y)
    }

    pub fn is_active_at(&self, now_ms: u64) -> bool {
        now_ms < self.expires_at_ms
    }

    pub fn headline(&self) -> String {
        format!("{}: {}", self.model_name, self.posture.headline())
    }
}

/// Everything the server accepts from a general worker. Deliberately small:
/// the worker decides, the server validates and applies.
#[derive(Debug, Clone, Deserialize)]
pub struct GeneralOrderRequest {
    pub team_id: u8,
    /// Posture keyword; see [`GeneralPosture::parse`].
    pub order: String,
    pub target_x: f32,
    pub target_y: f32,
    pub model_id: String,
    #[serde(default)]
    pub model_name: Option<String>,
    #[serde(default)]
    pub rationale: Option<String>,
    /// Optional override, clamped server-side.
    #[serde(default)]
    pub ttl_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeneralOrderError {
    UnknownTeam(u8),
    UnknownPosture(String),
    NonFiniteTarget,
    MissingModelId,
}

impl std::fmt::Display for GeneralOrderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTeam(team) => write!(f, "team {team} cannot be commanded"),
            Self::UnknownPosture(raw) => write!(f, "unknown order '{raw}'"),
            Self::NonFiniteTarget => write!(f, "target must be finite"),
            Self::MissingModelId => write!(f, "model_id is required"),
        }
    }
}

/// Validate a worker's request into an order, clamping everything that
/// reaches the simulation. World bounds are passed in so this stays a pure
/// function.
pub fn validate_order(
    request: &GeneralOrderRequest,
    now_ms: u64,
    sequence: u64,
    world_min: Vec2,
    world_max: Vec2,
) -> Result<GeneralOrder, GeneralOrderError> {
    if request.team_id != 1 && request.team_id != 2 {
        return Err(GeneralOrderError::UnknownTeam(request.team_id));
    }
    let posture = GeneralPosture::parse(&request.order)
        .ok_or_else(|| GeneralOrderError::UnknownPosture(request.order.clone()))?;
    if !request.target_x.is_finite() || !request.target_y.is_finite() {
        return Err(GeneralOrderError::NonFiniteTarget);
    }
    let model_id = request.model_id.trim();
    if model_id.is_empty() {
        return Err(GeneralOrderError::MissingModelId);
    }

    let target_x = request.target_x.clamp(world_min.x, world_max.x);
    let target_y = request.target_y.clamp(world_min.y, world_max.y);
    let ttl = request
        .ttl_ms
        .unwrap_or(GENERAL_ORDER_TTL_MS)
        .clamp(1_000, 60_000);
    let model_name = request
        .model_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(model_id)
        .to_owned();
    let mut rationale = request
        .rationale
        .as_deref()
        .unwrap_or_default()
        .trim()
        .replace(['\n', '\r'], " ");
    if rationale.chars().count() > GENERAL_RATIONALE_MAX_CHARS {
        rationale = rationale
            .chars()
            .take(GENERAL_RATIONALE_MAX_CHARS)
            .collect::<String>();
    }

    Ok(GeneralOrder {
        team_id: request.team_id,
        posture,
        target_x,
        target_y,
        model_id: model_id.to_owned(),
        model_name,
        rationale,
        issued_at_ms: now_ms,
        expires_at_ms: now_ms.saturating_add(ttl),
        sequence,
    })
}

/// Standing orders, one per team.
#[derive(Debug, Default)]
pub struct GeneralOrderBoard {
    orders: HashMap<u8, GeneralOrder>,
}

impl GeneralOrderBoard {
    pub fn set(&mut self, order: GeneralOrder) {
        self.orders.insert(order.team_id, order);
    }

    pub fn active_for_team(&self, team_id: u8, now_ms: u64) -> Option<&GeneralOrder> {
        self.orders
            .get(&team_id)
            .filter(|order| order.is_active_at(now_ms))
    }

    /// Active orders, newest team first for stable output.
    pub fn active(&self, now_ms: u64) -> Vec<GeneralOrder> {
        let mut active: Vec<GeneralOrder> = self
            .orders
            .values()
            .filter(|order| order.is_active_at(now_ms))
            .cloned()
            .collect();
        active.sort_by_key(|order| order.team_id);
        active
    }

    pub fn prune(&mut self, now_ms: u64) {
        self.orders.retain(|_, order| order.is_active_at(now_ms));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(order: &str) -> GeneralOrderRequest {
        GeneralOrderRequest {
            team_id: 1,
            order: order.to_owned(),
            target_x: 100.0,
            target_y: -50.0,
            model_id: "anthropic/claude-opus-5".to_owned(),
            model_name: Some("Claude Opus 5".to_owned()),
            rationale: Some("their flag is undefended".to_owned()),
            ttl_ms: None,
        }
    }

    fn bounds() -> (Vec2, Vec2) {
        (Vec2::new(-800.0, -600.0), Vec2::new(800.0, 600.0))
    }

    #[test]
    fn posture_parsing_accepts_natural_phrasings() {
        assert_eq!(GeneralPosture::parse("MASS ATTACK"), Some(GeneralPosture::MassAttack));
        assert_eq!(GeneralPosture::parse("fall-back"), Some(GeneralPosture::Retreat));
        assert_eq!(GeneralPosture::parse(" Hold "), Some(GeneralPosture::Hold));
        assert_eq!(
            GeneralPosture::parse("push_flag"),
            Some(GeneralPosture::PushObjective)
        );
        assert_eq!(GeneralPosture::parse("nonsense"), None);
    }

    #[test]
    fn hold_issues_no_movement_and_never_overrides_a_fighter() {
        assert!(!GeneralPosture::Hold.moves_the_team());
        assert!(!GeneralPosture::Hold.overrides_fighter_movement());
        assert!(GeneralPosture::MassAttack.moves_the_team());
        assert!(GeneralPosture::Retreat.overrides_fighter_movement());
        // A general may commit or extract the team, not micromanage it.
        assert!(!GeneralPosture::PushObjective.overrides_fighter_movement());
    }

    #[test]
    fn validation_clamps_target_and_ttl() {
        let (min, max) = bounds();
        let mut req = request("mass_attack");
        req.target_x = 99_999.0;
        req.target_y = -99_999.0;
        req.ttl_ms = Some(10_000_000);
        let order = validate_order(&req, 1_000, 7, min, max).expect("valid");
        assert_eq!(order.target_x, 800.0);
        assert_eq!(order.target_y, -600.0);
        assert_eq!(order.target(), Vec2::new(800.0, -600.0));
        assert_eq!(order.expires_at_ms, 1_000 + 60_000);
        assert_eq!(order.sequence, 7);
        assert_eq!(order.posture, GeneralPosture::MassAttack);
    }

    #[test]
    fn validation_rejects_bad_input() {
        let (min, max) = bounds();
        let mut req = request("mass_attack");
        req.team_id = 3;
        assert_eq!(
            validate_order(&req, 0, 0, min, max).unwrap_err(),
            GeneralOrderError::UnknownTeam(3)
        );

        let mut req = request("dance");
        req.team_id = 1;
        assert!(matches!(
            validate_order(&req, 0, 0, min, max).unwrap_err(),
            GeneralOrderError::UnknownPosture(_)
        ));

        let mut req = request("retreat");
        req.target_x = f32::NAN;
        assert_eq!(
            validate_order(&req, 0, 0, min, max).unwrap_err(),
            GeneralOrderError::NonFiniteTarget
        );

        let mut req = request("retreat");
        req.model_id = "   ".to_owned();
        assert_eq!(
            validate_order(&req, 0, 0, min, max).unwrap_err(),
            GeneralOrderError::MissingModelId
        );
    }

    #[test]
    fn rationale_is_trimmed_flattened_and_capped() {
        let (min, max) = bounds();
        let mut req = request("hold");
        req.rationale = Some(format!("  line one\nline two {}  ", "x".repeat(400)));
        let order = validate_order(&req, 0, 0, min, max).expect("valid");
        assert!(!order.rationale.contains('\n'));
        assert!(order.rationale.starts_with("line one line two"));
        assert_eq!(order.rationale.chars().count(), GENERAL_RATIONALE_MAX_CHARS);
    }

    #[test]
    fn board_expires_orders_so_a_dead_worker_cannot_pin_a_team() {
        let (min, max) = bounds();
        let mut board = GeneralOrderBoard::default();
        let order = validate_order(&request("mass_attack"), 1_000, 1, min, max).expect("valid");
        let expires = order.expires_at_ms;
        board.set(order);

        assert!(board.active_for_team(1, 1_500).is_some());
        assert!(board.active_for_team(2, 1_500).is_none());
        assert_eq!(board.active(1_500).len(), 1);

        assert!(board.active_for_team(1, expires).is_none());
        assert!(board.active(expires).is_empty());
        board.prune(expires);
        assert!(board.active(1_500).is_empty(), "pruned order does not come back");
    }

    #[test]
    fn a_new_order_replaces_the_standing_one_for_that_team() {
        let (min, max) = bounds();
        let mut board = GeneralOrderBoard::default();
        board.set(validate_order(&request("mass_attack"), 1_000, 1, min, max).unwrap());
        let mut second = request("retreat");
        second.model_id = "openai/gpt-6-astra".to_owned();
        board.set(validate_order(&second, 2_000, 2, min, max).unwrap());

        let active = board.active(2_100);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].posture, GeneralPosture::Retreat);
        assert_eq!(active[0].model_id, "openai/gpt-6-astra");
        assert_eq!(active[0].sequence, 2);
    }
}
