use std::time::Instant;
use std::collections::HashMap;
use glam::Vec2;
use gambetta_netcode::client::Client;
use gambetta_netcode::interpolation::Interpolatable;
use gambetta_netcode::lag_compensation::{Collider2d, LagCompensationHistory, Ray2d};
use gambetta_netcode::server::ServerSnapshot;
use gambetta_netcode::PlayerState2d;

fn distance_2d(a: &PlayerState2d, b: &PlayerState2d) -> f32 {
    a.position.distance(b.position)
}

fn main() {
    println!("============================================================");
    println!("  gambetta-netcode-rs (Rust) vs JavaScript Reference Bench  ");
    println!("============================================================");

    let speed = 250.0;
    let dt = 1.0 / 60.0;
    let movement_apply = move |state: &mut PlayerState2d, input: &gambetta_netcode::client::ClientInput<()>| {
        state.apply_movement(input.movement, speed, input.dt);
    };

    // 1. Client-Side Prediction & History Replay Reconciliation
    println!("\n--- 1. Client Prediction & Ring-Buffer Reconciliation ---");
    {
        let mut client = Client::new(1, PlayerState2d::new(1, Vec2::new(10.0, 10.0)), 128, 0.100);

        // Pre-fill input buffer with 32 unacknowledged inputs
        for _ in 0..32 {
            client.sample_and_apply_input(Vec2::X, (), dt, movement_apply);
        }

        let mut snapshot = ServerSnapshot {
            tick: 10,
            timestamp: 0.166,
            last_processed_inputs: HashMap::new(),
            entities: HashMap::new(),
        };

        let iterations = 1_000_000;
        let start = Instant::now();

        for seq in 0..iterations {
            let ack_seq = 1 + (seq % 16) as u64;
            snapshot.last_processed_inputs.insert(1, ack_seq);
            snapshot.entities.insert(1, PlayerState2d::new(1, Vec2::new(10.0 + ack_seq as f32 * 4.0, 10.0)));
            let _ = client.receive_server_snapshot(&snapshot, movement_apply, distance_2d);
        }

        let elapsed = start.elapsed();
        let ns_per_rec = elapsed.as_nanos() as f64 / iterations as f64;
        let recs_per_sec = iterations as f64 / elapsed.as_secs_f64();

        println!(
            "Reconciliations: {} | Time: {:.2?} | Latency: {:.2} ns/reconciliation | {:>10.0} recs/s",
            iterations, elapsed, ns_per_rec, recs_per_sec
        );
    }

    // 2. Multi-Entity Hermite Cubic Spline Interpolation
    println!("\n--- 2. Hermite Cubic Spline Entity Interpolation (100, 500, 1000 Remote Players) ---");
    for &entity_count in &[100, 500, 1000] {
        let p0 = PlayerState2d {
            id: 1,
            position: Vec2::new(0.0, 0.0),
            velocity: Vec2::new(200.0, 0.0),
            orientation: 0.0,
            health: 100.0,
        };
        let p1 = PlayerState2d {
            id: 1,
            position: Vec2::new(200.0 * dt, 0.0),
            velocity: Vec2::new(200.0, 50.0),
            orientation: 0.1,
            health: 95.0,
        };

        let frames = 10_000;
        let start = Instant::now();
        let mut sum_pos = Vec2::ZERO;

        for f in 0..frames {
            let alpha = (f % 100) as f32 * 0.01;
            for _ in 0..entity_count {
                let interpolated = p0.hermite(&p1, None, None, dt, alpha);
                sum_pos += interpolated.position;
            }
        }

        let elapsed = start.elapsed();
        let frame_latency = elapsed / frames as u32;
        let entities_per_sec = (frames * entity_count) as f64 / elapsed.as_secs_f64();

        println!(
            "Entities: {:>4} | Frame Time: {:>8.2?} | {:>10.0} entities/sec | SumPos: {:.1?}",
            entity_count, frame_latency, entities_per_sec, sum_pos
        );
    }

    // 3. Server Authoritative Lag Compensation & Hitbox Rewind
    println!("\n--- 3. Server Lag Compensation & Historical Rewind Hitbox Check ---");
    {
        let mut history: LagCompensationHistory<PlayerState2d> = LagCompensationHistory::new(1.0);

        // Record 60 history frames (1 second of 60 Hz physics)
        for i in 0..60 {
            let t = i as f64 * (1.0 / 60.0);
            let pos = Vec2::new(i as f32 * 2.0, 0.0);
            let mut entities = HashMap::new();
            entities.insert(1, (PlayerState2d::new(1, pos), Collider2d::circle(pos, 16.0)));
            entities.insert(2, (PlayerState2d::new(2, pos + Vec2::new(50.0, 0.0)), Collider2d::circle(pos + Vec2::new(50.0, 0.0), 16.0)));
            history.record_frame(t, entities);
        }

        let iterations = 1_000_000;
        let start = Instant::now();
        let mut hits = 0;

        for i in 0..iterations {
            let latency_s = 0.05 + (i % 20) as f64 * 0.01; // 50ms - 250ms simulated ping
            let rewind_time = 0.95 - latency_s;
            let ray = Ray2d::new(Vec2::new(40.0, 0.0), Vec2::X, 100.0);

            if let Ok(Some(_hit)) = history.rewind_and_raycast(rewind_time, 999, &ray) {
                hits += 1;
            }
        }

        let elapsed = start.elapsed();
        let ns_per_ray = elapsed.as_nanos() as f64 / iterations as f64;
        let rays_per_sec = iterations as f64 / elapsed.as_secs_f64();

        println!(
            "Rewind Raycasts: {} | Time: {:.2?} | Latency: {:.2} ns/raycast | {:>10.0} raycasts/s | Hits: {}",
            iterations, elapsed, ns_per_ray, rays_per_sec, hits
        );
    }

    println!("\n============================================================");
    println!("                      Benchmark Complete                    ");
    println!("============================================================");
}
