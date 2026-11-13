//! `NautilusModel`: a shell over a body with a mouth of three parts. It

use crate::world::entity::animations::nautilus::SWIMMING;
use crate::world::entity::keyframe::{AnimationDefinition, KeyframeAnimation};
use crate::world::entity::living::LivingRenderState;
use crate::world::entity::model::{LayerDefinition, Model, ModelPose, PartDefinition, PartId};
use crate::world::model_part::{Cube, Pose};

/// The swimming animation, driven by the walk.
pub static WALK_ANIMATION: &AnimationDefinition = &SWIMMING;

/// `0.0` for the shell's box and the mouth's, which are grown a hair.
fn mouth(uv: (f32, f32), origin: (f32, f32, f32), size: (f32, f32, f32), grow: f32) -> Vec<Cube> {
    vec![Cube::new(uv, origin, size).grown(grow)]
}

pub fn adult() -> LayerDefinition {
    let mut r = PartDefinition::root();
    let root = r.add_or_replace_child("root", vec![], Pose::offset(0.0, 29.0, -6.0));
    root.add_or_replace_child(
        "shell",
        vec![
            Cube::new((0.0, 0.0), (-7.0, -10.0, -7.0), (14.0, 10.0, 16.0)),
            Cube::new((0.0, 26.0), (-7.0, 0.0, -7.0), (14.0, 8.0, 20.0)),
            Cube::new((48.0, 26.0), (-7.0, 0.0, 6.0), (14.0, 8.0, 0.0)),
        ],
        Pose::offset(0.0, -13.0, 5.0),
    );
    let body = root.add_or_replace_child(
        "body",
        vec![
            Cube::new((0.0, 54.0), (-5.0, -4.51, -3.0), (10.0, 8.0, 14.0)),
            Cube::new((0.0, 76.0), (-5.0, -4.51, 7.0), (10.0, 8.0, 0.0)),
        ],
        Pose::offset(0.0, -8.5, 12.3),
    );
    body.add_or_replace_child(
        "upper_mouth",
        mouth((54.0, 54.0), (-5.0, -2.0, 0.0), (10.0, 4.0, 4.0), -0.001),
        Pose::offset(0.0, -2.51, 7.0),
    );
    body.add_or_replace_child(
        "inner_mouth",
        mouth((54.0, 70.0), (-3.0, -2.0, -0.5), (6.0, 4.0, 4.0), 0.0),
        Pose::offset(0.0, -0.51, 7.5),
    );
    body.add_or_replace_child(
        "lower_mouth",
        mouth((54.0, 62.0), (-5.0, -1.98, 0.0), (10.0, 4.0, 4.0), -0.001),
        Pose::offset(0.0, 1.49, 7.0),
    );
    LayerDefinition::new(r, 128.0, 128.0)
}

/// `NautilusSaddleModel.createSaddleLayer`: the shell grown by 0.2.
pub fn saddle() -> LayerDefinition {
    let mut r = PartDefinition::root();
    let root = r.add_or_replace_child("root", vec![], Pose::offset(0.0, 29.0, -6.0));
    root.add_or_replace_child(
        "shell",
        vec![Cube::new((0.0, 0.0), (-7.0, -10.0, -7.0), (14.0, 10.0, 16.0)).grown(0.2)],
        Pose::offset(0.0, -13.0, 5.0),
    );
    let body = root.add_or_replace_child(
        "body",
        vec![
            Cube::new((0.0, 54.0), (-5.0, -4.51, -3.0), (10.0, 8.0, 14.0)),
            Cube::new((0.0, 76.0), (-5.0, -4.51, 7.0), (10.0, 8.0, 0.0)),
        ],
        Pose::offset(0.0, -8.5, 12.3),
    );
    body.add_or_replace_child(
        "upper_mouth",
        mouth((54.0, 54.0), (-5.0, -2.0, 0.0), (10.0, 4.0, 4.0), -0.001),
        Pose::offset(0.0, -2.51, 7.0),
    );
    body.add_or_replace_child(
        "inner_mouth",
        mouth((54.0, 70.0), (-3.0, -2.0, -0.5), (6.0, 4.0, 4.0), 0.0),
        Pose::offset(0.0, -0.51, 7.5),
    );
    body.add_or_replace_child(
        "lower_mouth",
        mouth((54.0, 62.0), (-5.0, -1.98, 0.0), (10.0, 4.0, 4.0), -0.001),
        Pose::offset(0.0, 1.49, 7.0),
    );
    LayerDefinition::new(r, 128.0, 128.0)
}

