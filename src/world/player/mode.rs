//! The movement modes of the player and what picks them, the frame the

use super::PlayerState;

/// `LivingEntity.travel`'s branches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveMode {
    /// `travelInAir` on foot.
    Walk,
    /// Walking with the sprint flag (+30% speed, jump boost).
    Sprint,
    /// `travelInAir` on a ladder, vine or scaffolding (`handleOnClimbable`).
    Climb,
    /// `travelInWater` / `travelInLava`.
    Fluid,
    /// Sprint-swimming under water (`Player.travel`'s look steering).
    Swim,
    /// Creative or spectator flight (`Player.travel`'s `abilities.flying`).
    Fly,
    /// Elytra (`travelFallFlying`): see [`glide`].
    Glide,
    /// The riptide spin (`autoSpinAttackTicks`); see [`impulse`].
    SpinAttack,
}

/// Elytra constants of `LivingEntity.updateFallFlyingMovement`.
pub mod glide {
    /// Vertical lift fraction: `g * (-1 + cos(pitch)^2 * 0.75)`.
    pub const LIFT_FACTOR: f64 = 0.75;
    /// Pitch conversion of vertical speed into forward speed.
    pub const DIVE_CONVERSION: f64 = 0.1;
    /// Climb conversion when looking up.
    pub const CLIMB_CONVERSION: f64 = 0.04;
    pub const CLIMB_LIFT: f64 = 3.2;
    /// How fast velocity turns toward the look direction.
    pub const STEER: f64 = 0.1;
    /// Drag per axis (x, y, z).
    pub const DRAG: (f64, f64, f64) = (0.99_f32 as f64, 0.98_f32 as f64, 0.99_f32 as f64);
}

/// Impulse sources that ignore fall damage for a while
#[allow(dead_code)]
pub mod impulse {
    /// Riptide: `1.5 + 0.75 * (level - 1)` blocks of push; 20 spin ticks.
    pub fn riptide_strength(level: u32) -> f32 {
        1.5 + 0.75 * level.saturating_sub(1) as f32
    }
    pub const RIPTIDE_SPIN_TICKS: u32 = 20;
    /// A mace smash sets the vertical speed to this.
    pub const MACE_SMASH_VELOCITY_Y: f64 = 0.01;
    /// Spear lunge push per enchantment level, along the look direction.
    pub const LUNGE_PER_LEVEL: f32 = 0.458;
    /// Ticks the fall damage stays ignored after a lunge.
    pub const LUNGE_GRACE_TICKS: u32 = 10;
    /// Ticks the fall damage stays ignored after a mace smash.
    pub const MACE_GRACE_TICKS: u32 = 40;
}

impl PlayerState {
    /// The mode the next `travel` runs in. `fluid` is whether the body is
    pub fn move_mode(&self, fluid: bool) -> MoveMode {
        if self.abilities.flying {
            MoveMode::Fly
        } else if self.spin_ticks > 0 {
            MoveMode::SpinAttack
        } else if self.swimming {
            MoveMode::Swim
        } else if fluid {
            MoveMode::Fluid
        } else if self.gliding && !self.on_climbable {
            MoveMode::Glide
        } else if self.on_climbable {
            MoveMode::Climb
        } else if self.sprinting {
            MoveMode::Sprint
        } else {
            MoveMode::Walk
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn riptide_strength_grows_by_three_quarters_per_level() {
        assert_eq!(impulse::riptide_strength(1), 1.5);
        assert_eq!(impulse::riptide_strength(3), 3.0);
    }

    #[test]
    fn a_fresh_player_walks_and_a_fluid_beats_a_ladder() {
        let mut player = PlayerState::new(glam::DVec3::ZERO);
        assert_eq!(player.move_mode(false), MoveMode::Walk);
        player.on_climbable = true;
        assert_eq!(player.move_mode(false), MoveMode::Climb);
        assert_eq!(player.move_mode(true), MoveMode::Fluid);
    }
}
