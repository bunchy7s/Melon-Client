//! Where a tracked entity is: following the server's moves and teleports

use super::*;
use crate::world::fluid::FluidKind;

impl ClientEntity {
    /// `moveOrInterpolateTo`: glide there over [`LERP_STEPS`] ticks, or
    pub(super) fn interpolate_to(&mut self, pos: DVec3, yaw: f32, pitch: f32) {
        if pos.distance_squared(self.pos) > SNAP_DISTANCE * SNAP_DISTANCE {
            self.pos = pos;
            self.prev_pos = pos;
            self.yaw = yaw;
            self.pitch = pitch;
            self.lerp = None;
            return;
        }
        if self.kind.simulates_movement() {
            self.pos = pos;
            self.yaw = yaw;
            self.pitch = pitch;
            self.lerp = None;
            return;
        }
        self.lerp = Some(Lerp {
            pos,
            yaw,
            pitch,
            steps: LERP_STEPS,
        });
    }

    /// Where the interpolation ends, or the entity is.
    pub(super) fn target(&self) -> (DVec3, f32, f32) {
        self.lerp.map_or((self.pos, self.yaw, self.pitch), |l| {
            (l.pos, l.yaw, l.pitch)
        })
    }

    /// `handleMoveEntity`: each step counts from the one before it,
    pub(super) fn relative_move(&mut self, m: &MoveEntity) {
        let (target, mut yaw, mut pitch) = self.target();
        let mut pos = target;
        if !m.steps.is_empty() {
            let mut at = self.base;
            for step in &m.steps {
                at = decode_delta(at, *step);
            }
            self.base = at;
            pos = at;
        }
        if let Some([y, p]) = m.rotation {
            yaw = y;
            pitch = p;
        }
        self.on_ground = m.on_ground;
        self.interpolate_to(pos, yaw, pitch);
    }

    /// `VecDeltaCodec` only: the client is moving this entity itself, so a
    pub(super) fn track_codec_move(&mut self, m: &MoveEntity) {
        if !m.steps.is_empty() {
            let mut at = self.base;
            for step in &m.steps {
                at = decode_delta(at, *step);
            }
            self.base = at;
        }
        self.on_ground = m.on_ground;
    }

    /// Absolute or relative sync of the codec (and velocity), without
    pub(super) fn track_codec_position(&mut self, p: &SetPosition) {
        let r = |flag: i32, current: f64, new: f64| {
            if p.relative & flag != 0 {
                current + new
            } else {
                new
            }
        };
        self.base = DVec3::new(
            r(relative::X, self.base.x, p.pos[0]),
            r(relative::Y, self.base.y, p.pos[1]),
            r(relative::Z, self.base.z, p.pos[2]),
        );
        self.on_ground = p.on_ground;
        if let Some(v) = p.velocity {
            let d = |flag: i32, current: f64, new: f64| {
                if p.relative & flag != 0 {
                    current + new
                } else {
                    new
                }
            };
            self.velocity = DVec3::new(
                d(relative::DELTA_X, self.velocity.x, v[0]),
                d(relative::DELTA_Y, self.velocity.y, v[1]),
                d(relative::DELTA_Z, self.velocity.z, v[2]),
            );
        }
    }

    /// `handleEntityPositionSync` and `handleTeleportEntity`.
    pub(super) fn set_position(&mut self, p: &SetPosition) {
        let (target, yaw, pitch) = self.target();
        let r = |flag: i32, current: f64, new: f64| {
            if p.relative & flag != 0 {
                current + new
            } else {
                new
            }
        };
        let pos = DVec3::new(
            r(relative::X, target.x, p.pos[0]),
            r(relative::Y, target.y, p.pos[1]),
            r(relative::Z, target.z, p.pos[2]),
        );
        let yaw = r(relative::Y_ROT, f64::from(yaw), f64::from(p.yaw)) as f32;
        let pitch = r(relative::X_ROT, f64::from(pitch), f64::from(p.pitch)) as f32;
        self.base = pos;
        self.on_ground = p.on_ground;
        if let Some(v) = p.velocity {
            let d = |flag: i32, current: f64, new: f64| {
                if p.relative & flag != 0 {
                    current + new
                } else {
                    new
                }
            };
            self.velocity = DVec3::new(
                d(relative::DELTA_X, self.velocity.x, v[0]),
                d(relative::DELTA_Y, self.velocity.y, v[1]),
                d(relative::DELTA_Z, self.velocity.z, v[2]),
            );
        }
        self.interpolate_to(pos, yaw, pitch);
    }

