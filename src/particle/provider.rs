//! `ParticleResources.registerProviders`: what each type becomes when

use glam::DVec3;

use super::animation::fade::{Glow, LifetimeAlpha, SizeCurve};
use super::base::{Atlas, Particle};
use super::behavior::{Behavior, Drip, DripStage, Leaf, Plume};
use super::motion::{self, emitter::Emitter};
use super::options::{ParticleData, ParticleSpec};
use super::{Kind, TickCtx};
use crate::world::fluid::FluidKind;
use crate::world::java_random::JavaRandom;

/// A new particle of `spec` at `pos`; `aux` is the packet's or caller's
pub fn create(
    spec: &ParticleSpec,
    pos: DVec3,
    aux: DVec3,
    ctx: &TickCtx,
    rng: &mut JavaRandom,
) -> Option<Particle> {
    use Kind as P;
    let kind = spec.kind;
    let sprites = ctx.sprites;
    let random_sprite = |rng: &mut JavaRandom| sprites.random(kind, rng.next_float());
    let p = match kind {
        P::Heart => heart(kind, pos, random_sprite(rng), rng),
        P::AngryVillager => heart(kind, pos + DVec3::Y * 0.5, random_sprite(rng), rng),
        P::Block | P::BlockCrumble | P::DustPillar => {
            let ParticleData::Block(state) = spec.data else {
                return None;
            };
            let mut p = terrain(kind, pos, aux, state, ctx, rng)?;
            if kind == P::BlockCrumble {
                p.vel = DVec3::ZERO;
                p.lifetime = rng.next_int(10) + 1;
            } else if kind == P::DustPillar {
                p.vel = DVec3::new(
                    rng.next_gaussian() / 30.0,
                    aux.y + rng.next_gaussian() / 2.0,
                    rng.next_gaussian() / 30.0,
                );
                p.lifetime = rng.next_int(20) + 20;
            }
            p
        }
        P::BlockMarker => {
            let ParticleData::Block(state) = spec.data else {
                return None;
            };
            let look = ctx.blocks?.particle_look(state)?;
            let mut p = Particle::moving(kind, pos, DVec3::ZERO, rng);
            p.look.uv = look.uv;
            p.look.translucent = look.translucent;
            p.gravity = 0.0;
            p.lifetime = 80;
            p.physics = false;
            p.quad_size = 0.5;
            p.look.size = SizeCurve::Marker;
            p
        }
        P::BubbleColumnUp => {
            let uv = random_sprite(rng);
            let mut p = Particle::at(kind, pos, rng);
            p.look.uv = uv;
            p.gravity = -0.125;
            p.friction = 0.85;
            p.width = 0.02;
            p.height = 0.02;
            p.quad_size *= rng.next_float() * 0.6 + 0.2;
            let mut drift =
                |a: f64| a * f64::from(0.2f32) + f64::from((rng.next_float() * 2.0 - 1.0) * 0.02);
            p.vel = DVec3::new(drift(aux.x), drift(aux.y), drift(aux.z));
            p.lifetime = (40.0 / (rng.next_float() * 0.8 + 0.2)) as i32;
            p.behavior = Behavior::InWater;
            p
        }
        P::CurrentDown => {
            let uv = random_sprite(rng);
            let mut p = Particle::at(kind, pos, rng);
            p.look.uv = uv;
            p.lifetime = (rng.next_float() * 60.0) as i32 + 30;
            p.physics = false;
            p.vel = DVec3::new(0.0, -0.05, 0.0);
            p.width = 0.02;
            p.height = 0.02;
            p.quad_size *= rng.next_float() * 0.6 + 0.2;
            p.gravity = 0.002;
            p.behavior = Behavior::CurrentDown { angle: 0.0 };
            p
        }
        P::Bubble => {
            let mut p = Particle::at(kind, pos, rng);
            p.look.uv = random_sprite(rng);
            p.width = 0.02;
            p.height = 0.02;
            p.quad_size *= rng.next_float() * 0.6 + 0.2;
            let mut drift = |a: f64| a * 0.2 + f64::from((rng.next_float() * 2.0 - 1.0) * 0.02);
            p.vel = DVec3::new(drift(aux.x), drift(aux.y), drift(aux.z));
            p.lifetime = (8.0 / (rng.next_float() * 0.8 + 0.2)) as i32;
            p.behavior = Behavior::Bubble;
            p
        }
        P::BubblePop => {
            let mut p = Particle::at(kind, pos, rng);
            p.lifetime = 4;
            p.gravity = 0.008;
            p.vel = aux;
            p.look.animated = true;
            p.behavior = Behavior::BubblePop;
            p
        }
        P::CampfireCosySmoke | P::CampfireSignalSmoke => {
            let mut p = Particle::at(kind, pos, rng);
            p.look.uv = random_sprite(rng);
            p.scale(3.0);
            p.width = 0.25;
            p.height = 0.25;
            p.lifetime = rng.next_int(50)
                + if kind == P::CampfireSignalSmoke {
                    280
                } else {
                    80
                };
            p.gravity = 3.0e-6;
            p.vel = DVec3::new(aux.x, aux.y + f64::from(rng.next_float() / 500.0), aux.z);
            p.look.translucent = true;
            p.color[3] = if kind == P::CampfireSignalSmoke {
                0.95
            } else {
                0.9
            };
            p.behavior = Behavior::Campfire;
            p
        }
        P::Cloud | P::Sneeze => {
            let mut p = Particle::moving(kind, pos, DVec3::ZERO, rng);
            p.friction = 0.96;
            p.vel = p.vel * f64::from(0.1f32) + aux;
            let col = 1.0 - rng.next_float() * 0.3;
            p.set_rgb([col; 3]);
            p.quad_size *= 1.875;
            let base = (8.0 / (rng.next_float() * 0.8 + 0.3)) as i32;
            p.lifetime = ((base as f32 * 2.5) as i32).max(1);
            p.physics = false;
            p.look.animated = true;
            p.look.translucent = true;
            p.look.size = SizeCurve::GrowIn;
            if kind == P::Sneeze {
                p.set_rgb([0.22, 1.0, 0.53]);
                p.color[3] = 0.4;
            }
            p.behavior = Behavior::PlayerCloud;
            p
        }
        P::Composter | P::Dolphin | P::EggCrack | P::HappyVillager | P::Mycelium => {
            let mut p = Particle::moving(kind, pos, aux, rng);
            p.look.uv = random_sprite(rng);
            let br = rng.next_float() * 0.1 + 0.2;
            p.set_rgb([br; 3]);
            p.width = 0.02;
            p.height = 0.02;
            p.quad_size *= rng.next_float() * 0.6 + 0.5;
            p.vel *= f64::from(0.02f32);
            p.lifetime = (20.0 / (rng.next_float() * 0.8 + 0.2)) as i32;
            p.behavior = Behavior::Town;
            match kind {
                P::Composter => {
                    p.set_rgb([1.0; 3]);
                    p.lifetime = 3 + rng.next_int(5);
                }
                P::Dolphin => {
                    p.set_rgb([0.3, 0.5, 1.0]);
                    p.color[3] = 1.0 - rng.next_float() * 0.7;
                    p.lifetime /= 2;
                }
                P::EggCrack | P::HappyVillager => p.set_rgb([1.0; 3]),
                _ => {}
            }
            p
        }
        P::Flame | P::SoulFireFlame | P::CopperFireFlame | P::SmallFlame => {
            let mut p = rising(kind, pos, aux, random_sprite(rng), rng);
            p.free_move = true;
            p.look.size = SizeCurve::Flame;
            p.look.glow = Glow::Emission;
            if kind == P::SmallFlame {
                p.scale(0.5);
            }
            p
        }
        P::Crit | P::DamageIndicator | P::EnchantedHit => {
            let aux = if kind == P::DamageIndicator {
                aux + DVec3::Y
            } else {
                aux
            };
            let mut p = Particle::moving(kind, pos, DVec3::ZERO, rng);
            p.look.uv = random_sprite(rng);
            p.friction = 0.7;
            p.gravity = 0.5;
            p.vel = p.vel * f64::from(0.1f32) + aux * 0.4;
            let col = rng.next_float() * 0.3 + 0.6;
            p.set_rgb([col; 3]);
            p.quad_size *= 0.75;
            p.lifetime = ((6.0 / (rng.next_float() * 0.8 + 0.6)) as i32).max(1);
            p.physics = false;
            p.look.size = SizeCurve::GrowIn;
            p.behavior = Behavior::Crit;
            p.base_tick(ctx);
            p.color[1] *= 0.96;
            p.color[2] *= 0.9;
            if kind == P::DamageIndicator {
                p.lifetime = 20;
            } else if kind == P::EnchantedHit {
                p.color[0] *= 0.3;
                p.color[1] *= 0.8;
            }
            p
        }
        P::DragonBreath => {
            let mut p = Particle::at(kind, pos, rng);
            p.friction = 0.96;
            p.vel = aux;
            p.set_rgb([
                lerp(rng.next_float(), 0.717_647_1, 0.874_509_8),
                0.0,
                lerp(rng.next_float(), 0.823_529_4, 0.976_470_6),
            ]);
            p.quad_size *= 0.75;
            p.lifetime = (20.0 / (rng.next_float() * 0.8 + 0.2)) as i32;
            p.physics = false;
            p.look.animated = true;
            p.look.size = SizeCurve::GrowIn;
            p.behavior = Behavior::DragonBreath { landed: false };
            if let ParticleData::Power(power) = spec.data {
                p.set_power(power);
            }
            p
        }
        P::DrippingLava
        | P::FallingLava
        | P::LandingLava
        | P::DrippingWater
        | P::FallingWater
        | P::DrippingHoney
        | P::FallingHoney
        | P::LandingHoney
        | P::FallingNectar
        | P::FallingSporeBlossom
        | P::DrippingObsidianTear
        | P::FallingObsidianTear
        | P::LandingObsidianTear
        | P::DrippingDripstoneLava
        | P::FallingDripstoneLava
        | P::DrippingDripstoneWater
        | P::FallingDripstoneWater => drip(kind, pos, random_sprite(rng), rng),
        P::Dust | P::DustColorTransition => {
            let (rgb, scale) = match spec.data {
                ParticleData::Dust { rgb, scale } => (rgb, scale),
                ParticleData::DustTransition { from, scale, .. } => (from, scale),
                _ => (0xFF0000, 1.0),
            };
            let mut p = Particle::moving(kind, pos, aux, rng);
            p.friction = 0.96;
            p.speed_up_when_blocked = true;
            p.vel *= f64::from(0.1f32);
            p.quad_size *= 0.75 * scale;
            let base = (8.0 / (rng.next_double() * 0.8 + 0.2)) as i32;
            p.lifetime = ((base as f32 * scale) as i32).max(1);
            p.look.animated = true;
            p.look.size = SizeCurve::GrowIn;
            let base = rng.next_float() * 0.4 + 0.6;
            let mut shade = |c: f32| (rng.next_float() * 0.2 + 0.8) * c * base;
            let [r, g, b] = rgb_of(rgb);
            p.set_rgb([shade(r), shade(g), shade(b)]);
            if let ParticleData::DustTransition { from, to, .. } = spec.data {
                let [fr, fg, fb] = rgb_of(from);
                let [tr, tg, tb] = rgb_of(to);
                let from = [shade(fr), shade(fg), shade(fb)];
                let to = [shade(tr), shade(tg), shade(tb)];
                p.set_rgb(from);
                p.behavior = Behavior::DustTransition { from, to };
            }
            p
        }
        P::Effect
        | P::InstantEffect
        | P::EntityEffect
        | P::Witch
        | P::Infested
        | P::RaidOmen
        | P::TrialOmen => {
            let mut p = Particle::moving(
                kind,
                pos,
                DVec3::new(0.5 - rng.next_double(), aux.y, 0.5 - rng.next_double()),
                rng,
            );
            p.friction = 0.96;
            p.gravity = -0.1;
            p.speed_up_when_blocked = true;
            p.vel.y *= f64::from(0.2f32);
            if aux.x == 0.0 && aux.z == 0.0 {
                p.vel.x *= f64::from(0.1f32);
                p.vel.z *= f64::from(0.1f32);
            }
            p.quad_size *= 0.75;
            p.lifetime = (8.0 / (rng.next_float() * 0.8 + 0.2)) as i32;
            p.physics = false;
            p.look.animated = true;
            p.look.translucent = true;
            match spec.data {
                ParticleData::Spell { rgb, power } => {
                    p.set_rgb(rgb_of(rgb));
                    p.set_power(power);
                }
                ParticleData::Color(argb) => {
                    p.set_rgb(rgb_of(argb));
                    p.color[3] = (argb >> 24) as f32 / 255.0;
                }
                _ if kind == P::Witch => {
                    let br = rng.next_float() * 0.5 + 0.35;
                    p.set_rgb([br, 0.0, br]);
                }
                _ => {}
            }
            p.behavior = Behavior::Spell { alpha: p.color[3] };
            if ctx.close_to_scope(p.pos) {
                p.color[3] = 0.0;
            }
            p
        }
        P::Enchant | P::Nautilus | P::VaultConnection => {
            let mut p = Particle::at(kind, pos + aux, rng);
            p.prev = pos + aux;
            p.look.uv = random_sprite(rng);
            p.vel = aux;
            p.quad_size = 0.1 * (rng.next_float() * 0.5 + 0.2);
            let br = rng.next_float() * 0.6 + 0.4;
            p.set_rgb([0.9 * br, 0.9 * br, br]);
            p.physics = false;
            p.free_move = true;
            p.lifetime = (rng.next_float() * 10.0) as i32 + 30;
            p.look.glow = Glow::EmissionQuartic;
            p.behavior = Behavior::FlyTowards { start: pos };
            if kind == P::VaultConnection {
                p.alpha_curve = LifetimeAlpha {
                    start: 0.0,
                    end: 0.6,
                    from: 0.25,
                    to: 1.0,
                };
                p.color[3] = 0.0;
                p.look.translucent = true;
                p.look.glow = Glow::Block;
                p.scale(1.5);
            }
            p
        }
        P::EndRod | P::TotemOfUndying => {
            let mut p = Particle::at(kind, pos, rng);
            p.friction = if kind == P::TotemOfUndying { 0.6 } else { 0.91 };
            p.gravity = if kind == P::TotemOfUndying {
                1.25
            } else {
                0.0125
            };
            p.vel = aux;
            p.quad_size *= 0.75;
            p.lifetime = 60 + rng.next_int(12);
            p.look.animated = true;
            p.look.translucent = true;
            p.look.glow = Glow::Full;
            let fade = if kind == P::EndRod {
                p.free_move = true;
                Some(rgb_of(15_916_745))
            } else {
                let rgb = if rng.next_int(4) == 0 {
                    [
                        0.6 + rng.next_float() * 0.2,
                        0.6 + rng.next_float() * 0.3,
                        rng.next_float() * 0.2,
                    ]
                } else {
                    [
                        0.1 + rng.next_float() * 0.2,
                        0.4 + rng.next_float() * 0.3,
                        rng.next_float() * 0.2,
                    ]
                };
                p.set_rgb(rgb);
                None
            };
            p.behavior = Behavior::Animated { fade };
            p
        }
        P::ExplosionEmitter => {
            let mut p = Particle::at(kind, pos, rng);
            p.lifetime = 8;
            p.behavior = Behavior::Emitter(Emitter::ExplosionSeed);
            p
        }
        P::Explosion | P::SonicBoom => {
            let mut p = Particle::moving(kind, pos, DVec3::ZERO, rng);
            p.lifetime = 6 + rng.next_int(4);
            let col = rng.next_float() * 0.6 + 0.4;
            p.set_rgb([col; 3]);
            p.quad_size = 2.0 * (1.0 - aux.x as f32 * 0.5);
            if kind == P::SonicBoom {
                p.lifetime = 16;
                p.quad_size = 1.5;
            }
            p.look.animated = true;
            p.look.glow = Glow::Full;
            p.behavior = Behavior::Still;
            p
        }
        P::SweepAttack => {
            let mut p = Particle::moving(kind, pos, DVec3::ZERO, rng);
            p.lifetime = 4;
            let col = rng.next_float() * 0.6 + 0.4;
            p.set_rgb([col; 3]);
            p.quad_size = 1.0 - aux.x as f32 * 0.5;
            p.look.animated = true;
            p.look.glow = Glow::Full;
            p.behavior = Behavior::Still;
            p
        }
        P::FallingDust => {
            let ParticleData::Block(state) = spec.data else {
                return None;
            };
            let at = pos.floor().as_ivec3();
            let [r, g, b] = dust_color(ctx, state, [at.x, at.y, at.z]);
            let mut p = Particle::at(kind, pos, rng);
            p.set_rgb([r, g, b]);
            p.quad_size *= 0.674_999_95;
            let base = (32.0 / (rng.next_float() * 0.8 + 0.2)) as i32;
            p.lifetime = ((base as f32 * 0.9) as i32).max(1);
            p.look.animated = true;
            p.look.size = SizeCurve::GrowIn;
            let spin = (rng.next_float() - 0.5) * 0.1;
            p.roll = rng.next_float() * crate::math::angle::TAU;
            p.prev_roll = p.roll;
            p.behavior = Behavior::FallingDust { spin };
            p
        }
        P::Glow | P::WaxOn | P::WaxOff | P::ElectricSpark | P::Scrape => {
            let start = if kind == P::Glow {
                DVec3::new(0.5 - rng.next_double(), aux.y, 0.5 - rng.next_double())
            } else {
                DVec3::ZERO
            };
            let mut p = Particle::moving(kind, pos, start, rng);
            p.friction = 0.96;
            p.speed_up_when_blocked = true;
            p.quad_size *= 0.75;
            p.physics = false;
            p.look.animated = true;
            p.look.glow = Glow::Emission;
            match kind {
                P::Glow => {
                    p.set_rgb(if rng.next_boolean() {
                        [0.6, 1.0, 0.8]
                    } else {
                        [0.08, 0.4, 0.4]
                    });
                    p.vel.y *= f64::from(0.2f32);
                    if aux.x == 0.0 && aux.z == 0.0 {
                        p.vel.x *= f64::from(0.1f32);
                        p.vel.z *= f64::from(0.1f32);
                    }
                    p.lifetime = (8.0 / (rng.next_double() * 0.8 + 0.2)) as i32;
                }
                P::ElectricSpark => {
                    p.set_rgb([1.0, 0.9, 1.0]);
                    p.vel = aux * 0.25;
                    p.lifetime = rng.next_int(2) + 2;
                }
                P::Scrape => {
                    p.set_rgb(if rng.next_boolean() {
                        [0.29, 0.58, 0.51]
                    } else {
                        [0.43, 0.77, 0.62]
                    });
                    p.vel = aux * 0.01;
                    p.lifetime = rng.next_int(30) + 10;
                }
                _ => {
                    p.set_rgb(if kind == P::WaxOn {
                        [0.91, 0.55, 0.08]
                    } else {
                        [1.0, 0.9, 1.0]
                    });
                    p.vel = DVec3::new(aux.x * 0.01 / 2.0, aux.y * 0.01, aux.z * 0.01 / 2.0);
                    p.lifetime = rng.next_int(30) + 10;
                }
            }
            p
        }
        P::Item | P::ItemSlime | P::ItemCobweb | P::ItemSnowball | P::SulfurCubeGoo => {
            let item = match (kind, spec.data) {
                (P::Item, ParticleData::Item(item)) => Some(item),
                (P::ItemSlime, _) => Some(pumpkin_data::item::Item::SLIME_BALL.id),
                (P::ItemCobweb, _) => Some(pumpkin_data::item::Item::COBWEB.id),
                (P::ItemSnowball, _) => Some(pumpkin_data::item::Item::SNOWBALL.id),
                _ => None,
            };
            let mut p = Particle::moving(kind, pos, DVec3::ZERO, rng);
            p.gravity = 1.0;
            p.quad_size /= 2.0;
            if kind == P::Item {
                p.vel = p.vel * f64::from(0.1f32) + aux;
            }
            let (atlas, whole) = match item {
                Some(item) => (Atlas::Item(item), [0.0, 0.0, 1.0, 1.0]),
                None => (Atlas::Blocks, sprites.first(kind)),
            };
            p.look.atlas = atlas;
            p.look.uv = quarter(whole, rng);
            p
        }
        P::LargeSmoke | P::Smoke | P::WhiteSmoke => {
            let scale = if kind == P::LargeSmoke { 2.5 } else { 1.0 };
            let mut p = ash_smoke(kind, pos, [0.1; 3], aux, scale, 0.3, 8, -0.1, true, rng);
            if kind == P::WhiteSmoke {
                p.set_rgb([0.729_411_8, 0.694_117_67, 0.760_784_3]);
            }
            p
        }
        P::Ash => ash_smoke(
            kind,
            pos,
            [0.1, -0.1, 0.1],
            DVec3::ZERO,
            1.0,
            0.5,
            20,
            0.1,
            false,
            rng,
        ),
        P::WhiteAsh => {
            let xa = f64::from(rng.next_float()) * -1.9 * f64::from(rng.next_float()) * 0.1;
            let ya = f64::from(rng.next_float()) * -0.5 * f64::from(rng.next_float()) * 0.1 * 5.0;
            let za = f64::from(rng.next_float()) * -1.9 * f64::from(rng.next_float()) * 0.1;
            let mut p = ash_smoke(
                kind,
                pos,
                [0.1, -0.1, 0.1],
                DVec3::new(xa, ya, za),
                1.0,
                0.0,
                20,
                0.0125,
                false,
                rng,
            );
            p.set_rgb(rgb_of(12_235_202));
            p
        }
        P::Lava => {
            let mut p = Particle::moving(kind, pos, DVec3::ZERO, rng);
            p.look.uv = random_sprite(rng);
            p.gravity = 0.75;
            p.friction = 0.999;
            p.vel *= f64::from(0.8f32);
            p.vel.y = f64::from(rng.next_float() * 0.4 + 0.05);
            p.quad_size *= rng.next_float() * 2.0 + 0.2;
            p.lifetime = (16.0 / (rng.next_float() * 0.8 + 0.2)) as i32;
            p.look.size = SizeCurve::Lava;
            p.look.glow = Glow::Block;
            p.behavior = Behavior::Lava;
            p
        }
        P::Note => {
            let mut p = Particle::moving(kind, pos, DVec3::ZERO, rng);
            p.look.uv = random_sprite(rng);
            p.friction = 0.66;
            p.speed_up_when_blocked = true;
            p.vel *= f64::from(0.01f32);
            p.vel.y += 0.2;
            let note = aux.x as f32;
            let channel = |offset: f32| {
                ((note + offset) * crate::math::angle::TAU)
                    .sin()
                    .mul_add(0.65, 0.35)
                    .max(0.0)
            };
            p.set_rgb([channel(0.0), channel(1.0 / 3.0), channel(2.0 / 3.0)]);
            p.quad_size *= 1.5;
            p.lifetime = 6;
            p.look.size = SizeCurve::GrowIn;
            p
        }
        P::Poof | P::Spit => {
            let mut p = Particle::at(kind, pos, rng);
            p.gravity = if kind == P::Spit { 0.5 } else { -0.1 };
            p.friction = 0.9;
            let mut wobble = || f64::from((rng.next_float() * 2.0 - 1.0) * 0.05);
            p.vel = DVec3::new(aux.x + wobble(), aux.y + wobble(), aux.z + wobble());
            let col = rng.next_float() * 0.3 + 0.7;
            p.set_rgb([col; 3]);
            p.quad_size = 0.1 * (rng.next_float() * rng.next_float() * 6.0 + 1.0);
            p.lifetime = (16.0 / (rng.next_float() * 0.8 + 0.2)) as i32 + 2;
            p.look.animated = true;
            p
        }
        P::Portal | P::ReversePortal => {
            let mut p = Particle::at(kind, pos, rng);
            p.look.uv = random_sprite(rng);
            p.vel = aux;
            p.quad_size = 0.1 * (rng.next_float() * 0.2 + 0.5);
            let br = rng.next_float() * 0.6 + 0.4;
            p.set_rgb([br * 0.9, br * 0.3, br]);
            p.lifetime = (rng.next_float() * 10.0) as i32 + 40;
            p.free_move = true;
            p.look.size = SizeCurve::Portal;
            p.look.glow = Glow::EmissionQuartic;
            p.behavior = Behavior::Portal { start: pos };
            if kind == P::ReversePortal {
                p.quad_size *= 1.5;
                p.lifetime = (rng.next_float() * 2.0) as i32 + 60;
                p.look.size = SizeCurve::ReversePortal;
                p.behavior = Behavior::ReversePortal;
            }
            p
        }
        P::Rain | P::Splash => {
            let mut p = Particle::moving(kind, pos, DVec3::ZERO, rng);
            p.look.uv = random_sprite(rng);
            p.vel.x *= f64::from(0.3f32);
            p.vel.y = f64::from(rng.next_float() * 0.2 + 0.1);
            p.vel.z *= f64::from(0.3f32);
            p.width = 0.01;
            p.height = 0.01;
            p.gravity = 0.06;
            p.lifetime = (8.0 / (rng.next_float() * 0.8 + 0.2)) as i32;
            p.behavior = Behavior::WaterDrop;
            if kind == P::Splash {
                p.gravity = 0.04;
                if aux.y == 0.0 && (aux.x != 0.0 || aux.z != 0.0) {
                    p.vel = DVec3::new(aux.x, 0.1, aux.z);
                }
            }
            p
        }
        P::SquidInk | P::GlowSquidInk => {
            let mut p = Particle::at(kind, pos, rng);
            p.friction = 0.92;
            p.quad_size = 0.5;
            p.set_rgb(if kind == P::GlowSquidInk {
                [0.2, 0.8, 0.6]
            } else {
                [0.0; 3]
            });
            p.lifetime = (p.quad_size * 12.0 / (rng.next_float() * 0.8 + 0.2)) as i32;
            p.physics = false;
            p.vel = aux;
            p.look.animated = true;
            p.look.translucent = true;
            p.look.glow = Glow::Full;
            p.behavior = Behavior::SquidInk;
            p
        }
        P::Underwater | P::CrimsonSpore | P::WarpedSpore | P::SporeBlossomAir => {
            let at = pos - DVec3::Y * 0.125;
            let mut p = match kind {
                P::Underwater => Particle::at(kind, at, rng),
                P::CrimsonSpore => {
                    let vel = DVec3::new(
                        rng.next_gaussian() * 1.0e-6,
                        rng.next_gaussian() * 1.0e-4,
                        rng.next_gaussian() * 1.0e-6,
                    );
                    Particle::moving(kind, at, vel, rng)
                }
                P::WarpedSpore => {
                    let ya = f64::from(rng.next_float()) * -1.9 * f64::from(rng.next_float()) * 0.1;
                    Particle::moving(kind, at, DVec3::new(0.0, ya, 0.0), rng)
                }
                _ => Particle::moving(kind, at, DVec3::new(0.0, f64::from(-0.8f32), 0.0), rng),
            };
            p.look.uv = random_sprite(rng);
            p.width = 0.01;
            p.height = 0.01;
            let grow = if kind == P::Underwater { 0.2 } else { 0.6 };
            p.quad_size *= rng.next_float() * 0.6 + grow;
            p.lifetime = (16.0 / (rng.next_float() * 0.8 + 0.2)) as i32;
            p.physics = false;
            p.friction = 1.0;
            p.gravity = 0.0;
            match kind {
                P::Underwater => p.set_rgb([0.4, 0.4, 0.7]),
                P::CrimsonSpore => p.set_rgb([0.9, 0.4, 0.5]),
                P::WarpedSpore => {
                    p.set_rgb([0.1, 0.1, 0.3]);
                    p.width = 0.001;
                    p.height = 0.001;
                }
                _ => {
                    p.lifetime = 500 + rng.next_int(501);
                    p.gravity = 0.01;
                    p.set_rgb([0.32, 0.5, 0.22]);
                }
            }
            p
        }
        P::Snowflake => {
            let mut p = Particle::at(kind, pos, rng);
            p.gravity = 0.225;
            p.friction = 1.0;
            let mut wobble = || f64::from((rng.next_float() * 2.0 - 1.0) * 0.05);
            p.vel = DVec3::new(aux.x + wobble(), aux.y + wobble(), aux.z + wobble());
            p.quad_size = 0.1 * (rng.next_float() * rng.next_float() + 1.0);
            p.lifetime = (16.0 / (rng.next_float() * 0.8 + 0.2)) as i32 + 2;
            p.look.animated = true;
            p.set_rgb([0.923, 0.964, 0.999]);
            p.behavior = Behavior::Snowflake;
            p
        }
        P::Trail => {
            let ParticleData::Trail { target, rgb, ticks } = spec.data else {
                return None;
            };
            let mut p = Particle::moving(kind, pos, aux, rng);
            p.look.uv = random_sprite(rng);
            let mut vary = |c: f32| (c * (0.875 + rng.next_float() * 0.25)).min(1.0);
            let [r, g, b] = rgb_of(rgb);
            p.set_rgb([vary(r), vary(g), vary(b)]);
            p.quad_size = 0.26;
            p.lifetime = ticks;
            p.look.glow = Glow::Full;
            p.behavior = Behavior::Trail {
                target: DVec3::from_array(target),
            };
            p
        }
        P::Vibration => {
            let ParticleData::Vibration { to, ticks } = spec.data else {
                return None;
            };
            let mut p = Particle::moving(kind, pos, DVec3::ZERO, rng);
            p.look.uv = random_sprite(rng);
            p.quad_size = 0.3;
            p.lifetime = ticks;
            p.look.translucent = true;
            p.look.glow = Glow::Block;
            let (rot, pitch) = motion::destination(to, ctx)
                .map_or((0.0, 0.0), |target| motion::heading(pos, target));
            p.behavior = Behavior::Vibration {
                to,
                rot,
                prev_rot: rot,
                pitch,
                prev_pitch: pitch,
            };
            p
        }
        P::PauseMobGrowth | P::ResetMobGrowth => {
            let mut p = Particle::moving(kind, pos, aux, rng);
            p.look.uv = random_sprite(rng);
            p.vel = aux;
            p.gravity = 0.0;
            p.vel.y += if kind == P::ResetMobGrowth {
                0.03
            } else {
                -0.03
            };
            p.quad_size *= rng.next_float() * 0.6 + 0.5;
            p.lifetime = 8;
            p
        }
        P::Soul | P::SculkSoul => {
            let mut p = rising(kind, pos, aux, sprites.first(kind), rng);
            p.scale(1.5);
            p.look.animated = true;
            p.look.translucent = true;
            if kind == P::SculkSoul {
                p.look.glow = Glow::Block;
            }
            p
        }
        P::SulfurBubbles => {
            let uv = random_sprite(rng);
            let mut p = Particle::at(kind, pos, rng);
            p.look.uv = uv;
            p.gravity = -0.04;
            p.friction = 0.85;
            p.width = 0.02;
            p.height = 0.02;
            let mut push =
                |a: f64| a * f64::from(0.2f32) + f64::from((rng.next_float() * 2.0 - 1.0) * 0.02);
            p.vel.x = push(aux.x);
            p.vel.z = push(aux.y);
            let size_start = 0.02 + 0.02 * rng.next_float();
            p.quad_size = size_start;
            p.lifetime = i32::MAX;
            p.behavior = Behavior::SulfurBubble {
                y_start: pos.y,
                y_prev: pos.y,
                size_start,
            };
            p
        }
        P::NoxiousGas => {
            let mut p = ash_smoke(kind, pos, [0.1; 3], aux, 3.0, 0.3, 5, -0.02, true, rng);
            p.set_rgb([1.0; 3]);
            p.lifetime = (6.0 / (rng.next_float() * 0.5 + 0.5) as f64 * 3.0) as i32;
            p.look.translucent = true;
            p.behavior = Behavior::NoxiousGas;
            p
        }
        P::NoxiousGasCloud => {
            let mut p = Particle::at(kind, pos, rng);
            p.lifetime = 20;
            p.behavior = Behavior::Emitter(Emitter::NoxiousGasCloud);
            p
        }
        P::Geyser => {
            let ParticleData::Geyser { water_blocks } = spec.data else {
                return None;
            };
            let mut p = Particle::at(kind, pos, rng);
            p.lifetime = 20;
            p.behavior = Behavior::Emitter(Emitter::Geyser {
                aux: aux.as_vec3().to_array(),
                water_blocks,
            });
            p
        }
        P::GeyserBase | P::GeyserPoof => {
            let ParticleData::GeyserBase {
                water_blocks,
                impulse,
            } = spec.data
            else {
                return None;
            };
            let mut off = |spread: f32| f64::from((rng.next_float() - 0.5) * spread);
            let at = pos + DVec3::new(off(0.5), off(0.5) + f64::from(0.2f32), off(0.5));
            let burst = impulse + 0.25 * water_blocks as f32;
            let size = 3.0 + 0.125 * water_blocks as f32;
            let mut p = ash_smoke(kind, at, [burst; 3], aux, size, 0.0, 0, 0.0, true, rng);
            p.friction = 0.725;
            p.set_rgb([1.0; 3]);
            p.vel.y = p.vel.y.abs();
            p.lifetime = (25.0 * (0.8 + 0.2 * rng.next_float())) as i32;
            p
        }
        P::GeyserPlume => {
            let ParticleData::Geyser { water_blocks } = spec.data else {
                return None;
            };
            let x = f64::from((rng.next_float() - 0.5) * 0.2);
            let y = f64::from(rng.next_float());
            let z = f64::from((rng.next_float() - 0.5) * 0.2);
            let mut p = Particle::moving(kind, pos + DVec3::new(x, y, z), aux, rng);
            let height = 5 * water_blocks.max(1);
            p.speed_up_when_blocked = true;
            p.lifetime = height * 5;
            p.vel.y = 0.0;
            let spray = [
                (rng.next_float() - 0.5) * 0.2,
                (rng.next_float() - 0.5) * 0.2,
            ];
            p.friction = 1.0;
            let propulsion = if water_blocks == 1 { 1.5 } else { 1.0 } * height as f32 * 1.45;
            p.gravity = -propulsion;
            let randomized = p.quad_size * 0.75;
            let min_size = randomized * (2.0 + height as f32 / 8.0);
            let max_size = randomized * (3.0 + height as f32 / 8.0);
            p.quad_size = min_size;
            p.look.animated = true;
            p.behavior = Behavior::GeyserPlume(Plume {
                start_y: p.pos.y,
                height: height as f32,
                propulsion,
                spray,
                min_size,
                max_size,
                done: false,
            });
            p
        }
        P::Gust | P::SmallGust => {
            let mut p = Particle::at(kind, pos, rng);
            p.lifetime = 12 + rng.next_int(4);
            p.quad_size = 1.0;
            p.width = 1.0;
            p.height = 1.0;
            p.look.animated = true;
            p.look.glow = Glow::Full;
            p.behavior = Behavior::Still;
            if kind == P::SmallGust {
                p.scale(0.15);
            }
            p
        }
        P::GustEmitterLarge | P::GustEmitterSmall => {
            let mut p = Particle::at(kind, pos, rng);
            let (scale, lifetime, delay) = if kind == P::GustEmitterLarge {
                (3.0, 7, 0)
            } else {
                (1.0, 3, 2)
            };
            p.lifetime = lifetime;
            p.behavior = Behavior::Emitter(Emitter::GustSeed { scale, delay });
            p
        }
        P::Firework => {
            let mut p = Particle::at(kind, pos, rng);
            p.friction = 0.91;
            p.gravity = 0.1;
            p.vel = aux;
            p.quad_size *= 0.75;
            p.lifetime = 48 + rng.next_int(12);
            p.color[3] = 0.99;
            p.look.animated = true;
            p.look.translucent = true;
            p.look.glow = Glow::Full;
            p.behavior = Behavior::Animated { fade: None };
            p
        }
        P::Flash => {
            let uv = random_sprite(rng);
            let mut p = Particle::at(kind, pos, rng);
            p.look.uv = uv;
            p.lifetime = 4;
            p.quad_size = 1.0;
            if let ParticleData::Color(argb) = spec.data {
                p.set_rgb(rgb_of(argb));
            }
            p.alpha_curve = LifetimeAlpha {
                start: 0.725,
                end: 0.225,
                from: 0.0,
                to: 1.0,
            };
            p.look.translucent = true;
            p.look.size = SizeCurve::Flash;
            p
        }
        P::Fishing => {
            let mut p = Particle::moving(kind, pos, DVec3::ZERO, rng);
            p.vel.x *= f64::from(0.3f32);
            p.vel.y = f64::from(rng.next_float() * 0.2 + 0.1);
            p.vel.z *= f64::from(0.3f32);
            p.width = 0.01;
            p.height = 0.01;
            p.lifetime = (8.0 / (rng.next_float() * 0.8 + 0.2)) as i32;
            p.gravity = 0.0;
            p.vel = aux;
            p.look.uv = sprites.from_age(kind, p.age, p.lifetime);
            p.behavior = Behavior::Wake;
            p
        }
        P::CherryLeaves
        | P::PaleOakLeaves
        | P::RedPoplarLeaves
        | P::OrangePoplarLeaves
        | P::YellowPoplarLeaves
        | P::TintedLeaves => {
            let (fall, side, swirl, flow_away, scale, start): (f32, f32, bool, bool, f32, f32) =
                if kind == P::CherryLeaves {
                    (0.25, 2.0, false, true, 1.0, 0.0)
                } else {
                    (0.07, 10.0, true, false, 2.0, 0.021)
                };
            let uv = random_sprite(rng);
            let mut p = Particle::at(kind, pos, rng);
            p.look.uv = uv;
            let rot_speed = (if rng.next_boolean() { -30.0f32 } else { 30.0 }).to_radians();
            let spin = (if rng.next_boolean() { -5.0f32 } else { 5.0 }).to_radians();
            p.lifetime = 300;
            p.gravity = fall * 1.2 * 0.0025;
            let size = scale * if rng.next_boolean() { 0.05 } else { 0.075 };
            p.quad_size = size;
            p.width = size;
            p.height = size;
            p.friction = 1.0;
            p.vel.y = -f64::from(start);
            let r = rng.next_float();
            let flow_turn = f64::from(r * 60.0).to_radians();
            p.behavior = Behavior::Leaf(Leaf {
                rot_speed,
                spin,
                wind: side,
                swirl,
                flow_away,
                flow: [
                    (flow_turn.cos() * f64::from(side)) as f32,
                    (flow_turn.sin() * f64::from(side)) as f32,
                ],
                swirl_period: (1000.0 + r * 3000.0).to_radians(),
            });
            if let ParticleData::Color(argb) = spec.data {
                p.set_rgb(rgb_of(argb));
            }
            p
        }
        P::SculkCharge | P::SculkChargePop => {
            let mut p = Particle::moving(kind, pos, aux, rng);
            p.friction = 0.96;
            p.scale(if kind == P::SculkCharge { 1.5 } else { 1.0 });
            p.physics = false;
            p.vel = aux;
            if let ParticleData::SculkCharge(roll) = spec.data {
                p.roll = roll;
                p.prev_roll = roll;
            }
            p.lifetime = if kind == P::SculkCharge {
                rng.next_int(12) + 8
            } else {
                rng.next_int(4) + 6
            };
            p.look.animated = true;
            p.look.translucent = true;
            p.look.glow = Glow::Block;
            p
        }
        P::Shriek => {
            let ParticleData::Shriek(delay) = spec.data else {
                return None;
            };
            let uv = random_sprite(rng);
            let mut p = Particle::moving(kind, pos, DVec3::ZERO, rng);
            p.look.uv = uv;
            p.quad_size = 0.85;
            p.lifetime = 30;
            p.gravity = 0.0;
            p.vel = DVec3::new(0.0, 0.1, 0.0);
            p.alpha_curve = LifetimeAlpha {
                start: 1.0,
                end: 0.0,
                from: 0.0,
                to: 1.0,
            };
            p.look.translucent = true;
            p.look.glow = Glow::Block;
            p.look.size = SizeCurve::Shriek;
            p.behavior = Behavior::Shriek { delay };
            p
        }
        P::DustPlume => {
            let aux = aux + DVec3::Y * f64::from(0.15f32);
            let mut p = ash_smoke(
                kind,
                pos,
                [0.7, 0.6, 0.7],
                aux,
                1.0,
                0.5,
                7,
                0.5,
                false,
                rng,
            );
            let shift = rng.next_float() * 0.2;
            p.set_rgb(rgb_of(12_235_202).map(|c| c - shift));
            p.behavior = Behavior::DustPlume;
            p
        }
        P::TrialSpawnerDetection | P::TrialSpawnerDetectionOminous => {
            const SCALE: f32 = 1.5;
            let mut p = Particle::moving(kind, pos, DVec3::ZERO, rng);
            p.friction = 0.96;
            p.gravity = -0.1;
            p.speed_up_when_blocked = true;
            p.vel = DVec3::new(0.0, p.vel.y * 0.9, 0.0) + aux;
            p.quad_size *= 0.75 * SCALE;
            let between = rng.next_float() * 0.5 + 0.5;
            p.lifetime = ((8.0 / between * SCALE) as i32).max(1);
            p.look.animated = true;
            p.look.glow = Glow::Block;
            p.look.facing = super::base::Facing::Upright;
            p.look.size = SizeCurve::GrowIn;
            p
        }
        P::OminousSpawning => {
            let uv = random_sprite(rng);
            let mut p = Particle::at(kind, pos, rng);
            p.look.uv = uv;
            p.vel = aux;
            p.pos = pos + aux;
            p.prev = p.pos;
            p.quad_size = 0.1 * (rng.next_float() * 0.5 + 0.2);
            p.physics = false;
            p.free_move = true;
            p.lifetime = (rng.next_float() * 5.0) as i32 + 25;
            p.look.glow = Glow::Block;
            p.behavior = Behavior::FlyStraight {
                start: pos,
                from: 0xFF45_AEFE,
                to: 0xFFFF_FFFF,
            };
            p.scale(rng.next_float() * 2.0 + 3.0);
            p
        }
        P::Firefly => {
            let xa = 0.5 - rng.next_double();
            let ya = if rng.next_boolean() { aux.y } else { -aux.y };
            let za = 0.5 - rng.next_double();
            let uv = random_sprite(rng);
            let mut p = Particle::moving(kind, pos, DVec3::new(xa, ya, za), rng);
            p.look.uv = uv;
            p.speed_up_when_blocked = true;
            p.friction = 0.96;
            p.quad_size *= 0.75;
            p.vel *= f64::from(0.8f32);
            p.lifetime = 200 + rng.next_int(101);
            p.scale(1.5);
            p.color[3] = 0.0;
            p.look.translucent = true;
            p.look.glow = Glow::Firefly;
            p.behavior = Behavior::Firefly;
            p
        }
        P::ElderGuardian => return None,
    };
    let mut p = p;
    if p.look.animated {
        p.look.uv = sprites.from_age(kind, p.age, p.lifetime);
    }
    Some(p)
}

