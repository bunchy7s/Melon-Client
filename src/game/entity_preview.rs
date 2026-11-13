//! The player in an inventory's box, turned toward the mouse: drawn into a

use std::collections::HashMap;

use glam::Vec3;
use image::RgbaImage;

use super::InWorld;
use super::world_resources::WorldResources;
use crate::gui::entity_preview::{self, EntityPreview, PreviewLayer};
use crate::renderer::{Renderer, TextureId};
use crate::settings::options::Config;
use crate::world::block_entity::models::SkullType;
use crate::world::entity::kind::EntityKind;
use crate::world::entity::mobs;
use crate::world::entity::mobs::custom_head::HeadTransforms;
use crate::world::entity::model::PartId;
use crate::world::entity::player::PlayerModel;
use crate::world::head_profile::HeadProfile;

/// `Avatar.STANDING_DIMENSIONS`' and `POSES[CROUCHING]`'s heights.
const STANDING_HEIGHT: f32 = 1.8;
const CROUCHING_HEIGHT: f32 = 1.5;

#[derive(Default)]
pub struct PreviewTextures {
    by_size: HashMap<(u32, u32), TextureId>,
    /// The picture last written, and where.
    last: Option<(TextureId, RgbaImage)>,
    /// This frame's, if the overlay shows one.
    pub current: Option<TextureId>,
    /// The mob textures drawn so far, decoded (`None`: not there).
    mob_images: HashMap<String, Option<RgbaImage>>,
}

impl InWorld {
    /// The own profile id.
    pub fn profile_id(&self) -> uuid::Uuid {
        self.uuid
    }

    /// Draws the player for the open overlay's box, if it has one.
    pub fn prepare_entity_preview(
        &mut self,
        renderer: &mut Renderer,
        assets: &WorldResources,
        config: &Config,
        gui_scale: u32,
    ) {
        let Some(preview) = self.overlay.as_ref().and_then(|o| o.entity_preview()) else {
            self.preview.current = None;
            return;
        };
        let scale = gui_scale.max(1) as f32;
        let image = match preview.entity {
            Some(id) => self.mob_preview(assets, &preview, id, scale),
            None => Some(self.player_preview(assets, config, &preview, scale)),
        };
        let Some(image) = image else {
            self.preview.current = None;
            return;
        };
        let (width, height) = (image.width(), image.height());
        let previews = &mut self.preview;
        let texture = match previews.by_size.get(&(width, height)) {
            Some(&texture) => {
                let same = previews
                    .last
                    .as_ref()
                    .is_some_and(|(at, last)| *at == texture && *last == image);
                if !same {
                    renderer.update_texture(texture, 0, 0, 0, image.clone());
                }
                texture
            }
            None => match renderer.load_texture(&image) {
                Ok(texture) => {
                    previews.by_size.insert((width, height), texture);
                    texture
                }
                Err(e) => {
                    eprintln!("Spielervorschau nicht hochgeladen: {e:#}");
                    previews.current = None;
                    return;
                }
            },
        };
        previews.last = Some((texture, image));
        previews.current = Some(texture);
    }
}

