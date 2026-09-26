use gambetta_netcode::interpolation::{EntityInterpolator, InterpolationMode};
use gambetta_netcode::PlayerState2d;
use glam::Vec2;

#[test]
fn test_linear_interpolation() {
    let mut interpolator = EntityInterpolator::new(0.100);

    // Add snapshots at t=0.0 and t=0.2
    interpolator.add_snapshot(0.0, PlayerState2d::new(2, Vec2::new(0.0, 0.0)), None);
    interpolator.add_snapshot(0.2, PlayerState2d::new(2, Vec2::new(100.0, 50.0)), None);

    // Render delay is 0.100s.
    // If client current_time = 0.200s, render_time = 0.200 - 0.100 = 0.100s.
    // Exact midpoint (alpha = 0.5)
    let sample = interpolator.sample(0.200).expect("Sample should succeed");
    assert!(
        (sample.position.x - 50.0).abs() < 1e-3,
        "Expected x = 50.0, got {}",
        sample.position.x
    );
    assert!(
        (sample.position.y - 25.0).abs() < 1e-3,
        "Expected y = 25.0, got {}",
        sample.position.y
    );
}

#[test]
fn test_hermite_cubic_interpolation() {
    let mut interpolator = EntityInterpolator::new(0.100);
    interpolator.set_mode(InterpolationMode::HermiteCubic);

    // Snapshot at t=0.0 with velocity (100, 0)
    interpolator.add_snapshot(
        0.0,
        PlayerState2d::new(2, Vec2::new(0.0, 0.0)),
        Some(Vec2::new(100.0, 0.0)),
    );
    // Snapshot at t=1.0 with velocity (0, 100)
    interpolator.add_snapshot(
        1.0,
        PlayerState2d::new(2, Vec2::new(100.0, 100.0)),
        Some(Vec2::new(0.0, 100.0)),
    );

    // Render at t=0.5 (current_time = 0.600)
    let sample = interpolator.sample(0.600).expect("Sample should succeed");
    // Hermite curve should provide smooth non-linear interpolation
    assert!(sample.position.x > 0.0 && sample.position.x < 100.0);
    assert!(sample.position.y > 0.0 && sample.position.y < 100.0);
}

#[test]
fn test_interpolation_jitter_extrapolation() {
    let mut interpolator = EntityInterpolator::new(0.050);
    interpolator.add_snapshot(0.0, PlayerState2d::new(2, Vec2::new(0.0, 0.0)), None);
    interpolator.add_snapshot(0.1, PlayerState2d::new(2, Vec2::new(10.0, 0.0)), None);

    // Jitter delay: current_time = 0.170, render_time = 0.120 (past 0.1 by 0.020s, within 0.050 max extrapolation)
    let sample = interpolator.sample(0.170).expect("Extrapolation should succeed");
    assert!(
        sample.position.x > 10.0,
        "Expected extrapolated position > 10.0, got {}",
        sample.position.x
    );
}
