//! What the drawn player and the camera see between ticks: the

use super::PlayerState;
use super::first_person;
use super::hands::ItemUse;
use crate::math::angle::{self, PI, Rotation};
use crate::math::wrap_degrees;
use crate::world::entity::humanoid::{ArmPose, HumanoidRenderState, SwingPose};
use crate::world::entity::living::{Swim, SwimTurn};
use crate::world::entity::player::{Arm, AvatarRenderState, ShownParts};
use glam::{Mat3, Mat4, Vec3};

impl PlayerState {
    /// Once a frame: places the drawn player `partial` of the way from the
    pub fn update_render(&mut self, partial: f32, dt: f32) {
        let dt = dt.min(0.1);
        self.partial = partial.clamp(0.0, 1.0);
        self.pos = self
            .prev_feet
            .lerp(self.feet, f64::from(partial.clamp(0.0, 1.0)))
            .as_vec3();

        self.age_ticks += 20.0 * dt;

        let remaining = 0.5f32.powf(20.0 * dt);
        self.yaw_bob = self.yaw_degrees + wrap_degrees(self.yaw_bob - self.yaw_degrees) * remaining;
        self.pitch_bob = self.pitch_degrees + (self.pitch_bob - self.pitch_degrees) * remaining;
    }

    /// `Player.getRopeHoldPosition`: where a lead held by the player ends,
    pub fn rope_hold(&self, main_arm: Arm) -> Vec3 {
        let height = self.box_height() as f32;
        crate::world::entity::leash::player_hold(
            self.pos,
            self.yaw_degrees + 180.0,
            main_arm == Arm::Right,
            self.crouching,
            height,
        )
    }

    /// Where the fishing line leaves the hand in a view that isn't the
    pub fn rod_hand(&self, right: bool) -> Vec3 {
        crate::world::entity::fishing::hand_third_person(
            self.eye(),
            self.yaw_degrees + 180.0,
            right,
            self.crouching,
            1.0,
        )
    }

    /// Camera position: feet plus the current pose's eye height
    pub fn eye(&self) -> Vec3 {
        self.pos + Vec3::new(0.0, self.eye_height(), 0.0)
    }

    /// `Camera.setup`'s position: the eye height eased between ticks, so
    pub fn camera_eye(&self) -> Vec3 {
        let height = self.camera_eye_old + (self.camera_eye - self.camera_eye_old) * self.partial;
        self.pos + Vec3::new(0.0, height, 0.0)
    }

    /// World-space look direction (yaw 0 = `-Z`, matching [`Self::tick`]'s
    pub fn forward_vector(&self) -> Vec3 {
        forward_vector_for(self.yaw_degrees, self.pitch_degrees)
    }

    /// `Camera` order: translate by `-eye`, then rotate - unlike the sky's
    pub fn view_matrix_at(&self, eye: Vec3) -> Mat4 {
        view_matrix_for(self.yaw_degrees, self.pitch_degrees, eye)
    }

    /// The item being used, whose `UseEffects` slow the next ticks.
    pub fn set_item_use(&mut self, item_use: Option<ItemUse>) {
        self.item_use = item_use;
    }

    /// `GameRenderer.bobView`: the sway of a walking head, in the camera's
    pub fn view_bob(&self) -> Mat4 {
        let partial = self.partial;
        let step = self.walk_dist - self.walk_dist_old;
        let walked = -(self.walk_dist + step * partial);
        let bob = self.bob_old + (self.bob - self.bob_old) * partial;
        let (sin, cos) = angle::sin_cos(walked * PI);
        Mat4::from_translation(Vec3::new(sin * bob * 0.5, -(cos * bob).abs(), 0.0))
            * Mat4::rot_z_deg(sin * bob * 3.0)
            * Mat4::rot_x_deg((angle::cos(walked * PI - 0.2) * bob).abs() * 5.0)
    }

    /// What the body is drawn as this frame: where, facing where, the
    pub fn avatar_state(
        &self,
        arms: ArmPoses,
        swing: Option<SwingPose>,
        spectator: bool,
        shown: ShownParts,
    ) -> AvatarRenderState {
        let (body_yaw, head_turn) = match self.ride_body.filter(|_| self.riding) {
            Some((before, now)) => {
                let mount = crate::world::entity::living::rot_lerp(self.partial, before, now);
                let turn = crate::math::wrap_degrees(self.yaw_degrees - mount).clamp(-85.0, 85.0);
                let body =
                    self.yaw_degrees - turn + if turn.abs() > 50.0 { turn * 0.2 } else { 0.0 };
                (body, self.yaw_degrees - body)
            }
            None => (self.yaw_degrees, 0.0),
        };
        AvatarRenderState {
            humanoid: HumanoidRenderState {
                x_rot: self.pitch_degrees,
                y_rot: crate::math::wrap_degrees(head_turn),
                riding: self.riding,
                walk_position: self.walk.position(self.partial),
                walk_speed: self.walk.speed(self.partial),
                speed_value: 1.0,
                age_in_ticks: self.age_ticks,
                crouching: self.crouching,
                swim_amount: self.swim_amount_old
                    + (self.swim_amount - self.swim_amount_old) * self.partial,
                using_item: self.item_use.is_some(),
                right_arm_pose: arms.right,
                left_arm_pose: arms.left,
                swing,
                ..Default::default()
            },
            position: self.pos,
            body_yaw,
            spectator,
            shown,
            death_time: 0.0,
            upside_down: None,
            swim: Swim {
                amount: self.swim_amount_old
                    + (self.swim_amount - self.swim_amount_old) * self.partial,
                turn: SwimTurn::Avatar {
                    in_water: self.in_water,
                    visually_swimming: self.swim_pose && self.in_water,
                },
            },
        }
    }

