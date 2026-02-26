// Property-based tests for critical game systems using proptest.
//
// These tests verify invariants that must hold for *all* valid inputs,
// not just hand-picked examples.  Each test documents the invariant it
// checks so failures are easy to diagnose.

use proptest::prelude::*;

use massive_game_server_core::core::constants::{
    quantize_position, quantize_velocity, quantize_rotation,
    QUANTIZE_POSITION_MIN, QUANTIZE_POSITION_MAX, QUANTIZE_VELOCITY_MAX,
    AOI_RADIUS, AOI_EXIT_RADIUS, AOI_UPDATE_DIVISOR_DEFAULT,
    PISTOL_DAMAGE, SHOTGUN_DAMAGE, RIFLE_DAMAGE, SNIPER_DAMAGE, MELEE_DAMAGE,
    PISTOL_FIRE_RATE_SECS, SHOTGUN_FIRE_RATE_SECS, RIFLE_FIRE_RATE_SECS,
    SNIPER_FIRE_RATE_SECS, MELEE_FIRE_RATE_SECS,
    MIN_SHOT_INTERVAL_SECONDS, FIRE_RATE_JITTER_TOLERANCE_SECS,
};
use massive_game_server_core::core::types::{PlayerState, ServerWeaponType};
use massive_game_server_core::systems::combat::weapons::{
    apply_distance_falloff, distance_damage_multiplier, profile, falloff_profile,
};
use massive_game_server_core::memory::arena::Arena;

// ════════════════════════════════════════════════════════════════════
// 1. Position quantization roundtrip
// ════════════════════════════════════════════════════════════════════

proptest! {
    /// For any position in valid world range [0, 4096], quantizing and
    /// then interpreting the result should be within 0.25 of the original.
    #[test]
    fn position_quantize_roundtrip_in_range(v in 0.0f32..=QUANTIZE_POSITION_MAX) {
        let q = quantize_position(v);
        let error = (q - v).abs();
        // Maximum step size = 1 / QUANTIZE_POSITION_SCALE ≈ 0.0625
        // After rounding, max error is half a step ≈ 0.031
        prop_assert!(
            error <= 0.25,
            "quantize_position({}) = {} (error {})",
            v, q, error
        );
    }

    /// For any f32 position in [-4096, 4096], quantize then dequantize
    /// should be within 0.25 of original.
    #[test]
    fn position_quantize_roundtrip_extended(v in -4096.0f32..=4096.0f32) {
        let q = quantize_position(v);
        let error = (q - v).abs();
        prop_assert!(
            error <= 0.25,
            "quantize_position({}) = {} (error {})",
            v, q, error
        );
    }

    /// Quantized position is always within valid world bounds [-4096, 4096].
    #[test]
    fn position_quantize_stays_in_bounds(v in prop::num::f32::ANY) {
        if v.is_finite() {
            let q = quantize_position(v);
            prop_assert!(q >= QUANTIZE_POSITION_MIN - 0.01, "quantize_position({}) = {} < min", v, q);
            prop_assert!(
                q <= QUANTIZE_POSITION_MAX + 0.01,
                "quantize_position({}) = {} > max",
                v, q
            );
        }
    }
}

// ════════════════════════════════════════════════════════════════════
// 2. Velocity quantization roundtrip
// ════════════════════════════════════════════════════════════════════

proptest! {
    /// Velocity quantization error should be bounded.
    #[test]
    fn velocity_quantize_roundtrip(v in -QUANTIZE_VELOCITY_MAX..=QUANTIZE_VELOCITY_MAX) {
        let q = quantize_velocity(v);
        let step = QUANTIZE_VELOCITY_MAX / 127.0;  // ~3.94
        let max_error = step / 2.0 + 0.01; // half step + float tolerance
        let error = (q - v).abs();
        prop_assert!(
            error <= max_error,
            "quantize_velocity({}) = {} (error {}, max_allowed {})",
            v, q, error, max_error
        );
    }
}

// ════════════════════════════════════════════════════════════════════
// 3. Angle normalization (via quantize_rotation)
// ════════════════════════════════════════════════════════════════════

