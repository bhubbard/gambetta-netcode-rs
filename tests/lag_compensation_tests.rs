use std::collections::HashMap;
use gambetta_netcode::lag_compensation::{Collider2d, LagCompensationHistory, Ray2d};
use gambetta_netcode::PlayerState2d;
use glam::Vec2;

#[test]
fn test_lag_compensation_rewind_hitscan() {
    let mut history: LagCompensationHistory<PlayerState2d> = LagCompensationHistory::new(1.0);

    // Target player (ID 2) moves across the screen:
    // At t=0.100s, at position (50, 0)
    // At t=0.200s, at position (100, 0)
    // At t=0.300s, at position (150, 0)

    let mut f1 = HashMap::new();
    f1.insert(
        2,
        (
            PlayerState2d::new(2, Vec2::new(50.0, 0.0)),
            Collider2d::circle(Vec2::new(50.0, 0.0), 10.0),
        ),
    );
    history.record_frame(0.100, f1);

    let mut f2 = HashMap::new();
    f2.insert(
        2,
        (
            PlayerState2d::new(2, Vec2::new(100.0, 0.0)),
            Collider2d::circle(Vec2::new(100.0, 0.0), 10.0),
        ),
    );
    history.record_frame(0.200, f2);

    let mut f3 = HashMap::new();
    f3.insert(
        2,
        (
            PlayerState2d::new(2, Vec2::new(150.0, 0.0)),
            Collider2d::circle(Vec2::new(150.0, 0.0), 10.0),
        ),
    );
    history.record_frame(0.300, f3);

    // At current time t=0.300s, target is at x=150.
    // Client 1 aims at x=100 because client was seeing the target with 100ms render delay (t=0.200s).
    // Client fires ray from (100, -50) upwards toward (100, 0).
    let ray = Ray2d::new(Vec2::new(100.0, -50.0), Vec2::Y, 100.0);

    // If server checked current time (t=0.300s), target is at x=150 and ray misses!
    let live_hit = history.rewind_and_raycast(0.300, 1, &ray).unwrap();
    assert!(
        live_hit.is_none(),
        "Without lag compensation, shot at x=100 should miss target currently at x=150"
    );

    // With lag compensation rewound to t=0.200s:
    let compensated_hit = history.rewind_and_raycast(0.200, 1, &ray).unwrap();
    assert!(
        compensated_hit.is_some(),
        "Lag compensation should register hit at t=0.200s"
    );
    let hit = compensated_hit.unwrap();
    assert_eq!(hit.entity_id, 2);
    // Hit distance from y = -50 to target edge (center at y=0, radius 10 => hit point y = -10, dist = 40)
    assert!((hit.distance - 40.0).abs() < 1e-3);
}

#[test]
fn test_lag_compensation_shooter_exclusion() {
    let mut history: LagCompensationHistory<PlayerState2d> = LagCompensationHistory::new(1.0);

    let mut frame = HashMap::new();
    // Shooter (ID 1) at (0, 0)
    frame.insert(
        1,
        (
            PlayerState2d::new(1, Vec2::ZERO),
            Collider2d::circle(Vec2::ZERO, 10.0),
        ),
    );
    history.record_frame(0.100, frame);

    // Ray originating from inside shooter's collider
    let ray = Ray2d::new(Vec2::ZERO, Vec2::X, 100.0);

    // Shooter should not shoot themselves
    let hit = history.rewind_and_raycast(0.100, 1, &ray).unwrap();
    assert!(hit.is_none(), "Shooter should be excluded from hit detection");
}