    /// World matrix for the bare first-person arm, lowered by `lowered`
    pub fn first_person_arm_transform(
        &self,
        eye: Vec3,
        arm: Arm,
        lowered: f32,
        attack: f32,
    ) -> Mat4 {
        self.hand_space(eye) * first_person::player_arm(arm, lowered, attack)
    }

    /// Where first-person hands are drawn from: the camera at `eye`, with
    pub fn hand_space(&self, eye: Vec3) -> Mat4 {
        let cam_to_world = Mat4::from_translation(eye)
            * Mat4::rot_y(-self.yaw_degrees.to_radians())
            * Mat4::rot_x(-self.pitch_degrees.to_radians());
        let yaw_lag = wrap_degrees(self.yaw_degrees - self.yaw_bob);
        let pitch_lag = self.pitch_degrees - self.pitch_bob;
        let sway = Mat4::rot_x_deg(pitch_lag * 0.1) * Mat4::rot_y_deg(yaw_lag * 0.1);
        cam_to_world * sway
    }
}

/// How each arm holds its item (`AvatarRenderer.getArmPose`).
#[derive(Clone, Copy, Default, Debug)]
pub struct ArmPoses {
    pub right: ArmPose,
    pub left: ArmPose,
}

impl ArmPoses {
    /// The main hand is `main_arm`, the offhand the other.
    pub fn new(main_arm: Arm, main_hand: ArmPose, offhand: ArmPose) -> Self {
        let offhand = offhand.offhand_of(main_hand);
        match main_arm {
            Arm::Right => Self {
                right: main_hand,
                left: offhand,
            },
            Arm::Left => Self {
                right: offhand,
                left: main_hand,
            },
        }
    }
}

/// World-space look direction for an arbitrary `yaw`/`pitch` (yaw 0 = `-Z`).
pub fn forward_vector_for(yaw_degrees: f32, pitch_degrees: f32) -> Vec3 {
    let (sin_yaw, cos_yaw) = angle::unit(yaw_degrees * angle::DEG_TO_RAD);
    let (sin_pitch, cos_pitch) = angle::unit(pitch_degrees * angle::DEG_TO_RAD);
    Vec3::new(sin_yaw * cos_pitch, -sin_pitch, -cos_yaw * cos_pitch)
}

/// View matrix for an arbitrary `yaw`/`pitch`/`eye` (`Camera` order:
pub fn view_matrix_for(yaw_degrees: f32, pitch_degrees: f32, eye: Vec3) -> Mat4 {
    let (sx, cx) = angle::unit(pitch_degrees * angle::DEG_TO_RAD);
    let (sy, cy) = angle::unit(yaw_degrees * angle::DEG_TO_RAD);
    let rotation = Mat3::from_cols(
        Vec3::new(cy, sx * sy, -cx * sy),
        Vec3::new(0.0, cx, sx),
        Vec3::new(sy, -sx * cy, cx * cy),
    );
    Mat4::from_cols(
        rotation.x_axis.extend(0.0),
        rotation.y_axis.extend(0.0),
        rotation.z_axis.extend(0.0),
        (-(rotation * eye)).extend(1.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_matrix_is_pitch_yaw_then_eye() {
        let eye = Vec3::new(12.5, 70.0, -3.25);
        for (yaw, pitch) in [(0.0, 0.0), (37.0, -20.0), (-140.0, 85.0), (275.0, 10.0)] {
            let expected =
                Mat4::rot_x_deg(pitch) * Mat4::rot_y_deg(yaw) * Mat4::from_translation(-eye);
            assert!(
                view_matrix_for(yaw, pitch, eye).abs_diff_eq(expected, 1e-4),
                "{yaw} {pitch}"
            );
            let ahead = view_matrix_for(yaw, pitch, Vec3::ZERO)
                .transform_vector3(forward_vector_for(yaw, pitch));
            assert!(ahead.abs_diff_eq(Vec3::NEG_Z, 1e-4), "{ahead}");
        }
    }
}