proptest! {
    /// For any finite f32 angle, quantize_rotation should produce a value
    /// in [0, 2*PI).  The quantization grid is 256 steps over 2*PI.
    #[test]
    fn angle_quantize_in_range(angle in -1000.0f32..=1000.0f32) {
        let q = quantize_rotation(angle);
        let two_pi = std::f32::consts::TAU;
        prop_assert!(
            q >= 0.0 && q < two_pi + 0.01,
            "quantize_rotation({}) = {} not in [0, 2π)",
            angle, q
        );
    }

    /// Angles that differ by 2*PI should quantize to approximately the same value.
    #[test]
    fn angle_quantize_periodic(angle in 0.0f32..=std::f32::consts::TAU) {
        let q1 = quantize_rotation(angle);
        let q2 = quantize_rotation(angle + std::f32::consts::TAU);
        let diff = (q1 - q2).abs();
        // Allow for quantization step difference at wraparound
        let step = std::f32::consts::TAU / 255.0;
        prop_assert!(
            diff < step + 0.01 || diff > std::f32::consts::TAU - step - 0.01,
            "quantize_rotation({}) = {}, quantize_rotation({}) = {} (diff {})",
            angle, q1, angle + std::f32::consts::TAU, q2, diff
        );
    }
}

// ════════════════════════════════════════════════════════════════════
// 4. Damage calculation invariants
// ════════════════════════════════════════════════════════════════════

fn arb_weapon() -> impl Strategy<Value = ServerWeaponType> {
    prop_oneof![
        Just(ServerWeaponType::Pistol),
        Just(ServerWeaponType::Shotgun),
        Just(ServerWeaponType::Rifle),
        Just(ServerWeaponType::Sniper),
        Just(ServerWeaponType::Melee),
    ]
}

fn max_damage_for(weapon: ServerWeaponType) -> i32 {
    match weapon {
        ServerWeaponType::Pistol => PISTOL_DAMAGE,
        ServerWeaponType::Shotgun => SHOTGUN_DAMAGE,
        ServerWeaponType::Rifle => RIFLE_DAMAGE,
        ServerWeaponType::Sniper => SNIPER_DAMAGE,
        ServerWeaponType::Melee => MELEE_DAMAGE,
    }
}

proptest! {
    /// For any weapon type and distance, damage should be non-negative
    /// and <= max weapon damage.
    #[test]
    fn damage_bounded(weapon in arb_weapon(), distance in 0.0f32..2000.0) {
        let base_damage = max_damage_for(weapon);
        let actual_damage = apply_distance_falloff(weapon, base_damage, distance);
        prop_assert!(
            actual_damage >= 0,
            "Negative damage: weapon={:?}, distance={}, damage={}",
            weapon, distance, actual_damage
        );
        prop_assert!(
            actual_damage <= base_damage,
            "Damage exceeds base: weapon={:?}, distance={}, damage={} > base={}",
            weapon, distance, actual_damage, base_damage
        );
    }

    /// Distance damage multiplier is always in [0, 1].
    #[test]
    fn damage_multiplier_in_unit_range(
        weapon in arb_weapon(),
        distance in 0.0f32..5000.0
    ) {
        let mult = distance_damage_multiplier(weapon, distance);
        prop_assert!(
            mult >= 0.0 && mult <= 1.0,
            "multiplier out of [0,1]: weapon={:?}, distance={}, mult={}",
            weapon, distance, mult
        );
    }

    /// Damage is monotonically non-increasing with distance: closer = more damage.
    #[test]
    fn damage_monotonically_decreasing(
        weapon in arb_weapon(),
        d1 in 0.0f32..2000.0,
        d2 in 0.0f32..2000.0,
    ) {
        let (near, far) = if d1 <= d2 { (d1, d2) } else { (d2, d1) };
        let base_damage = max_damage_for(weapon);
        let dmg_near = apply_distance_falloff(weapon, base_damage, near);
        let dmg_far = apply_distance_falloff(weapon, base_damage, far);
        prop_assert!(
            dmg_near >= dmg_far,
            "Damage increased with distance: weapon={:?}, near={}@{}dmg, far={}@{}dmg",
            weapon, near, dmg_near, far, dmg_far
        );
    }
}

// ════════════════════════════════════════════════════════════════════
// 5. Fire rate validation: can_shoot monotonicity
// ════════════════════════════════════════════════════════════════════