/// `HeartParticle`: rises a little and slows.
fn heart(kind: Kind, pos: DVec3, uv: [f32; 4], rng: &mut JavaRandom) -> Particle {
    let mut p = Particle::moving(kind, pos, DVec3::ZERO, rng);
    p.look.uv = uv;
    p.speed_up_when_blocked = true;
    p.friction = 0.86;
    p.vel *= f64::from(0.01f32);
    p.vel.y += 0.1;
    p.quad_size *= 1.5;
    p.lifetime = 16;
    p.physics = false;
    p.look.size = SizeCurve::GrowIn;
    p
}

/// `RisingParticle`: barely thrown, `aux` its drift, a short life.
fn rising(kind: Kind, pos: DVec3, aux: DVec3, uv: [f32; 4], rng: &mut JavaRandom) -> Particle {
    let mut p = Particle::moving(kind, pos, aux, rng);
    p.look.uv = uv;
    p.friction = 0.96;
    p.vel = p.vel * f64::from(0.01f32) + aux;
    let mut jitter = || f64::from((rng.next_float() - rng.next_float()) * 0.05);
    p.pos += DVec3::new(jitter(), jitter(), jitter());
    p.prev = p.pos;
    p.lifetime = (8.0 / (rng.next_double() * 0.8 + 0.2)) as i32 + 4;
    p
}

