# ⚡ gambetta-netcode-rs

[![GitHub Pages](https://img.shields.io/badge/Live%20Demo-GitHub%20Pages-brightgreen?style=for-the-badge&logo=github)](https://bhubbard.github.io/gambetta-netcode-rs/)
[![Rust Edition 2024](https://img.shields.io/badge/Rust-2024%20Edition-orange?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue?style=for-the-badge)](LICENSE)
[![Tests](https://img.shields.io/badge/Tests-Passing-success?style=for-the-badge&logo=githubactions)](tests/)

> **Pure Rust client-side prediction, server reconciliation, remote entity interpolation, and lag compensation netcode.**
> Faithfully based on Gabriel Gambetta’s iconic *Fast-Paced Multiplayer* architecture series.

---

## 🎮 [Launch Live Interactive Sandbox](https://bhubbard.github.io/gambetta-netcode-rs/)

Experience the netcode live in your browser:
- **Tri-View Visualization**: Real-time side-by-side rendering of Client 1 (predicted), Authoritative Server, and Client 2 (remote bot).
- **Latency & Packet Jitter**: Dial up latency from 0ms to 400ms (800ms RTT) and packet drop rate from 0% to 30%.
- **Live Reconciliation Nudge**: Inject authoritative server disturbances to watch client prediction discard stale inputs ($\le S_{srv}$) and smoothly replay unacknowledged actions ($> S_{srv}$).
- **Lag Compensation Hitbox Rewind**: Fire hitscan rays at moving remote targets; watch the server rewind hitboxes to $T_{shot} = T_{now} - \text{RTT}/2 - \text{render\_delay}$.

---

## 🏗 Architecture & The Four Pillars

```
+-------------------------------------------------------------------------+
|                              CLIENT 1                                   |
|                                                                         |
|  [Input Sample] ---> (InputBuffer) ---> [Client Prediction State]       |
|         |                                        ^                      |
|         v (UDP / WebRTC)                         | (Reconcile & Replay) |
|         |                                        |                      |
+---------|----------------------------------------|----------------------+
          |                                        |
          | ClientInput { seq, dt, move }          | ServerSnapshot
          v                                        | { tick, last_seq, entities }
+--------------------------------------------------|----------------------+
|                         AUTHORITATIVE SERVER                            |
|                                                                         |
|  [Input Queues] ---> [Authoritative Physics Tick] ---> [Snapshot Broadcast]
|                                |                                        |
|                                v                                        |
|                     (Lag Compensation Buffer)                           |
|                     - 1.0s Rolling Hitbox History                       |
|                     - Rewind & Raycast at T_shot                        |
+-------------------------------------------------------------------------+
                                 |
                                 | ServerSnapshot
                                 v
+-------------------------------------------------------------------------+
|                              CLIENT 2                                   |
|                                                                         |
|  [SnapshotBuffer] ---> [Entity Interpolator (100ms delay)]              |
|                        - Linear Lerp or Hermite Cubic Spline            |
+-------------------------------------------------------------------------+
```

### 1. Client-Side Prediction (`src/client.rs`)
Eliminates perceived input lag for the local player:
- Samples local inputs with a monotonic sequence counter (`seq`).
- Stores inputs in a circular `InputBuffer`.
- Immediately applies movement to `predicted_state` locally.

### 2. Authoritative Server Simulation & Reconciliation (`src/server.rs` & `src/reconciliation.rs`)
Preserves server authority and anti-cheat guarantees:
- Server steps physics at a fixed tick rate (e.g. 60Hz), processing queued inputs strictly in sequence.
- Broadcasts authoritative snapshots containing the latest state and `last_processed_sequence`.
- Upon receiving a snapshot, the client:
  1. Discards acknowledged inputs ($\text{seq} \le S_{srv}$).
  2. Snaps local predicted state to the server's authoritative state.
  3. Replays all remaining unacknowledged inputs ($\text{seq} > S_{srv}$) through the deterministic movement function.

### 3. Remote Entity Interpolation (`src/interpolation.rs`)
Guarantees smooth, jitter-free rendering of remote players without teleportation:
- Buffers timestamped remote entity snapshots.
- Renders remote players with a fixed render delay (e.g. 100ms behind real-time).
- Provides both **Linear Interpolation** and **Hermite Cubic Spline Interpolation** using positions and velocities:
  $$p(t) = h_{00}(\alpha) p_0 + h_{10}(\alpha) \Delta t \cdot v_0 + h_{01}(\alpha) p_1 + h_{11}(\alpha) \Delta t \cdot v_1$$

### 4. Lag Compensation / Server Hitbox Rewind (`src/lag_compensation.rs`)
Restores fair hit registration for high-ping shooters ("what you see is what you hit"):
- Server stores rolling timestamped positions and colliders (`Circle2d`, `Aabb2d`) over the past 1.0 second.
- When a client fires, it calculates its exact observation timestamp:
  $$T_{shot} = T_{\text{client\_now}} - \frac{\text{RTT}}{2} - \text{render\_delay}$$
- Server rewinds all player hitboxes to $T_{shot}$ via linear collider interpolation, performs raycasting against rewound colliders (excluding the shooter), and immediately returns the result without altering the live server state.

---

## 📦 Installation

Add `gambetta-netcode` to your `Cargo.toml`:

```toml
[dependencies]
gambetta-netcode = "0.1.0"
glam = { version = "0.29", features = ["serde"] }
```

---

## 🚀 Quick Start Example

```rust
use gambetta_netcode::client::Client;
use gambetta_netcode::server::Server;
use gambetta_netcode::lag_compensation::{Collider2d, Ray2d};
use gambetta_netcode::PlayerState2d;
use glam::Vec2;

fn main() {
    let dt = 1.0 / 60.0;
    let speed = 250.0;

    // Deterministic state transition function
    let apply_movement = |state: &mut PlayerState2d, input: &gambetta_netcode::client::ClientInput<()>| {
        state.apply_movement(input.movement, speed, input.dt);
    };

    // Initialize Server and Client
    let mut server = Server::new(60.0, 1.0);
    server.add_entity(1, PlayerState2d::new(1, Vec2::ZERO));

    let mut client = Client::new(1, PlayerState2d::new(1, Vec2::ZERO), 128, 0.100);

    // 1. Client applies predicted local input
    let input = client.sample_and_apply_input(Vec2::X, (), dt, apply_movement);
    println!("Client predicted position: {:?}", client.predicted_state.position);

    // 2. Server receives input and steps simulation
    server.queue_client_input(1, input);
    server.tick(dt, apply_movement, |s| Collider2d::circle(s.position, 16.0));

    // 3. Client receives server snapshot and reconciles
    let snapshot = server.create_snapshot();
    let distance_fn = |a: &PlayerState2d, b: &PlayerState2d| a.position.distance(b.position);
    let recon = client.receive_server_snapshot(&snapshot, apply_movement, distance_fn);

    println!("Reconciliation diagnostics: {:?}", recon);

    // 4. Server Lag-Compensated Raycast
    let shot_time = client.calculate_shot_time();
    let ray = Ray2d::new(Vec2::new(0.0, -50.0), Vec2::Y, 100.0);
    let hit = server.handle_shot(1, shot_time, &ray).unwrap();
    println!("Hit result: {:?}", hit);
}
```

---

## 🧪 Running Tests

```bash
cargo test
```

Includes test suites verifying:
- Client-side prediction and unacknowledged input buffering.
- Server reconciliation loop, input pruning, and recovery from authoritative position overrides.
- Linear and Hermite cubic spline snapshot interpolation with jitter extrapolation.
- Lag compensation hitbox rewind raycasting, past frame reconstruction, and shooter exclusion.

---

## 📄 License

Licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
at your option.