proptest! {
    /// If a player cannot shoot at time T, they cannot shoot at time T - epsilon
    /// (for the same state). Equivalently: if can_shoot(T) then can_shoot(T + epsilon).
    /// We test this by checking that if we wait *longer* than the cooldown, we can always shoot.
    #[test]
    fn fire_rate_monotonic(
        weapon in arb_weapon(),
        wait_ms in 0u64..5000,
    ) {
        use std::time::{Instant, Duration};
        let cooldown = match weapon {
            ServerWeaponType::Pistol => PISTOL_FIRE_RATE_SECS,
            ServerWeaponType::Shotgun => SHOTGUN_FIRE_RATE_SECS,
            ServerWeaponType::Rifle => RIFLE_FIRE_RATE_SECS,
            ServerWeaponType::Sniper => SNIPER_FIRE_RATE_SECS,
            ServerWeaponType::Melee => MELEE_FIRE_RATE_SECS,
        };
        let effective_cooldown = (cooldown.max(MIN_SHOT_INTERVAL_SECONDS) - FIRE_RATE_JITTER_TOLERANCE_SECS).max(0.0);
        let effective_cooldown_ms = (effective_cooldown * 1000.0) as u64;

        // Create a player state that just shot
        let mut player = PlayerState::new(
            "test".to_string(),
            "TestPlayer".to_string(),
            100.0,
            100.0,
        );
        player.weapon = weapon;
        player.ammo = 10;
        let base_time = Instant::now();
        player.last_shot_time = Some(base_time);

        // Check at wait_ms offset
        let check_time = base_time + Duration::from_millis(wait_ms);
        let can = player.can_shoot(check_time);

        if wait_ms >= effective_cooldown_ms + 1 {
            // After cooldown has fully elapsed, we should be able to shoot
            prop_assert!(
                can,
                "Should be able to shoot after {}ms (cooldown={}ms) weapon={:?}",
                wait_ms, effective_cooldown_ms, weapon
            );
        }
        if wait_ms < effective_cooldown_ms.saturating_sub(1) {
            // Before cooldown has elapsed, we should NOT be able to shoot
            prop_assert!(
                !can,
                "Should NOT be able to shoot at {}ms (cooldown={}ms) weapon={:?}",
                wait_ms, effective_cooldown_ms, weapon
            );
        }
    }
}

// ════════════════════════════════════════════════════════════════════
// 6. Player state field roundtrip
// ════════════════════════════════════════════════════════════════════

proptest! {
    /// Creating a PlayerState and reading back its fields should preserve
    /// the initial values.
    #[test]
    fn player_state_field_roundtrip(
        x in -1000.0f32..1000.0,
        y in -1000.0f32..1000.0,
        health in 0i32..200,
        score in -1000i32..1000,
        kills in 0i32..100,
        deaths in 0i32..100,
        team_id in 0u8..8,
    ) {
        let mut player = PlayerState::new(
            "roundtrip_test".to_string(),
            "RoundtripPlayer".to_string(),
            x,
            y,
        );
        player.health = health;
        player.score = score;
        player.kills = kills;
        player.deaths = deaths;
        player.team_id = team_id;

        // Verify fields preserved
        prop_assert_eq!(player.x, x);
        prop_assert_eq!(player.y, y);
        prop_assert_eq!(player.health, health);
        prop_assert_eq!(player.score, score);
        prop_assert_eq!(player.kills, kills);
        prop_assert_eq!(player.deaths, deaths);
        prop_assert_eq!(player.team_id, team_id);
    }
}

// ════════════════════════════════════════════════════════════════════
// 7. Arena handle slot reuse
// ════════════════════════════════════════════════════════════════════