/// `BaseAshSmokeParticle`: smoke and ash, darkened at random, growing in.
#[allow(clippy::too_many_arguments)]
fn ash_smoke(
    kind: Kind,
    pos: DVec3,
    dir: [f32; 3],
    aux: DVec3,
    scale: f32,
    color_random: f32,
    max_lifetime: i32,
    gravity: f32,
    physics: bool,
    rng: &mut JavaRandom,
) -> Particle {
    let mut p = Particle::moving(kind, pos, DVec3::ZERO, rng);
    p.friction = 0.96;
    p.gravity = gravity;
    p.speed_up_when_blocked = true;
    p.vel *= DVec3::new(f64::from(dir[0]), f64::from(dir[1]), f64::from(dir[2]));
    p.vel += aux;
    let col = rng.next_float() * color_random;
    p.set_rgb([col; 3]);
    p.quad_size *= 0.75 * scale;
    p.lifetime = ((f64::from(max_lifetime) / (rng.next_double() * 0.8 + 0.2) * f64::from(scale))
        as i32)
        .max(1);
    p.physics = physics;
    p.look.animated = true;
    p.look.size = SizeCurve::GrowIn;
    p
}

/// `TerrainParticle`: a quarter of the block's texture, tinted and
fn terrain(
    kind: Kind,
    pos: DVec3,
    aux: DVec3,
    state: u16,
    ctx: &TickCtx,
    rng: &mut JavaRandom,
) -> Option<Particle> {
    if state == 0 || is_moving_piston(state) {
        return None;
    }
    let models = ctx.blocks?;
    let look = models.particle_look(state)?;
    let mut p = Particle::moving(kind, pos, aux, rng);
    p.gravity = 1.0;
    let mut rgb = look.rgb.map(|c| c * 0.6);
    if look.tint != crate::world::block_tint::TintKind::None {
        let at = pos.floor().as_ivec3();
        let c = ctx.level.tint_at(look.tint, [at.x, at.y, at.z]);
        for (out, c) in rgb.iter_mut().zip(c) {
            *out *= f32::from(c) / 255.0;
        }
    }
    p.set_rgb(rgb);
    p.quad_size /= 2.0;
    p.look.uv = quarter(look.uv, rng);
    p.look.translucent = look.translucent;
    Some(p)
}

