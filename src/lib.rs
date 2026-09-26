use glam::Vec2;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod client;
pub mod interpolation;
pub mod lag_compensation;
pub mod reconciliation;
pub mod server;

/// Unique entity identifier.
pub type EntityId = u64;

/// Result type alias for gambetta-netcode operations.
pub type Result<T> = std::result::Result<T, NetcodeError>;

#[derive(Debug, Error, PartialEq)]
pub enum NetcodeError {
    #[error("Entity {0} was not found")]
    EntityNotFound(EntityId),

    #[error("Insufficient snapshot history for interpolation: available {available}, required {required}")]
    InsufficientSnapshots { available: usize, required: usize },

    #[error("Rewind time {requested:.4}s is outside available history window [{oldest:.4}s, {newest:.4}s]")]
    RewindTimeOutOfBounds {
        requested: f64,
        oldest: f64,
        newest: f64,
    },

    #[error("Invalid sequence number: received {received}, expected >= {expected}")]
    InvalidSequence { received: u64, expected: u64 },
}

/// Standard 2D player state with position, velocity, and orientation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlayerState2d {
    pub id: EntityId,
    pub position: Vec2,
    pub velocity: Vec2,
    pub orientation: f32,
    pub health: f32,
}

impl PlayerState2d {
    pub fn new(id: EntityId, position: Vec2) -> Self {
        Self {
            id,
            position,
            velocity: Vec2::ZERO,
            orientation: 0.0,
            health: 100.0,
        }
    }

    /// Moves the player state according to a direction vector, speed, and time delta.
    pub fn apply_movement(&mut self, movement: Vec2, speed: f32, dt: f32) {
        let norm_movement = if movement.length_squared() > 1.0 {
            movement.normalize()
        } else {
            movement
        };

        self.velocity = norm_movement * speed;
        self.position += self.velocity * dt;
        if norm_movement.length_squared() > 0.001 {
            self.orientation = norm_movement.y.atan2(norm_movement.x);
        }
    }
}

impl interpolation::Interpolatable for PlayerState2d {
    fn lerp(&self, other: &Self, alpha: f32) -> Self {
        Self {
            id: self.id,
            position: self.position.lerp(other.position, alpha),
            velocity: self.velocity.lerp(other.velocity, alpha),
            orientation: lerp_angle(self.orientation, other.orientation, alpha),
            health: self.health + (other.health - self.health) * alpha,
        }
    }

    fn hermite(
        &self,
        other: &Self,
        v0: Option<Vec2>,
        v1: Option<Vec2>,
        dt: f32,
        alpha: f32,
    ) -> Self {
        let vel0 = v0.unwrap_or(self.velocity);
        let vel1 = v1.unwrap_or(other.velocity);

        let pos = interpolation::hermite_cubic(self.position, other.position, vel0, vel1, dt, alpha);

        Self {
            id: self.id,
            position: pos,
            velocity: vel0.lerp(vel1, alpha),
            orientation: lerp_angle(self.orientation, other.orientation, alpha),
            health: self.health + (other.health - self.health) * alpha,
        }
    }
}

/// Helper for angle shortest-path linear interpolation.
pub fn lerp_angle(a: f32, b: f32, alpha: f32) -> f32 {
    let diff = (b - a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    a + diff * alpha
}
