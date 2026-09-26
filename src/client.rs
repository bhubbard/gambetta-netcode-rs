use std::collections::VecDeque;
use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::interpolation::EntityInterpolator;
use crate::reconciliation::{reconcile_state, ReconciliationResult};
use crate::server::ServerSnapshot;
use crate::EntityId;

/// Sequence-numbered client input struct.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ClientInput<A = ()> {
    /// Monotonically increasing sequence number.
    pub sequence: u64,
    /// Client timestamp when the input was sampled.
    pub timestamp: f64,
    /// 2D movement vector (typically normalized direction).
    pub movement: Vec2,
    /// Custom game actions (fire, jump, reload, etc.).
    pub actions: A,
    /// Frame delta time associated with this input step.
    pub dt: f32,
}

impl<A> ClientInput<A> {
    pub fn new(sequence: u64, timestamp: f64, movement: Vec2, actions: A, dt: f32) -> Self {
        Self {
            sequence,
            timestamp,
            movement,
            actions,
            dt,
        }
    }
}

/// Circular / ring buffer storing pending, unacknowledged client inputs.
#[derive(Debug, Clone)]
pub struct InputBuffer<A = ()> {
    buffer: VecDeque<ClientInput<A>>,
    max_capacity: usize,
}

impl<A: Clone> InputBuffer<A> {
    pub fn new(max_capacity: usize) -> Self {
        Self {
            buffer: VecDeque::with_capacity(max_capacity),
            max_capacity,
        }
    }

    /// Appends a new sequence-numbered input. Drops oldest if capacity is exceeded.
    pub fn push(&mut self, input: ClientInput<A>) {
        if self.buffer.len() >= self.max_capacity {
            self.buffer.pop_front();
        }
        self.buffer.push_back(input);
    }

    /// Discards all inputs with sequence <= `last_acknowledged_sequence`.
    pub fn discard_acknowledged(&mut self, last_acknowledged_sequence: u64) -> usize {
        let original_len = self.buffer.len();
        self.buffer
            .retain(|input| input.sequence > last_acknowledged_sequence);
        original_len - self.buffer.len()
    }

    /// Returns a slice/iterator of all unacknowledged inputs.
    pub fn unacknowledged(&self) -> impl Iterator<Item = &ClientInput<A>> {
        self.buffer.iter()
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

/// Represents the client-side game entity, responsible for:
/// 1. Generating sequence-numbered inputs.
/// 2. Predicting local position immediately.
/// 3. Buffering unacknowledged inputs.
/// 4. Reconciling with authoritative server updates.
/// 5. Interpolating remote entities.
#[derive(Debug)]
pub struct Client<S, A = ()> {
    pub entity_id: EntityId,
    pub next_sequence: u64,
    pub current_time: f64,
    pub predicted_state: S,
    pub input_buffer: InputBuffer<A>,
    pub rtt: f64,
    pub render_delay: f64,
    pub last_acknowledged_sequence: u64,
    pub remote_interpolators: std::collections::HashMap<EntityId, EntityInterpolator<S>>,
    pub prediction_enabled: bool,
}

impl<S: Clone, A: Clone> Client<S, A> {
    pub fn new(
        entity_id: EntityId,
        initial_state: S,
        buffer_capacity: usize,
        render_delay: f64,
    ) -> Self {
        Self {
            entity_id,
            next_sequence: 1,
            current_time: 0.0,
            predicted_state: initial_state,
            input_buffer: InputBuffer::new(buffer_capacity),
            rtt: 0.050,
            render_delay,
            last_acknowledged_sequence: 0,
            remote_interpolators: std::collections::HashMap::new(),
            prediction_enabled: true,
        }
    }

    /// Submits a movement input for the current frame.
    ///
    /// If client prediction is enabled, immediately applies the input to `predicted_state`.
    /// The input is buffered with an incrementing sequence number until confirmed by the server.
    pub fn sample_and_apply_input<F>(
        &mut self,
        movement: Vec2,
        actions: A,
        dt: f32,
        apply_fn: F,
    ) -> ClientInput<A>
    where
        F: Fn(&mut S, &ClientInput<A>),
    {
        let seq = self.next_sequence;
        self.next_sequence += 1;
        self.current_time += dt as f64;

        let input = ClientInput::new(seq, self.current_time, movement, actions, dt);
        self.input_buffer.push(input.clone());

        if self.prediction_enabled {
            apply_fn(&mut self.predicted_state, &input);
        }

        input
    }

    /// Ingests an authoritative server snapshot and reconciles client prediction.
    ///
    /// - For the local player: drops acknowledged inputs (seq <= srv_seq), snaps to server
    ///   state, and replays all subsequent unacknowledged inputs.
    /// - For remote entities: passes their states to their corresponding `EntityInterpolator`.
    pub fn receive_server_snapshot<F, DistFn>(
        &mut self,
        snapshot: &ServerSnapshot<S>,
        apply_fn: F,
        distance_fn: DistFn,
    ) -> Option<ReconciliationResult<S>>
    where
        F: Fn(&mut S, &ClientInput<A>),
        DistFn: Fn(&S, &S) -> f32,
        S: crate::interpolation::Interpolatable,
    {
        let mut local_result = None;

        // Process local player reconciliation
        if let (Some(&server_seq), Some(server_state)) = (
            snapshot.last_processed_inputs.get(&self.entity_id),
            snapshot.entities.get(&self.entity_id),
        ) && server_seq >= self.last_acknowledged_sequence
        {
            self.last_acknowledged_sequence = server_seq;

            if self.prediction_enabled {
                let res = reconcile_state(
                    &mut self.predicted_state,
                    server_state,
                    server_seq,
                    &mut self.input_buffer,
                    &apply_fn,
                    &distance_fn,
                );
                local_result = Some(res);
            } else {
                // Without prediction, client simply snaps directly to server state
                self.predicted_state = server_state.clone();
                self.input_buffer.discard_acknowledged(server_seq);
            }
        }

        // Process remote entities interpolation
        for (&id, remote_state) in &snapshot.entities {
            if id != self.entity_id {
                let interpolator = self
                    .remote_interpolators
                    .entry(id)
                    .or_insert_with(|| EntityInterpolator::new(self.render_delay));
                interpolator.add_snapshot(snapshot.timestamp, remote_state.clone(), None);
            }
        }

        local_result
    }

    /// Computes the exact historical shot timestamp for server lag compensation:
    /// $T_{shot} = T_{client\_now} - RTT/2 - \text{render\_delay}$.
    pub fn calculate_shot_time(&self) -> f64 {
        (self.current_time - (self.rtt * 0.5) - self.render_delay).max(0.0)
    }

    /// Samples a remote entity at the current render time.
    pub fn sample_remote_entity(&self, entity_id: EntityId) -> Option<S>
    where
        S: crate::interpolation::Interpolatable,
    {
        self.remote_interpolators
            .get(&entity_id)
            .and_then(|interp| interp.sample(self.current_time))
    }
}
