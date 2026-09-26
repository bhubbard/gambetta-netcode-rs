use std::collections::VecDeque;
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Cubic Hermite spline basis calculation in 2D.
pub fn hermite_cubic(p0: Vec2, p1: Vec2, v0: Vec2, v1: Vec2, dt: f32, alpha: f32) -> Vec2 {
    let t = alpha.clamp(0.0, 1.0);
    let t2 = t * t;
    let t3 = t2 * t;

    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;

    p0 * h00 + (v0 * dt) * h10 + p1 * h01 + (v1 * dt) * h11
}

/// Trait implemented by game states that can be smoothly interpolated.
pub trait Interpolatable: Clone {
    fn lerp(&self, other: &Self, alpha: f32) -> Self;

    fn hermite(
        &self,
        other: &Self,
        v0: Option<Vec2>,
        v1: Option<Vec2>,
        dt: f32,
        alpha: f32,
    ) -> Self {
        let _ = (v0, v1, dt);
        self.lerp(other, alpha)
    }
}

impl Interpolatable for Vec2 {
    fn lerp(&self, other: &Self, alpha: f32) -> Self {
        Vec2::lerp(*self, *other, alpha)
    }

    fn hermite(
        &self,
        other: &Self,
        v0: Option<Vec2>,
        v1: Option<Vec2>,
        dt: f32,
        alpha: f32,
    ) -> Self {
        hermite_cubic(
            *self,
            *other,
            v0.unwrap_or(Vec2::ZERO),
            v1.unwrap_or(Vec2::ZERO),
            dt,
            alpha,
        )
    }
}

impl Interpolatable for f32 {
    fn lerp(&self, other: &Self, alpha: f32) -> Self {
        self + (other - self) * alpha
    }
}

/// Interpolation mode for remote entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterpolationMode {
    Linear,
    HermiteCubic,
}

/// Timestamped snapshot of an entity's state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntitySnapshot<S> {
    pub timestamp: f64,
    pub state: S,
    pub velocity: Option<Vec2>,
}

/// Circular buffer storing recent snapshots for remote entity interpolation.
#[derive(Debug, Clone)]
pub struct SnapshotBuffer<S> {
    snapshots: VecDeque<EntitySnapshot<S>>,
    max_capacity: usize,
}

impl<S: Clone> SnapshotBuffer<S> {
    pub fn new(max_capacity: usize) -> Self {
        Self {
            snapshots: VecDeque::with_capacity(max_capacity),
            max_capacity,
        }
    }

    /// Inserts a new snapshot into the buffer, maintaining ascending timestamp order.
    pub fn insert(&mut self, snapshot: EntitySnapshot<S>) {
        if self.snapshots.len() >= self.max_capacity {
            self.snapshots.pop_front();
        }

        // Insert maintaining timestamp ordering
        let pos = self
            .snapshots
            .iter()
            .rposition(|s| s.timestamp <= snapshot.timestamp);

        match pos {
            Some(idx) => self.snapshots.insert(idx + 1, snapshot),
            None => self.snapshots.push_front(snapshot),
        }
    }

    pub fn len(&self) -> usize {
        self.snapshots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }

    pub fn oldest(&self) -> Option<&EntitySnapshot<S>> {
        self.snapshots.front()
    }

    pub fn newest(&self) -> Option<&EntitySnapshot<S>> {
        self.snapshots.back()
    }

    pub fn iter(&self) -> impl Iterator<Item = &EntitySnapshot<S>> {
        self.snapshots.iter()
    }
}

/// Interpolator for remote entities with configurable render time delay.
#[derive(Debug, Clone)]
pub struct EntityInterpolator<S> {
    buffer: SnapshotBuffer<S>,
    pub render_delay: f64,
    pub mode: InterpolationMode,
    pub max_extrapolation: f64,
}

impl<S: Interpolatable> EntityInterpolator<S> {
    pub fn new(render_delay: f64) -> Self {
        Self {
            buffer: SnapshotBuffer::new(64),
            render_delay,
            mode: InterpolationMode::Linear,
            max_extrapolation: 0.050, // 50ms default jitter absorption
        }
    }

    pub fn with_capacity(render_delay: f64, capacity: usize) -> Self {
        Self {
            buffer: SnapshotBuffer::new(capacity),
            render_delay,
            mode: InterpolationMode::Linear,
            max_extrapolation: 0.050,
        }
    }

    pub fn set_mode(&mut self, mode: InterpolationMode) {
        self.mode = mode;
    }

    /// Adds a new snapshot from the server.
    pub fn add_snapshot(&mut self, timestamp: f64, state: S, velocity: Option<Vec2>) {
        self.buffer.insert(EntitySnapshot {
            timestamp,
            state,
            velocity,
        });
    }

    /// Samples the remote entity at render time: $T_{render} = T_{now} - \text{render\_delay}$.
    pub fn sample(&self, current_time: f64) -> Option<S> {
        let render_time = current_time - self.render_delay;
        self.sample_at_time(render_time)
    }

    /// Directly samples the entity state at an explicit past or present timestamp.
    pub fn sample_at_time(&self, render_time: f64) -> Option<S> {
        if self.buffer.is_empty() {
            return None;
        }

        let oldest = self.buffer.oldest().unwrap();
        let newest = self.buffer.newest().unwrap();

        // If target time is older than the oldest snapshot in the window, clamp to oldest
        if render_time <= oldest.timestamp {
            return Some(oldest.state.clone());
        }

        // If target time is newer than newest snapshot, check extrapolation or clamp
        if render_time >= newest.timestamp {
            let delta = render_time - newest.timestamp;
            if delta <= self.max_extrapolation && self.buffer.len() >= 2 {
                // Extrapolate using the last two snapshots
                let prev = &self.buffer.snapshots[self.buffer.len() - 2];
                let dt = (newest.timestamp - prev.timestamp).max(1e-4) as f32;
                let alpha = 1.0 + (delta as f32 / dt);
                return Some(prev.state.lerp(&newest.state, alpha));
            }
            return Some(newest.state.clone());
        }

        // Find surrounding snapshots [s0, s1] such that s0.timestamp <= render_time <= s1.timestamp
        for i in 0..self.buffer.snapshots.len() - 1 {
            let s0 = &self.buffer.snapshots[i];
            let s1 = &self.buffer.snapshots[i + 1];

            if s0.timestamp <= render_time && render_time <= s1.timestamp {
                let dt = (s1.timestamp - s0.timestamp) as f32;
                if dt <= 1e-6 {
                    return Some(s1.state.clone());
                }

                let alpha = ((render_time - s0.timestamp) as f32 / dt).clamp(0.0, 1.0);

                return match self.mode {
                    InterpolationMode::Linear => Some(s0.state.lerp(&s1.state, alpha)),
                    InterpolationMode::HermiteCubic => {
                        Some(s0.state.hermite(&s1.state, s0.velocity, s1.velocity, dt, alpha))
                    }
                };
            }
        }

        Some(newest.state.clone())
    }
}
