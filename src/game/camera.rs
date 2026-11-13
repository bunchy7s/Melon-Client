//! Vanilla `Camera` and `CameraType`: where the view is for this frame, and

use glam::{DVec3, Mat4, Vec3};

use super::InWorld;
use crate::math::angle::{self, Rotation};
use crate::renderer::world_projection;
use crate::world::culling::frustum::Frustum;
use crate::world::level::Level;
use crate::world::pick;
use crate::world::player::{self, PlayerState};

/// `Camera.getMaxZoom`'s starting distance behind or in front of the head.
const THIRD_PERSON_DISTANCE: f32 = 4.0;

/// Matches vanilla's `CameraType`: `F5` cycles through all three.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum CameraMode {
    #[default]
    FirstPerson,
    /// Normal over-the-shoulder chase camera, behind the player.
    ThirdPersonBack,
    /// The "selfie" view: camera in front of the player, facing back at
    ThirdPersonFront,
}

impl crate::cycle::Cycle for CameraMode {
    const ALL: &'static [Self] = &[
        CameraMode::FirstPerson,
        CameraMode::ThirdPersonBack,
        CameraMode::ThirdPersonFront,
    ];
}

/// One frame's camera.
pub struct Camera {
    pub position: Vec3,
    pub view: Mat4,
    /// Vertical field of view, degrees.
    pub fov: f32,
    /// Far plane, blocks.
    pub far: f32,
    /// `GameRenderer.bobView`, between the projection and the view: what
    pub bob: Mat4,
}

impl Camera {
    /// `Camera.setup`: at the eye, or pulled back along (or against) the
    pub fn follow(
        player: &PlayerState,
        level: &Level,
        mode: CameraMode,
        fov: f32,
        far: f32,
    ) -> Self {
        let eye = player.camera_eye();
        let clip = |from: DVec3, to: DVec3| pick::clip_blocks(level, from, to).map(|h| h.location);
        let (position, view) = match mode {
            CameraMode::FirstPerson => (eye, player.view_matrix_at(eye)),
            CameraMode::ThirdPersonBack => {
                let forward = player.forward_vector();
                let pos = eye - forward * max_zoom(eye, forward, THIRD_PERSON_DISTANCE, clip);
                (pos, player.view_matrix_at(pos))
            }
            CameraMode::ThirdPersonFront => {
                let yaw = player.yaw() + 180.0;
                let pitch = -player.pitch();
                let forward = player::forward_vector_for(yaw, pitch);
                let pos = eye - forward * max_zoom(eye, forward, THIRD_PERSON_DISTANCE, clip);
                (pos, player::view_matrix_for(yaw, pitch, pos))
            }
        };
        Self {
            position,
            view,
            fov,
            far,
            bob: Mat4::IDENTITY,
        }
    }

    /// Where the camera looks, in world space.
    pub fn forward(&self) -> Vec3 {
        -Vec3::new(self.view.x_axis.z, self.view.y_axis.z, self.view.z_axis.z)
    }

    /// The camera's up, in world space.
    pub fn up(&self) -> Vec3 {
        Vec3::new(self.view.x_axis.y, self.view.y_axis.y, self.view.z_axis.y)
    }

    /// The camera's left, in world space.
    pub fn left(&self) -> Vec3 {
        -Vec3::new(self.view.x_axis.x, self.view.y_axis.x, self.view.z_axis.x)
    }

    /// `Camera.getNearPlane`: where the middle and the corners of the near
    pub fn near_plane(&self, aspect: f32) -> [Vec3; 5] {
        let near = crate::renderer::WORLD_Z_NEAR;
        let half_height = (self.fov.to_radians() / 2.0).tan() * near;
        let half_width = half_height * aspect;
        let forward = self.forward() * near;
        let (up, left) = (self.up() * half_height, self.left() * half_width);
        [
            forward,
            forward + up + left,
            forward + up - left,
            forward - up + left,
            forward - up - left,
        ]
    }

    /// The view volume for a framebuffer of this `aspect`, the same one the
    pub fn frustum(&self, aspect: f32) -> Frustum {
        self.frustum_with_fov(aspect, self.fov)
    }

    /// [`Self::frustum`] for another field of view: vanilla culls with at
    pub fn frustum_with_fov(&self, aspect: f32, fov: f32) -> Frustum {
        Frustum::from_view_projection(
            world_projection(fov, aspect, self.far) * self.bob * self.view,
        )
    }
}

/// `Camera.getMaxZoom`: how far behind `eye` (against `forward`) the camera
pub fn max_zoom(
    eye: Vec3,
    forward: Vec3,
    distance: f32,
    clip: impl Fn(DVec3, DVec3) -> Option<DVec3>,
) -> f32 {
    let mut distance = distance;
    let back = -forward.as_dvec3();
    for corner in 0..8 {
        let offset = Vec3::new(
            ((corner & 1) * 2 - 1) as f32,
            (((corner >> 1) & 1) * 2 - 1) as f32,
            (((corner >> 2) & 1) * 2 - 1) as f32,
        ) * 0.1;
        let from = (eye + offset).as_dvec3();
        let to = from + back * f64::from(distance);
        if let Some(hit) = clip(from, to) {
            let hit_distance = hit.distance(eye.as_dvec3()) as f32;
            if hit_distance < distance {
                distance = hit_distance;
            }
        }
    }
    distance
}

/// `Camera.fovModifier`'s bounds.
const FOV_MODIFIER_RANGE: (f32, f32) = (0.1, 1.5);