impl InWorld {
    /// The own player turned toward the mouse, skin then worn armor.
    fn player_preview(
        &mut self,
        assets: &WorldResources,
        config: &Config,
        preview: &EntityPreview,
        scale: f32,
    ) -> RgbaImage {
        let look = assets.players.for_profile(self.uuid);
        let player = &look.model.model;
        let partial = self.tick_time / crate::world::player::TICK;
        let arms = self.arm_poses(config.main_hand);
        let swing = self.swing_pose(config.main_hand, partial);
        let mut state = self
            .player
            .avatar_state(arms, swing, false, config.model_parts);
        let (x_angle, y_angle) = preview.angles();
        state.position = Vec3::ZERO;
        state.body_yaw = x_angle * 20.0;
        state.humanoid.y_rot = x_angle * 20.0;
        state.humanoid.x_rot = -y_angle * 20.0;
        let height = if state.humanoid.crouching {
            CROUCHING_HEIGHT
        } else {
            STANDING_HEIGHT
        };
        let root = preview.view(scale, height) * PlayerModel::root_transform(&state);
        let pose = player.pose(&state);
        let overlay = player.overlay_parts();
        let mut worn = Vec::new();
        mobs::push_humanoid_armor(EntityKind::Player, false, self.own_armor(), &mut worn);
        for layer in &worn {
            let images = &mut self.preview.mob_images;
            images
                .entry(layer.texture.to_string())
                .or_insert_with(|| assets.mobs.image(&layer.texture));
        }
        let armor_posed: Vec<_> = worn
            .iter()
            .map(|layer| {
                let (mob, _) = assets.mobs.get(layer.model);
                (
                    mob,
                    mobs::pose_worn((&player.model, &player.humanoid, &pose), mob),
                )
            })
            .collect();
        let worn_head = self.own_armor()[0];
        let skull = SkullType::of_item(worn_head.item.registry_key);
        let head_profile = HeadProfile::of_stack(worn_head);
        if let Some(kind) = skull.filter(|&k| k != SkullType::Player) {
            let images = &mut self.preview.mob_images;
            images
                .entry(kind.texture().to_owned())
                .or_insert_with(|| assets.mobs.image(kind.texture()));
        }
        let head_skull = skull.map(|kind| {
            let model = assets.players.head.model(kind);
            let head = player
                .model
                .part_transform(&pose, player.humanoid.head, root);
            (
                model,
                model.pose(state.humanoid.walk_position),
                super::heads::worn_skull_transform(head, HeadTransforms::DEFAULT),
            )
        });
        let head_texture = match skull {
            Some(SkullType::Player) => Some(assets.players.for_head(head_profile.as_ref()).image),
            Some(kind) => self
                .preview
                .mob_images
                .get(kind.texture())
                .and_then(|t| t.as_ref()),
            None => None,
        };
        let head_overlay: Vec<PartId> = head_skull
            .as_ref()
            .and_then(|(model, ..)| model.overlay())
            .into_iter()
            .collect();
        let mut layers = vec![
            PreviewLayer {
                model: &player.model,
                pose: &pose,
                root,
                texture: look.image,
                tint: [255; 3],
                skip: &overlay,
                only: None,
                share_depth: false,
                glint: None,
            },
            PreviewLayer {
                model: &player.model,
                pose: &pose,
                root,
                texture: look.image,
                tint: [255; 3],
                skip: &[],
                only: Some(&overlay),
                share_depth: true,
                glint: None,
            },
        ];
        for (layer, (mob, armor_pose)) in worn.iter().zip(&armor_posed) {
            let Some(texture) = self
                .preview
                .mob_images
                .get(layer.texture.as_ref())
                .and_then(|t| t.as_ref())
            else {
                continue;
            };
            layers.push(PreviewLayer {
                model: &mob.model,
                pose: armor_pose,
                root,
                texture,
                tint: layer
                    .tint
                    .map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8),
                skip: &[],
                only: None,
                share_depth: true,
                glint: layer.foil.then_some(&assets.glint_armor_image),
            });
        }
        if let (Some((model, pose, at)), Some(texture)) = (&head_skull, head_texture) {
            layers.push(PreviewLayer {
                model: model.model(),
                pose,
                root: *at,
                texture,
                tint: [255; 3],
                skip: &head_overlay,
                only: None,
                share_depth: true,
                glint: None,
            });
            if !head_overlay.is_empty() {
                layers.push(PreviewLayer {
                    model: model.model(),
                    pose,
                    root: *at,
                    texture,
                    tint: [255; 3],
                    skip: &[],
                    only: Some(&head_overlay),
                    share_depth: true,
                    glint: None,
                });
            }
        }
        let (width, height) = preview.pixels(scale);
        entity_preview::rasterize_layers(&layers, width, height)
    }

    /// A tracked mob turned toward the mouse, layer by layer (a horse's
    fn mob_preview(
        &mut self,
        assets: &WorldResources,
        preview: &EntityPreview,
        id: i32,
        scale: f32,
    ) -> Option<RgbaImage> {
        let entity = self.entities.get(id)?;
        let mut specs = Vec::new();
        mobs::layers(entity, 0.0, &|id| assets.mobs.hat(id), &mut specs);
        specs.retain(|layer| !layer.translucent);
        let mut input = mobs::pose_input(entity, 1.0);
        let (x_angle, y_angle) = preview.angles();
        input.living.position = Vec3::ZERO;
        input.living.body_rot = x_angle * 20.0;
        input.living.y_rot = x_angle * 20.0;
        input.living.x_rot = -y_angle * 20.0;
        input.living.walk_position = 0.0;
        input.living.walk_speed = 0.0;
        input.living.riding = false;
        let root = preview.view(scale, entity.size().1) * mobs::root(entity, &input, 1.0);
        let mut posed = Vec::new();
        let mut body_pose: Option<(&mobs::MobModel, crate::world::entity::model::ModelPose)> = None;
        for layer in &specs {
            let (mob, _) = assets.mobs.get(layer.model);
            let pose = if layer.copy_humanoid {
                body_pose
                    .as_ref()
                    .and_then(|(from_mob, from_pose)| {
                        let parts = from_mob.humanoid()?;
                        mob.humanoid()?;
                        Some(mobs::pose_worn((&from_mob.model, parts, from_pose), mob))
                    })
                    .unwrap_or_else(|| mob.model.initial_pose())
            } else {
                mob.pose(&input)
            };
            if body_pose.is_none() {
                body_pose = Some((mob, pose.clone()));
            }
            posed.push((mob, pose));
        }
        for layer in &specs {
            let images = &mut self.preview.mob_images;
            images
                .entry(layer.texture.to_string())
                .or_insert_with(|| assets.mobs.image(&layer.texture));
        }
        let layers: Vec<_> = specs
            .iter()
            .zip(&posed)
            .enumerate()
            .filter_map(|(i, (layer, (mob, pose)))| {
                let texture = self
                    .preview
                    .mob_images
                    .get(layer.texture.as_ref())?
                    .as_ref()?;
                Some(PreviewLayer {
                    model: &mob.model,
                    pose,
                    root,
                    texture,
                    tint: layer
                        .tint
                        .map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8),
                    skip: &[],
                    only: None,
                    share_depth: i > 0,
                    glint: layer.foil.then_some(&assets.glint_armor_image),
                })
            })
            .collect();
        let (width, height) = preview.pixels(scale);
        Some(entity_preview::rasterize_layers(&layers, width, height))
    }
}
