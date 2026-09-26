use std::collections::{HashMap, VecDeque};
use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::interpolation::Interpolatable;
use crate::{EntityId, NetcodeError, Result};

/// Ray in 2D space for hitscan weapons.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Ray2d {
    pub origin: Vec2,
    pub direction: Vec2,
    pub max_distance: f32,
}

impl Ray2d {
    pub fn new(origin: Vec2, direction: Vec2, max_distance: f32) -> Self {
        let dir = if direction.length_squared() > 1e-6 {
            direction.normalize()
        } else {
            Vec2::X
        };
        Self {
            origin,
            direction: dir,
            max_distance,
        }
    }
}

/// Detailed raycast hit result.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RayHit {
    pub entity_id: EntityId,
    pub distance: f32,
    pub point: Vec2,
    pub normal: Vec2,
}

/// Circle collider in 2D space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CircleCollider {
    pub center: Vec2,
    pub radius: f32,
}

impl CircleCollider {
    pub fn new(center: Vec2, radius: f32) -> Self {
        Self { center, radius }
    }

    pub fn ray_intersection(&self, ray: &Ray2d) -> Option<RayHit> {
        let oc = ray.origin - self.center;
        let b = oc.dot(ray.direction);
        let c = oc.dot(oc) - self.radius * self.radius;
        let discriminant = b * b - c;

        if discriminant < 0.0 {
            return None;
        }

        let sqrt_disc = discriminant.sqrt();
        let mut t = -b - sqrt_disc;
        if t < 0.0 {
            t = -b + sqrt_disc;
        }

        if t >= 0.0 && t <= ray.max_distance {
            let hit_point = ray.origin + ray.direction * t;
            let normal = (hit_point - self.center).normalize_or_zero();
            Some(RayHit {
                entity_id: 0,
                distance: t,
                point: hit_point,
                normal,
            })
        } else {
            None
        }
    }
}

/// Axis-Aligned Bounding Box collider in 2D.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AabbCollider {
    pub min: Vec2,
    pub max: Vec2,
}

impl AabbCollider {
    pub fn new(min: Vec2, max: Vec2) -> Self {
        Self { min, max }
    }

    pub fn from_center_half_extents(center: Vec2, half_extents: Vec2) -> Self {
        Self {
            min: center - half_extents,
            max: center + half_extents,
        }
    }

    pub fn center(&self) -> Vec2 {
        (self.min + self.max) * 0.5
    }

    pub fn half_extents(&self) -> Vec2 {
        (self.max - self.min) * 0.5
    }

    pub fn ray_intersection(&self, ray: &Ray2d) -> Option<RayHit> {
        let mut tmin = 0.0f32;
        let mut tmax = ray.max_distance;
        let mut normal = Vec2::ZERO;

        // X axis
        if ray.direction.x.abs() > 1e-6 {
            let inv_d = 1.0 / ray.direction.x;
            let mut t1 = (self.min.x - ray.origin.x) * inv_d;
            let mut t2 = (self.max.x - ray.origin.x) * inv_d;
            let mut n = Vec2::new(-1.0, 0.0);

            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
                n = Vec2::new(1.0, 0.0);
            }

            if t1 > tmin {
                tmin = t1;
                normal = n;
            }
            tmax = tmax.min(t2);
            if tmin > tmax {
                return None;
            }
        } else if ray.origin.x < self.min.x || ray.origin.x > self.max.x {
            return None;
        }

        // Y axis
        if ray.direction.y.abs() > 1e-6 {
            let inv_d = 1.0 / ray.direction.y;
            let mut t1 = (self.min.y - ray.origin.y) * inv_d;
            let mut t2 = (self.max.y - ray.origin.y) * inv_d;
            let mut n = Vec2::new(0.0, -1.0);

            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
                n = Vec2::new(0.0, 1.0);
            }

            if t1 > tmin {
                tmin = t1;
                normal = n;
            }
            tmax = tmax.min(t2);
            if tmin > tmax {
                return None;
            }
        } else if ray.origin.y < self.min.y || ray.origin.y > self.max.y {
            return None;
        }

        if tmin <= ray.max_distance && tmin >= 0.0 {
            Some(RayHit {
                entity_id: 0,
                distance: tmin,
                point: ray.origin + ray.direction * tmin,
                normal,
            })
        } else {
            None
        }
    }
}

/// Generic 2D collider that can be either circle or box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Collider2d {
    Circle(CircleCollider),
    Aabb(AabbCollider),
}

impl Collider2d {
    pub fn circle(center: Vec2, radius: f32) -> Self {
        Collider2d::Circle(CircleCollider::new(center, radius))
    }

    pub fn aabb(center: Vec2, half_extents: Vec2) -> Self {
        Collider2d::Aabb(AabbCollider::from_center_half_extents(center, half_extents))
    }

    pub fn center(&self) -> Vec2 {
        match self {
            Collider2d::Circle(c) => c.center,
            Collider2d::Aabb(b) => b.center(),
        }
    }