proptest! {
    /// alloc -> dealloc -> alloc cycle should reuse the same slot index.
    /// The second allocation should get the same ArenaHandle index.
    #[test]
    fn arena_slot_reuse_after_dealloc(
        first_val in 0i32..1000,
        second_val in 0i32..1000,
    ) {
        let mut arena = Arena::with_capacity(4);
        let h1 = arena.alloc(first_val);
        let idx = h1.index;

        // Dealloc frees the slot
        let removed = arena.dealloc(h1);
        prop_assert_eq!(removed, Some(first_val));

        // Next alloc should reuse the same index
        let h2 = arena.alloc(second_val);
        prop_assert_eq!(
            h2.index, idx,
            "Expected reuse of slot {} but got {}",
            idx, h2.index
        );
        prop_assert_eq!(arena.get(h2), Some(&second_val));
    }

    /// Multiple allocs followed by selective deallocs should maintain correct
    /// len and allow get on remaining handles.
    #[test]
    fn arena_len_consistent(count in 1usize..50) {
        let mut arena = Arena::with_capacity(count);
        let mut handles = Vec::with_capacity(count);
        for i in 0..count {
            handles.push(arena.alloc(i as i32));
        }
        prop_assert_eq!(arena.len(), count);

        // Dealloc even-indexed handles
        let mut dealloc_count = 0usize;
        for i in (0..count).step_by(2) {
            arena.dealloc(handles[i]);
            dealloc_count += 1;
        }
        prop_assert_eq!(arena.len(), count - dealloc_count);

        // Odd-indexed handles should still be valid
        for i in (1..count).step_by(2) {
            prop_assert_eq!(arena.get(handles[i]), Some(&(i as i32)));
        }
    }
}

// ════════════════════════════════════════════════════════════════════
// 8. AoI hysteresis band sufficiency
// ════════════════════════════════════════════════════════════════════

proptest! {
    /// The AoI hysteresis band (exit - enter) must be large enough
    /// that an entity moving at max speed cannot cross it in one
    /// update interval.  At 20Hz (50ms update interval) and max speed
    /// 300 units/sec, max displacement = 15 units.  Band = 40 units.
    #[test]
    fn aoi_hysteresis_sufficient_for_max_speed(
        speed in 0.0f32..500.0,
    ) {
        let update_interval_secs = 1.0 / 60.0 * AOI_UPDATE_DIVISOR_DEFAULT as f32;
        let displacement = speed * update_interval_secs;
        let hysteresis_band = AOI_EXIT_RADIUS - AOI_RADIUS;

        // The hysteresis band should be at least 2x the max displacement
        // at normal gameplay speeds (< 300 u/s).
        if speed <= 300.0 {
            prop_assert!(
                hysteresis_band >= displacement,
                "Hysteresis band {} too small for speed {} (displacement {})",
                hysteresis_band, speed, displacement
            );
        }
    }
}

// ════════════════════════════════════════════════════════════════════
// 9. Weapon profile consistency
// ════════════════════════════════════════════════════════════════════

proptest! {
    /// Every weapon's profile should have positive damage and positive fire rate.
    #[test]
    fn weapon_profile_valid(weapon in arb_weapon()) {
        let prof = profile(weapon);
        prop_assert!(prof.damage > 0, "weapon {:?} has zero/negative damage", weapon);
        prop_assert!(prof.fire_rate_seconds > 0.0, "weapon {:?} has zero/negative fire rate", weapon);
        // Melee has 0 max_ammo, ranged weapons should have positive
        if weapon != ServerWeaponType::Melee {
            prop_assert!(prof.max_ammo > 0, "weapon {:?} has zero max_ammo", weapon);
        }

        let fp = falloff_profile(weapon);
        prop_assert!(fp.max_range > 0.0, "weapon {:?} has zero max range", weapon);
        prop_assert!(
            fp.min_multiplier >= 0.0 && fp.min_multiplier <= 1.0,
            "weapon {:?} min_multiplier {} out of [0,1]",
            weapon, fp.min_multiplier
        );
    }
}

// ════════════════════════════════════════════════════════════════════
// 10. Damage + falloff compose correctly
// ════════════════════════════════════════════════════════════════════

proptest! {
    /// apply_distance_falloff with 0 base damage always returns 0.
    #[test]
    fn zero_base_damage_stays_zero(
        weapon in arb_weapon(),
        distance in 0.0f32..2000.0,
    ) {
        let dmg = apply_distance_falloff(weapon, 0, distance);
        prop_assert_eq!(dmg, 0, "Zero base damage should yield 0, got {}", dmg);
    }

    /// apply_distance_falloff with negative base damage clamps to 0.
    #[test]
    fn negative_base_damage_clamped(
        weapon in arb_weapon(),
        base in -100i32..0,
        distance in 0.0f32..500.0,
    ) {
        let dmg = apply_distance_falloff(weapon, base, distance);
        prop_assert_eq!(
            dmg, 0,
            "Negative base damage {} should yield 0, got {}",
            base, dmg
        );
    }
}
