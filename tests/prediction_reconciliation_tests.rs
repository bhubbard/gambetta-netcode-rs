use gambetta_netcode::client::Client;
use gambetta_netcode::server::Server;
use gambetta_netcode::PlayerState2d;
use glam::Vec2;

fn distance_2d(a: &PlayerState2d, b: &PlayerState2d) -> f32 {
    a.position.distance(b.position)
}

#[test]
fn test_client_prediction_and_reconciliation() {
    let speed = 200.0;
    let dt = 1.0 / 60.0;

    let movement_apply = move |state: &mut PlayerState2d, input: &gambetta_netcode::client::ClientInput<()>| {
        state.apply_movement(input.movement, speed, input.dt);
    };

    let mut client = Client::new(1, PlayerState2d::new(1, Vec2::new(10.0, 10.0)), 128, 0.100);
    let mut server: Server<PlayerState2d> = Server::new(60.0, 1.0);
    server.add_entity(1, PlayerState2d::new(1, Vec2::new(10.0, 10.0)));

    // Client generates 5 inputs moving Right (Vec2::X)
    let mut sent_inputs = Vec::new();
    for _ in 0..5 {
        let input = client.sample_and_apply_input(Vec2::X, (), dt, movement_apply);
        sent_inputs.push(input);
    }

    // Client predicted position should have advanced by 5 * (speed * dt)
    let expected_client_x = 10.0 + 5.0 * (speed * dt);
    assert!(
        (client.predicted_state.position.x - expected_client_x).abs() < 1e-4,
        "Client prediction failed to advance position"
    );
    assert_eq!(client.input_buffer.len(), 5);

    // Server receives and processes first 3 inputs
    for input in sent_inputs.into_iter().take(3) {
        server.queue_client_input(1, input);
    }

    server.tick(
        dt,
        movement_apply,
        |state| gambetta_netcode::lag_compensation::Collider2d::circle(state.position, 16.0),
    );

    // Server snapshot after 3 inputs
    let snapshot = server.create_snapshot();
    assert_eq!(*snapshot.last_processed_inputs.get(&1).unwrap(), 3);

    let server_pos_x = snapshot.entities.get(&1).unwrap().position.x;
    let expected_server_x = 10.0 + 3.0 * (speed * dt);
    assert!(
        (server_pos_x - expected_server_x).abs() < 1e-4,
        "Server state mismatch after 3 inputs"
    );

    // Client receives snapshot acknowledging seq 3
    let recon_result = client
        .receive_server_snapshot(&snapshot, movement_apply, distance_2d)
        .expect("Expected reconciliation result");

    assert_eq!(recon_result.discarded_inputs, 3);
    assert_eq!(recon_result.reapplied_inputs, 2);
    assert_eq!(client.input_buffer.len(), 2);
    // Predicted position should still match the full 5 steps!
    assert!(
        (client.predicted_state.position.x - expected_client_x).abs() < 1e-4,
        "Reconciliation diverged from valid prediction"
    );
    assert!(
        recon_result.prediction_error < 1e-4,
        "Prediction error should be zero without external disturbance"
    );
}

#[test]
fn test_reconciliation_server_correction() {
    let speed = 100.0;
    let dt = 1.0 / 60.0;

    let movement_apply = move |state: &mut PlayerState2d, input: &gambetta_netcode::client::ClientInput<()>| {
        state.apply_movement(input.movement, speed, input.dt);
    };

    let mut client = Client::new(1, PlayerState2d::new(1, Vec2::ZERO), 128, 0.100);
    let mut server: Server<PlayerState2d> = Server::new(60.0, 1.0);
    server.add_entity(1, PlayerState2d::new(1, Vec2::ZERO));

    // Client moves Right for 3 inputs
    let mut inputs = Vec::new();
    for _ in 0..3 {
        inputs.push(client.sample_and_apply_input(Vec2::X, (), dt, movement_apply));
    }

    // Client thinks it is at (3 * speed * dt, 0)
    assert!(client.predicted_state.position.x > 0.0);

    // Server modifies state (e.g. pushed back by explosion or wall)
    for input in inputs {
        server.queue_client_input(1, input);
    }
    server.tick(
        dt,
        |state, input| {
            // Apply input but clamp at x = 1.0 (wall collision)
            state.apply_movement(input.movement, speed, input.dt);
            if state.position.x > 1.0 {
                state.position.x = 1.0;
            }
        },
        |state| gambetta_netcode::lag_compensation::Collider2d::circle(state.position, 16.0),
    );

    let snapshot = server.create_snapshot();
    let recon_result = client
        .receive_server_snapshot(&snapshot, movement_apply, distance_2d)
        .expect("Expected reconciliation");

    // Prediction error should be detected because server clamped at x = 1.0
    assert!(recon_result.prediction_error > 0.0);
    assert_eq!(recon_result.discarded_inputs, 3);
    assert_eq!(recon_result.reapplied_inputs, 0);
    assert_eq!(client.predicted_state.position.x, 1.0);
}