pub fn baby() -> LayerDefinition {
    let mut r = PartDefinition::root();
    let root = r.add_or_replace_child("root", vec![], Pose::offset(-0.5, 28.0, -0.5));
    root.add_or_replace_child(
        "shell",
        vec![
            Cube::new((0.0, 0.0), (-6.0, -4.0, -1.0), (7.0, 4.0, 7.0)),
            Cube::new((0.0, 11.0), (-6.0, 0.0, -1.0), (7.0, 4.0, 9.0)),
            Cube::new((23.0, 11.0), (-6.0, 0.0, 5.0), (7.0, 4.0, 0.0)),
        ],
        Pose::offset(3.0, -8.0, -2.0),
    );
    let body = root.add_or_replace_child(
        "body",
        vec![
            Cube::new((0.0, 24.0), (-2.5, -3.01, -1.0), (5.0, 4.0, 7.0)),
            Cube::new((0.0, 35.0), (-2.5, -3.01, 4.1), (5.0, 4.0, 0.0)),
        ],
        Pose::offset(0.5, -5.0, 3.0),
    );
    body.add_or_replace_child(
        "upper_mouth",
        mouth((24.0, 24.0), (-2.5, -1.0, 0.0), (5.0, 2.0, 2.0), -0.001),
        Pose::offset(0.0, -2.01, 3.9),
    );
    body.add_or_replace_child(
        "inner_mouth",
        mouth((24.0, 32.0), (-1.5, -1.0, -1.0), (3.0, 2.0, 2.0), 0.0),
        Pose::offset(0.0, -1.01, 4.9),
    );
    body.add_or_replace_child(
        "lower_mouth",
        mouth((24.0, 28.0), (-2.5, -1.0, 0.0), (5.0, 2.0, 2.0), -0.001),
        Pose::offset(0.0, -0.01, 3.9),
    );
    LayerDefinition::new(r, 64.0, 64.0)
}

#[derive(Clone, Copy, Debug)]
pub struct NautilusParts {
    body: PartId,
}

impl NautilusParts {
    pub fn find(model: &Model) -> Self {
        Self {
            body: model.part("body"),
        }
    }
}

/// `NautilusModel.setupAnim`'s turn of the body toward the look, and the
pub fn setup_anim(
    parts: &NautilusParts,
    swim: &KeyframeAnimation,
    pose: &mut ModelPose,
    state: &LivingRenderState,
) {
    let body = &mut pose[parts.body].pose;
    body.rotation.y = state.y_rot.clamp(-10.0, 10.0).to_radians();
    body.rotation.x = state.x_rot.clamp(-10.0, 10.0).to_radians();
    swim.apply_walk(
        pose,
        state.walk_position + state.age_in_ticks / 5.0,
        state.walk_speed + 0.2,
        2.0,
        3.0,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nautilus_pulses_even_standing_still_and_looks_a_little() {
        for layer in [adult(), baby()] {
            let model = Model::bake(&layer);
            let parts = NautilusParts::find(&model);
            let swim = KeyframeAnimation::bake(&model, WALK_ANIMATION).unwrap();
            let rest = model.initial_pose();
            let mut pose = model.initial_pose();
            let state = LivingRenderState {
                age_in_ticks: 7.0,
                y_rot: 45.0,
                ..Default::default()
            };
            setup_anim(&parts, &swim, &mut pose, &state);
            assert_ne!(pose, rest);
            assert!((pose[parts.body].pose.rotation.y - 10.0_f32.to_radians()).abs() < 1e-5);
        }
    }
}