    /// `FallingBlockEntity.tick` and `PrimedTnt.tick` as the client runs
    pub(super) fn simulate(&mut self, level: &Level) -> bool {
        let at = self.pos.floor();
        if level
            .block_state(at.x as i32, (at.y as i32).clamp(-2048, 2048), at.z as i32)
            .is_none()
        {
            return false;
        }
        if self.kind == EntityKind::Item {
            self.simulate_item(level);
            return false;
        }
        if !self.appearance.no_gravity {
            self.velocity.y -= GRAVITY;
        }
        self.move_by_velocity(level);
        self.velocity *= f64::from(AIR_DRAG);
        if self.kind == EntityKind::PrimedTnt {
            if self.on_ground {
                self.velocity *= DVec3::new(0.7, -0.5, 0.7);
            }
            self.appearance.fuse -= 1;
            return self.appearance.fuse <= 0;
        }
        false
    }

    /// `ItemEntity.tick` as the client runs it: buoyancy in a fluid, else
    fn simulate_item(&mut self, level: &Level) {
        let (w, h) = self.size();
        let region = Aabb::at_feet(self.pos, f64::from(w), f64::from(h)).deflate(0.001);
        let fluid = [FluidKind::Water, FluidKind::Lava]
            .into_iter()
            .find(|&kind| physics::fluid_current(level, &region, self.pos.y, kind).height > 0.1);
        match fluid {
            Some(kind) => {
                let drag = if kind == FluidKind::Water { 0.99 } else { 0.95 };
                let lift = if self.velocity.y < 0.06 { 5.0e-4 } else { 0.0 };
                self.velocity = DVec3::new(
                    self.velocity.x * drag,
                    self.velocity.y + lift,
                    self.velocity.z * drag,
                );
            }
            None if self.appearance.no_gravity => {}
            None => self.velocity.y -= GRAVITY,
        }
        let horizontal = self.velocity.x * self.velocity.x + self.velocity.z * self.velocity.z;
        if self.on_ground
            && horizontal <= 1.0e-5
            && (i64::from(self.age) + i64::from(self.id)).rem_euclid(4) != 0
        {
            return;
        }
        self.move_by_velocity(level);
        let mut friction = AIR_DRAG;
        if self.on_ground {
            let block = crate::world::block_motion::block_on(
                level,
                self.pos,
                self.support.block,
                0.999_999,
            );
            friction = block.slipperiness * 0.98;
        }
        let friction = f64::from(friction);
        self.velocity *= DVec3::new(friction, f64::from(AIR_DRAG), friction);
        if self.on_ground && self.velocity.y < 0.0 {
            self.velocity.y *= -0.5;
        }
    }

    /// `Entity.move(MoverType.SELF, deltaMovement)` for a box that doesn't
    pub(super) fn move_by_velocity(&mut self, level: &Level) {
        let delta = self.velocity;
        let (w, h) = self.size();
        let aabb = Aabb::at_feet(self.pos, f64::from(w), f64::from(h));
        let moved = physics::collide(level, &[], aabb, delta, 0.0, self.on_ground);
        let length = moved.length_squared();
        if length > 1.0e-7 || delta.length_squared() - length < 1.0e-7 {
            self.pos += moved;
        }
        let differs = |a: f64, b: f64| (b - a).abs() >= 1.0e-5;
        let (x_hit, z_hit) = (differs(delta.x, moved.x), differs(delta.z, moved.z));
        let moved_vertically = delta.y.abs() > 0.0;
        let vertical_hit = moved_vertically && delta.y != moved.y;
        if moved_vertically {
            self.on_ground = vertical_hit && delta.y < 0.0;
            let now = Aabb::at_feet(self.pos, f64::from(w), f64::from(h));
            self.support =
                physics::update_support(level, &now, self.pos, self.on_ground, moved, self.support);
        }
        if vertical_hit || x_hit || z_hit {
            self.restitute(level, [x_hit, z_hit, vertical_hit], moved.y);
        }
        let factor = f64::from(self.block_speed_factor(level));
        self.velocity.x *= factor;
        self.velocity.z *= factor;
    }

    pub(super) fn block_speed_factor(&self, level: &Level) -> f32 {
        crate::world::block_motion::factor(
            level,
            self.pos,
            self.support.block,
            true,
            crate::world::block_motion::speed_multiplier,
        )
    }

    /// `Entity.restituteMovementAfterCollisions` for a non-living entity.
    pub(super) fn restitute(&mut self, level: &Level, hits: [bool; 3], moved_y: f64) {
        let landed_on = (hits[2] && self.on_ground).then(|| {
            crate::world::block_motion::block_underfoot(level, self.pos, self.support.block)
        });
        self.velocity = crate::world::block_motion::restituted(
            self.velocity,
            moved_y,
            hits,
            landed_on,
            false,
            GRAVITY,
            f64::from(AIR_DRAG),
            false,
        );
    }
}