    pub fn lerp(&self, other: &Self, alpha: f32) -> Self {
        match (self, other) {
            (Collider2d::Circle(c1), Collider2d::Circle(c2)) => {
                Collider2d::Circle(CircleCollider {
                    center: c1.center.lerp(c2.center, alpha),
                    radius: c1.radius + (c2.radius - c1.radius) * alpha,
                })
            }
            (Collider2d::Aabb(b1), Collider2d::Aabb(b2)) => Collider2d::Aabb(AabbCollider {
                min: b1.min.lerp(b2.min, alpha),
                max: b1.max.lerp(b2.max, alpha),
            }),
            // Fallback for mismatch: pick nearest based on alpha
            _ => {
                if alpha < 0.5 {
                    *self
                } else {
                    *other
                }
            }
        }
    }

    pub fn ray_intersection(&self, ray: &Ray2d) -> Option<RayHit> {
        match self {
            Collider2d::Circle(c) => c.ray_intersection(ray),
            Collider2d::Aabb(b) => b.ray_intersection(ray),
        }
    }
}

/// A snapshot of all players and their hitboxes at an exact server timestamp.
#[derive(Debug, Clone)]
pub struct HistoryFrame<S> {
    pub timestamp: f64,
    pub entities: HashMap<EntityId, (S, Collider2d)>,
}

/// Server lag compensation history buffer, maintaining player hitboxes over a past time window.
#[derive(Debug, Clone)]
pub struct LagCompensationHistory<S> {
    frames: VecDeque<HistoryFrame<S>>,
    pub max_history_duration: f64,
}

impl<S: Interpolatable> LagCompensationHistory<S> {
    pub fn new(max_history_duration: f64) -> Self {
        Self {
            frames: VecDeque::new(),
            max_history_duration,
        }
    }

    /// Records the current authoritative frame into the lag compensation history.
    pub fn record_frame(&mut self, timestamp: f64, entities: HashMap<EntityId, (S, Collider2d)>) {
        self.frames.push_back(HistoryFrame {
            timestamp,
            entities,
        });

        // Prune frames older than max history window
        while let Some(front) = self.frames.front() {
            if timestamp - front.timestamp > self.max_history_duration {
                self.frames.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn oldest_timestamp(&self) -> Option<f64> {
        self.frames.front().map(|f| f.timestamp)
    }

    pub fn newest_timestamp(&self) -> Option<f64> {
        self.frames.back().map(|f| f.timestamp)
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Reconstructs the world state at `rewind_time` by interpolating between the two nearest frames,
    /// runs the query closure `query_fn`, and returns its output.
    pub fn rewind_and_query<F, R>(&self, rewind_time: f64, query_fn: F) -> Result<R>
    where
        F: FnOnce(&HashMap<EntityId, (S, Collider2d)>) -> R,
    {
        if self.frames.is_empty() {
            return Err(NetcodeError::InsufficientSnapshots {
                available: 0,
                required: 1,
            });
        }

        let oldest = self.frames.front().unwrap().timestamp;
        let newest = self.frames.back().unwrap().timestamp;

        // If history is small, clamp to available window
        let clamped_time = rewind_time.clamp(oldest, newest);

        // Find bounding frames [f0, f1]
        let mut reconstructed = HashMap::new();

        if (clamped_time - oldest).abs() < 1e-5 || self.frames.len() == 1 {
            reconstructed = self.frames.front().unwrap().entities.clone();
        } else if (clamped_time - newest).abs() < 1e-5 {
            reconstructed = self.frames.back().unwrap().entities.clone();
        } else {
            for i in 0..self.frames.len() - 1 {
                let f0 = &self.frames[i];
                let f1 = &self.frames[i + 1];

                if f0.timestamp <= clamped_time && clamped_time <= f1.timestamp {
                    let dt = (f1.timestamp - f0.timestamp) as f32;
                    let alpha = if dt > 1e-6 {
                        ((clamped_time - f0.timestamp) as f32 / dt).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };

                    for (id, (state0, col0)) in &f0.entities {
                        if let Some((state1, col1)) = f1.entities.get(id) {
                            let lerped_state = state0.lerp(state1, alpha);
                            let lerped_col = col0.lerp(col1, alpha);
                            reconstructed.insert(*id, (lerped_state, lerped_col));
                        } else {
                            reconstructed.insert(*id, (state0.clone(), *col0));
                        }
                    }
                    break;
                }
            }
        }

        Ok(query_fn(&reconstructed))
    }

    /// Rewinds all player colliders to `rewind_time` ($T_{shot}$) and performs raycast collision detection.
    /// Excludes `shooter_id` from the raycast test.
    pub fn rewind_and_raycast(
        &self,
        rewind_time: f64,
        shooter_id: EntityId,
        ray: &Ray2d,
    ) -> Result<Option<RayHit>> {
        self.rewind_and_query(rewind_time, |entities| {
            let mut closest_hit: Option<RayHit> = None;

            for (&id, (_state, collider)) in entities {
                if id == shooter_id {
                    continue;
                }

                if let Some(mut hit) = collider.ray_intersection(ray) {
                    hit.entity_id = id;
                    if let Some(ref current_closest) = closest_hit {
                        if hit.distance < current_closest.distance {
                            closest_hit = Some(hit);
                        }
                    } else {
                        closest_hit = Some(hit);
                    }
                }
            }

            closest_hit
        })
    }
}
