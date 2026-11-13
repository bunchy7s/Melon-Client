//! Riding pipe: collect how the vehicle steers, drive it, seat the rider,

use super::{InWorld, melon_yaw, minecraft_yaw};
use crate::network::play::game::PlayerCommand;
use crate::world::entity::boat_sim::BoatInput;
use crate::world::entity::horse_sim::HorseInput;
use crate::world::entity::kind::EntityKind;
use crate::world::entity::ride::{JumpCharge, Steering};
use crate::world::inventory::OFFHAND_SLOT;
use crate::world::player::MoveInput;

/// What riding keeps between ticks.
#[derive(Default)]
pub struct RideState {
    pub jump: JumpCharge,
    /// The paddles the server was last told about.
    paddles: (bool, bool),
}

impl InWorld {
    /// How the vehicle takes the player's keys; `None` on foot.
    pub(super) fn steering(&self) -> Option<Steering> {
        let main = self.inventory.selected_stack();
        let off = &self.inventory.slots()[OFFHAND_SLOT];
        self.entities.own_steering(main, off)
    }

    /// Whether the inventory key opens the mount's inventory instead of the
    pub(super) fn rides_with_inventory(&self) -> bool {
        self.entities
            .own_vehicle_kind()
            .is_some_and(|kind| kind.is_equine() || kind == EntityKind::Nautilus)
    }

    /// `Keybinds` while riding: the inventory key asks the server for the
    pub(super) fn open_mount_inventory(&self) {
        self.joined
            .session
            .send_player_command(PlayerCommand::OpenInventory);
    }

    /// One riding tick, after the entities have moved: collect steering,
    pub(super) fn tick_ride(&mut self, input: MoveInput) {
        let steering = self.steering();
        self.drive_vehicle(input, steering);
        let riding = self.seat_on_vehicle(steering);
        self.send_ride_packets(input, riding);
    }

    /// The vehicle's own motion from the rider's keys (a boat's paddles,
    fn drive_vehicle(&mut self, input: MoveInput, steering: Option<Steering>) {
        let paddles = (steering == Some(Steering::Paddles))
            .then(|| Steering::Paddles.controls().filter(input))
            .map(|keys| BoatInput {
                left: keys.strafe < 0.0,
                right: keys.strafe > 0.0,
                up: keys.forward > 0.0,
                down: keys.forward < 0.0,
            });
        self.entities.drive_boat(&self.level, paddles);
        let horse = (steering == Some(Steering::Horse)).then(|| {
            let keys = Steering::Horse.controls().filter(input);
            HorseInput::from_keys(
                keys.forward,
                keys.strafe,
                minecraft_yaw(self.player.yaw()),
                self.player.pitch() * 0.5,
            )
        });
        self.entities.drive_horse(&self.level, horse);
    }

    /// The player stays on its seat (`Entity.positionRider`). `None` when
    fn seat_on_vehicle(&mut self, steering: Option<Steering>) -> Option<Steering> {
        let (steering, seat) = match (steering, self.entities.own_seat()) {
            (Some(steering), Some(seat)) => (steering, seat),
            _ => {
                if self.player.riding() {
                    self.player.dismount();
                }
                self.ride.jump.tick(false, false);
                self.status.jump_bar = None;
                return None;
            }
        };
        let body = self
            .entities
            .own_vehicle_body()
            .map(|(before, now)| (melon_yaw(before), melon_yaw(now)));
        self.player.ride_tick(seat, body);
        Some(steering)
    }

    /// Jump charge and paddle packets, then the HUD's jump bar.
    fn send_ride_packets(&mut self, input: MoveInput, steering: Option<Steering>) {
        let Some(steering) = steering else {
            return;
        };
        let keys = steering.controls().filter(input);
        if let Some(strength) = self.ride.jump.tick(keys.jump, steering.charges_jump()) {
            self.entities.horse_jump(strength);
            self.joined
                .session
                .send_player_command_with(PlayerCommand::StartRidingJump, strength);
        }
        self.send_paddles(steering, keys);
        self.status.jump_bar = self.jump_bar();
    }

    /// `AbstractBoat.controlBoat`'s paddle state, sent when it changes:
    fn send_paddles(&mut self, steering: Steering, keys: MoveInput) {
        let paddles = if steering == Steering::Paddles {
            let (left, right) = (keys.strafe < 0.0, keys.strafe > 0.0);
            let ahead = keys.forward > 0.0;
            (right && !left || ahead, left && !right || ahead)
        } else {
            (false, false)
        };
        if paddles != self.ride.paddles {
            self.ride.paddles = paddles;
            self.joined.session.send_paddle_boat(paddles.0, paddles.1);
        }
    }

    /// The mount, if it is a horse-like one, for the HUD's jump bar.
    pub(super) fn jump_bar(&self) -> Option<f32> {
        let kind: EntityKind = self.entities.own_vehicle_kind()?;
        (kind.is_equine() && self.steering() == Some(Steering::Horse))
            .then(|| self.ride.jump.scale())
    }
}