/// A random quarter of `rect`, as `TerrainParticle` and
fn quarter(rect: [f32; 4], rng: &mut JavaRandom) -> [f32; 4] {
    let uo = rng.next_float() * 3.0;
    let vo = rng.next_float() * 3.0;
    let u = |f: f32| rect[0] + (rect[2] - rect[0]) * f;
    let v = |f: f32| rect[1] + (rect[3] - rect[1]) * f;
    [
        u((uo + 1.0) / 4.0),
        v(vo / 4.0),
        u(uo / 4.0),
        v((vo + 1.0) / 4.0),
    ]
}

fn is_moving_piston(state: u16) -> bool {
    super::client::block_name(state) == "moving_piston"
}

/// `FallingDustParticle`'s colour: a falling block's dust colour, the
fn dust_color(ctx: &TickCtx, state: u16, at: [i32; 3]) -> [f32; 3] {
    let name = super::client::block_name(state);
    let dust = match name {
        "sand" | "suspicious_sand" => Some(14_406_560),
        "red_sand" => Some(11_098_145),
        "gravel" | "suspicious_gravel" => Some(8_356_727),
        "dragon_egg" => Some(0),
        _ => None,
    };
    if let Some(rgb) = dust {
        return rgb_of(rgb);
    }
    if let Some(look) = ctx.blocks.and_then(|m| m.particle_look(state))
        && look.tint_source
    {
        if look.tint == crate::world::block_tint::TintKind::None {
            return look.rgb;
        }
        let [r, g, b] = ctx.level.tint_at(look.tint, at);
        return [r, g, b].map(|c| f32::from(c) / 255.0);
    }
    let map = pumpkin_data::BlockStateId::new(state)
        .map_or(0, |id| pumpkin_data::Block::from_state_id(id).map_color);
    rgb_of(crate::world::map_color::rgb(map))
}