impl InWorld {
    /// `Camera.tickFov`: the share of the field of view that sprinting,
    pub(super) fn tick_fov(&mut self, effect_scale: f32) {
        let target = self.target_fov_modifier(effect_scale);
        self.fov_modifier_old = self.fov_modifier;
        self.fov_modifier += (target - self.fov_modifier) * 0.5;
        self.fov_modifier = self
            .fov_modifier
            .clamp(FOV_MODIFIER_RANGE.0, FOV_MODIFIER_RANGE.1);
    }

    /// `AbstractClientPlayer.getFieldOfViewModifier`.
    fn target_fov_modifier(&self, effect_scale: f32) -> f32 {
        let mut modifier = self.player.fov_modifier();
        if let Some(using) = self.hands.using {
            if using.item_use.animation == crate::world::player::hands::UseAnimation::Bow {
                let used = (using.item_use.duration - using.remaining) as f32;
                let scale = (used / 20.0).min(1.0);
                modifier *= 1.0 - scale * scale * 0.15;
            } else if self.camera_mode == CameraMode::FirstPerson
                && self.hand_stack(using.hand).item.registry_key == "spyglass"
            {
                return 0.1;
            }
        }
        1.0 + (modifier - 1.0) * effect_scale
    }

    /// `Camera.calculateFov`'s `fov * lerp(partialTicks, old, new)`.
    pub(super) fn fov_with_effects(&self, fov: f32, partial: f32) -> f32 {
        fov * (self.fov_modifier_old + (self.fov_modifier - self.fov_modifier_old) * partial)
    }

    /// `GameRenderer.bobHurt`: a dying camera rolls over to one side
    pub(super) fn hurt_bob(&self, strength: f32, partial: f32) -> Mat4 {
        let mut bob = Mat4::IDENTITY;
        if self.status.health <= 0.0 {
            let duration = (self.own_death as f32 + partial).min(20.0);
            bob *= Mat4::rot_z_deg(40.0 - 8000.0 / (duration + 200.0));
        }
        let hurt = self.own_hurt as f32 - partial;
        if hurt >= 0.0 {
            let hurt = hurt / crate::world::entity::tracker::HURT_DURATION as f32;
            let hurt = angle::sin(hurt * hurt * hurt * hurt * crate::math::angle::PI);
            let direction = self.own_hurt_dir;
            bob *= Mat4::rot_y_deg(-direction)
                * Mat4::rot_z_deg(-hurt * 14.0 * strength)
                * Mat4::rot_y_deg(direction);
        }
        bob
    }
}

/// `Camera.modifyFovBasedOnDeathOrFluid`: the view narrows while the camera
pub fn dying_fov(fov: f32, death_time: f32) -> f32 {
    let duration = death_time.min(20.0);
    fov / ((1.0 - 500.0 / (duration + 500.0)) * 2.0 + 1.0)
}

/// `GameRenderer.getDepthFar`: four times the render distance, so fog-free
pub fn far_plane(render_distance: u8) -> f32 {
    (f32::from(render_distance) * 16.0 * 4.0).max(256.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::angle::{HALF_PI, Rotation};

    /// A wall at `x = wall`, hit by any segment crossing it.
    fn wall(wall: f64) -> impl Fn(DVec3, DVec3) -> Option<DVec3> {
        move |from, to| {
            let (low, high) = (from.x.min(to.x), from.x.max(to.x));
            (low <= wall && wall <= high).then(|| DVec3::new(wall, from.y, from.z))
        }
    }

    #[test]
    fn the_camera_goes_the_whole_way_back_in_open_air() {
        let eye = Vec3::new(0.0, 65.0, 0.0);
        assert_eq!(max_zoom(eye, Vec3::X, 4.0, |_, _| None), 4.0);
    }

    #[test]
    fn a_wall_behind_the_player_stops_the_camera_short_of_it() {
        let eye = Vec3::new(0.0, 65.0, 0.0);
        let zoom = max_zoom(eye, Vec3::X, 4.0, wall(-2.0));
        assert!((2.0..2.01).contains(&zoom), "{zoom}");
        assert_eq!(max_zoom(eye, Vec3::X, 4.0, wall(2.0)), 4.0);
    }

    #[test]
    fn a_dying_camera_narrows_its_view_for_twenty_ticks() {
        assert_eq!(dying_fov(70.0, 0.0), 70.0 / 1.0);
        let narrowed = dying_fov(70.0, 20.0);
        assert!(narrowed < 70.0 && narrowed > 60.0, "{narrowed}");
        assert_eq!(dying_fov(70.0, 40.0), dying_fov(70.0, 20.0));
    }

    #[test]
    fn the_axes_follow_the_view() {
        let camera = Camera {
            position: Vec3::ZERO,
            view: Mat4::rot_y(HALF_PI),
            fov: 70.0,
            far: 256.0,
            bob: Mat4::IDENTITY,
        };
        assert!((camera.forward() - Vec3::X).length() < 1e-6);
        assert!((camera.up() - Vec3::Y).length() < 1e-6);
        assert!((camera.left() - Vec3::NEG_Z).length() < 1e-6);
        let plane = camera.near_plane(2.0);
        assert!((plane[0] - Vec3::X * 0.05).length() < 1e-6);
        let mean = plane[1..].iter().copied().sum::<Vec3>() / 4.0;
        assert!((mean - plane[0]).length() < 1e-6);
    }
}
