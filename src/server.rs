use std::collections::{HashMap, VecDeque};
use serde::{Deserialize, Serialize};

use crate::client::ClientInput;
use crate::interpolation::Interpolatable;
use crate::lag_compensation::{Collider2d, LagCompensationHistory, Ray2d, RayHit};
use crate::{EntityId, Result};

/// Authoritative snapshot sent to clients across the network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSnapshot<S> {
    pub tick: u64,
    pub timestamp: f64,
    pub last_processed_inputs: HashMap<EntityId, u64>,
    pub entities: HashMap<EntityId, S>,
}

/// Authoritative game server simulating client inputs, maintaining world state,
/// and providing lag compensation rewind buffers.
#[derive(Debug)]
pub struct Server<S, A = ()> {
    pub current_tick: u64,
    pub current_time: f64,
    pub tick_rate: f64,
    pub entities: HashMap<EntityId, S>,
    pub client_input_queues: HashMap<EntityId, VecDeque<ClientInput<A>>>,
    pub last_processed_sequences: HashMap<EntityId, u64>,
    pub lag_compensation: LagCompensationHistory<S>,
}

impl<S: Interpolatable, A: Clone> Server<S, A> {
    pub fn new(tick_rate: f64, lag_history_duration: f64) -> Self {
        Self {
            current_tick: 0,
            current_time: 0.0,
            tick_rate,
            entities: HashMap::new(),
            client_input_queues: HashMap::new(),
            last_processed_sequences: HashMap::new(),
            lag_compensation: LagCompensationHistory::new(lag_history_duration),
        }
    }

    /// Registers a new entity into the authoritative simulation.
    pub fn add_entity(&mut self, id: EntityId, initial_state: S) {
        self.entities.insert(id, initial_state);
        self.client_input_queues.insert(id, VecDeque::new());
        self.last_processed_sequences.insert(id, 0);
    }

    /// Removes an entity from the server simulation.
    pub fn remove_entity(&mut self, id: EntityId) -> Option<S> {
        self.client_input_queues.remove(&id);
        self.last_processed_sequences.remove(&id);
        self.entities.remove(&id)
    }

    /// Enqueues a client input packet received over the network.
    pub fn queue_client_input(&mut self, id: EntityId, input: ClientInput<A>) {
        if let Some(queue) = self.client_input_queues.get_mut(&id) {
            queue.push_back(input);
        }
    }

    /// Advances the server simulation by one tick:
    /// 1. Drains and processes all pending inputs for each client.
    /// 2. Applies state updates via `apply_fn`.
    /// 3. Updates `last_processed_sequences`.
    /// 4. Records entity hitboxes into `lag_compensation` history.
    pub fn tick<F, C>(&mut self, dt: f32, mut apply_fn: F, mut collider_fn: C)
    where
        F: FnMut(&mut S, &ClientInput<A>),
        C: FnMut(&S) -> Collider2d,
    {
        self.current_tick += 1;
        self.current_time += dt as f64;

        // Process inputs for all connected clients
        for (&entity_id, queue) in self.client_input_queues.iter_mut() {
            if let Some(state) = self.entities.get_mut(&entity_id) {
                while let Some(input) = queue.pop_front() {
                    apply_fn(state, &input);
                    self.last_processed_sequences.insert(entity_id, input.sequence);
                }
            }
        }

        // Record current frame into lag compensation history
        let mut colliders = HashMap::with_capacity(self.entities.len());
        for (&id, state) in &self.entities {
            colliders.insert(id, (state.clone(), collider_fn(state)));
        }
        self.lag_compensation.record_frame(self.current_time, colliders);
    }

    /// Builds a serializable snapshot of the current authoritative game state.
    pub fn create_snapshot(&self) -> ServerSnapshot<S> {
        ServerSnapshot {
            tick: self.current_tick,
            timestamp: self.current_time,
            last_processed_inputs: self.last_processed_sequences.clone(),
            entities: self.entities.clone(),
        }
    }

    /// Performs lag-compensated hit detection when a player fires a hitscan weapon.
    ///
    /// The shot time should be calculated by the client as:
    /// $T_{shot} = T_{client\_now} - RTT/2 - \text{render\_delay}$.
    pub fn handle_shot(
        &self,
        shooter_id: EntityId,
        shot_time: f64,
        ray: &Ray2d,
    ) -> Result<Option<RayHit>> {
        self.lag_compensation
            .rewind_and_raycast(shot_time, shooter_id, ray)
    }
}