/// `DripParticle`'s providers: colour, stage and what each turns into.
fn drip(kind: Kind, pos: DVec3, uv: [f32; 4], rng: &mut JavaRandom) -> Particle {
    use Kind as P;
    const WATER: [f32; 3] = [0.2, 0.3, 1.0];
    const LAVA: [f32; 3] = [1.0, 0.285_714_3, 0.083_333_336];
    const TEAR: [f32; 3] = [0.511_718_75, 0.031_25, 0.890_625];
    let mut p = Particle::at(kind, pos, rng);
    p.look.uv = uv;
    p.width = 0.01;
    p.height = 0.01;
    p.gravity = 0.06;
    let water = Some(FluidKind::Water);
    let lava = Some(FluidKind::Lava);
    let fall_and_land = |rng: &mut JavaRandom| (64.0 / (rng.next_float() * 0.8 + 0.2)) as i32;
    let land = |rng: &mut JavaRandom, of: f32| (of / (rng.next_float() * 0.8 + 0.2)) as i32;
    let (fluid, stage, next, cooling, rgb) = match kind {
        P::DrippingWater => (water, DripStage::Hang, Some(P::FallingWater), false, WATER),
        P::FallingWater => (water, DripStage::FallAndLand, Some(P::Splash), false, WATER),
        P::DrippingLava => (lava, DripStage::Hang, Some(P::FallingLava), true, [1.0; 3]),
        P::FallingLava => (
            lava,
            DripStage::FallAndLand,
            Some(P::LandingLava),
            false,
            LAVA,
        ),
        P::LandingLava => (lava, DripStage::Land, None, false, LAVA),
        P::DrippingDripstoneWater => (
            water,
            DripStage::Hang,
            Some(P::FallingDripstoneWater),
            false,
            WATER,
        ),
        P::FallingDripstoneWater => (water, DripStage::FallAndLand, Some(P::Splash), false, WATER),
        P::DrippingDripstoneLava => (
            lava,
            DripStage::Hang,
            Some(P::FallingDripstoneLava),
            true,
            [1.0; 3],
        ),
        P::FallingDripstoneLava => (
            lava,
            DripStage::FallAndLand,
            Some(P::LandingLava),
            false,
            LAVA,
        ),
        P::DrippingHoney => (
            None,
            DripStage::Hang,
            Some(P::FallingHoney),
            false,
            [0.622, 0.508, 0.082],
        ),
        P::FallingHoney => (
            None,
            DripStage::FallAndLand,
            Some(P::LandingHoney),
            false,
            [0.582, 0.448, 0.082],
        ),
        P::LandingHoney => (None, DripStage::Land, None, false, [0.522, 0.408, 0.082]),
        P::FallingNectar => (None, DripStage::Fall, None, false, [0.92, 0.782, 0.72]),
        P::FallingSporeBlossom => (None, DripStage::Fall, None, false, [0.32, 0.5, 0.22]),
        P::DrippingObsidianTear => (
            None,
            DripStage::Hang,
            Some(P::FallingObsidianTear),
            false,
            TEAR,
        ),
        P::FallingObsidianTear => (
            None,
            DripStage::FallAndLand,
            Some(P::LandingObsidianTear),
            false,
            TEAR,
        ),
        _ => (None, DripStage::Land, None, false, TEAR),
    };
    p.set_rgb(rgb);
    match stage {
        DripStage::Hang => {
            p.gravity *= 0.02;
            p.lifetime = 40;
        }
        DripStage::FallAndLand => p.lifetime = fall_and_land(rng),
        DripStage::Land => p.lifetime = land(rng, 16.0),
        DripStage::Fall => {}
    }
    match kind {
        P::FallingHoney => p.gravity = 0.01,
        P::DrippingHoney | P::DrippingObsidianTear => {
            p.gravity *= 0.01;
            p.lifetime = 100;
        }
        P::LandingHoney => p.lifetime = land(rng, 128.0),
        P::FallingNectar => {
            p.lifetime = land(rng, 16.0);
            p.gravity = 0.007;
        }
        P::FallingSporeBlossom => {
            p.lifetime = (64.0 / lerp(rng.next_float(), 0.1, 0.9)) as i32;
            p.gravity = 0.005;
        }
        P::FallingObsidianTear => p.gravity = 0.01,
        P::LandingObsidianTear => p.lifetime = land(rng, 28.0),
        _ => {}
    }
    if matches!(
        kind,
        P::DrippingObsidianTear | P::FallingObsidianTear | P::LandingObsidianTear
    ) {
        p.look.glow = Glow::Block;
    }
    p.behavior = Behavior::Drip(Drip {
        fluid,
        stage,
        next,
        cooling,
    });
    p
}

/// `0xRRGGBB` as floats.
pub fn rgb_of(rgb: u32) -> [f32; 3] {
    [
        ((rgb >> 16) & 0xFF) as f32 / 255.0,
        ((rgb >> 8) & 0xFF) as f32 / 255.0,
        (rgb & 0xFF) as f32 / 255.0,
    ]
}

fn lerp(t: f32, a: f32, b: f32) -> f32 {
    a + (b - a) * t
}
