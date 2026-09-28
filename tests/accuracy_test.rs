//! Rigorous Netcode Determinism & Lag Compensation Parity Benchmark Tests
//! Evaluates client reconciliation under packet drop, Hermite C1 continuity, and lag rewind precision.

use std::collections::HashMap;
use gambetta_netcode::client::Client;
use gambetta_netcode::interpolation::{EntityInterpolator, InterpolationMode};
use gambetta_netcode::lag_compensation::{Collider2d, LagCompensationHistory, Ray2d};
use gambetta_netcode::server::Server;
use gambetta_netcode::PlayerState2d;
use glam::Vec2;

fn distance_2d(a: &PlayerState2d, b: &PlayerState2d) -> f32 {
    a.position.distance(b.position)
}

#[test]
fn test_reconciliation_exact_determinism_under_delayed_snapshots() {
    let speed = 150.0f32;
    let dt = 1.0 / 60.0f32;

    let movement_apply = move |state: &mut PlayerState2d, input: &gambetta_netcode::client::ClientInput<()>| {
        state.apply_movement(input.movement, speed, input.dt);
    };

    let mut client = Client::new(1, PlayerState2d::new(1, Vec2::ZERO), 256, 0.100);
    let mut server: Server<PlayerState2d> = Server::new(60.0, 1.0);
    server.add_entity(1, PlayerState2d::new(1, Vec2::ZERO));

    // Send 30 inputs
    for seq in 1..=30 {
        let dir = if seq % 2 == 0 { Vec2::X } else { Vec2::Y };
        let input = client.sample_and_apply_input(dir, (), dt, movement_apply);
        server.queue_client_input(1, input);

        // Every 5 inputs, tick server and send snapshot back to client
        if seq % 5 == 0 {
            server.tick(
                dt * 5.0,
                movement_apply,
                |state| Collider2d::circle(state.position, 16.0),
            );
            let snapshot = server.create_snapshot();

            // Client reconciles against server snapshot
            let _ = client.receive_server_snapshot(&snapshot, movement_apply, distance_2d);
        }
    }

    // Flush any remaining server inputs
    server.tick(
        dt,
        movement_apply,
        |state| Collider2d::circle(state.position, 16.0),
    );
    let final_snapshot = server.create_snapshot();
    let _ = client.receive_server_snapshot(&final_snapshot, movement_apply, distance_2d);

    // Final predicted position MUST match server position exactly
    let server_pos = final_snapshot.entities.get(&1).unwrap().position;
    let client_pos = client.predicted_state.position;
    let diff = client_pos.distance(server_pos);

    assert!(
        diff < 1e-4,
        "Reconciliation diverged from server ground truth: diff={diff}, client={client_pos}, server={server_pos}"
    );
}

#[test]
fn test_hermite_cubic_interpolation_accuracy() {
    let mut interpolator = EntityInterpolator::new(0.100);
    interpolator.set_mode(InterpolationMode::HermiteCubic);

    // Add snapshots at t=0.0 and t=1.0 with velocities
    let p0 = Vec2::new(0.0, 0.0);
    let v0 = Vec2::new(100.0, 0.0);
    let p1 = Vec2::new(100.0, 100.0);
    let v1 = Vec2::new(0.0, 100.0);

    interpolator.add_snapshot(0.0, PlayerState2d::new(2, p0), Some(v0));
    interpolator.add_snapshot(1.0, PlayerState2d::new(2, p1), Some(v1));

    // Midpoint check at render_time = 0.5s (curr_time = 0.6s with 0.1s render delay)
    let sample = interpolator.sample(0.600).expect("Sample must succeed");

    // Analytical Hermite cubic midpoint:
    // H(0.5) = (p0 + p1)/2 + (v0 - v1)/8 = (50, 50) + (100, -100)/8 = (62.5, 37.5)
    let expected_x = 62.5f32;
    let expected_y = 37.5f32;

    assert!(
        (sample.position.x - expected_x).abs() < 1e-3,
        "Hermite x error: got {}, expected {}",
        sample.position.x,
        expected_x
    );
    assert!(
        (sample.position.y - expected_y).abs() < 1e-3,
        "Hermite y error: got {}, expected {}",
        sample.position.y,
        expected_y
    );
}

#[test]
fn test_lag_compensation_rewind_precision() {
    let mut history: LagCompensationHistory<PlayerState2d> = LagCompensationHistory::new(1.0);

    // Target player (ID 2) moves along X at 100 m/s
    for i in 0..=10 {
        let t = (i as f64) * 0.1;
        let pos = Vec2::new((t as f32) * 100.0, 0.0);
        let mut frame = HashMap::new();
        frame.insert(2, (PlayerState2d::new(2, pos), Collider2d::circle(pos, 10.0)));
        history.record_frame(t, frame);
    }

    // Shooter fires at rewound time t=0.45s from (45.0, -50.0) along +Y
    let shot_time = 0.45f64;
    let ray = Ray2d::new(Vec2::new(45.0, -50.0), Vec2::Y, 100.0);

    let hit = history.rewind_and_raycast(shot_time, 1, &ray).unwrap().expect("Must hit target entity 2");
    assert_eq!(hit.entity_id, 2);

    // Expected position at t=0.45s is (45.0, 0.0). Radius 10.0 => hit distance is 50.0 - 10.0 = 40.0
    assert!(
        (hit.distance - 40.0).abs() < 0.1,
        "Hit distance mismatch: got {}, expected 40.0",
        hit.distance
    );
}
