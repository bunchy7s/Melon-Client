//! Items in hand on the GPU: each item's [`HeldItem`] meshes and texture,

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use pumpkin_data::item_stack::ItemStack;

use image::RgbaImage;

use crate::renderer::{MeshId, Renderer, TextureId};
use crate::world::head_profile::HeadProfile;
use crate::world::item_model::{
    self, Count, HandPose, HeldAnimation, HeldItem, HeldUse, ItemKey, ItemModel,
};

/// One item's uploaded meshes, by [`HandPose`], and their texture.
struct Uploaded {
    texture: TextureId,
    translucent: bool,
    meshes: [MeshId; 10],
    /// The ground mesh's lowest point and its depth (`getModelBoundingBox`
    ground: (f32, f32),
    /// Animated textures: the frames and the one showing.
    animation: Option<(HeldAnimation, usize)>,
}

pub struct HeldItems {
    assets_root: PathBuf,
    /// `None` for an item with nothing to draw in hand; by item and
    items: HashMap<(ItemKey, HeldUse), Option<Uploaded>>,
    /// Blocks drawn by a mob (a mooshroom's mushrooms), by model id.
    blocks: HashMap<&'static str, Option<Uploaded>>,
    /// Player heads with a profile, by [`HeadProfile::skin_key`] and use:
    heads: HashMap<(uuid::Uuid, bool), (bool, Option<Uploaded>)>,
}

impl HeldItems {
    pub fn new(assets_root: &Path) -> Self {
        Self {
            assets_root: assets_root.to_path_buf(),
            items: HashMap::new(),
            blocks: HashMap::new(),
            heads: HashMap::new(),
        }
    }

    /// Makes sure `stack` can be drawn in hand, in use or not, building it
    pub fn prepare(
        &mut self,
        renderer: &mut Renderer,
        stack: &ItemStack,
        using: impl Into<HeldUse>,
    ) {
        let using = using.into();
        let key = ItemKey::of(&self.assets_root, stack);
        let key = (key, using);
        if stack.is_empty() || self.items.contains_key(&key) {
            return;
        }
        let name = stack.item.registry_key;
        let held = ItemModel::held_poses(
            &self.assets_root,
            name,
            key.0.potion_color,
            (key.1, key.0.count()),
            key.0.variant.as_deref(),
            None,
        )
        .and_then(|held| {
            upload(renderer, held)
                .map_err(|e| eprintln!("Item {name} in der Hand nicht hochgeladen: {e:#}"))
                .ok()
        });
        self.items.insert(key, held);
    }

    /// Makes sure a player head with `profile` can be drawn in hand with
    pub fn prepare_head(
        &mut self,
        renderer: &mut Renderer,
        stack: &ItemStack,
        profile: &HeadProfile,
        (skin, own_skin): (&RgbaImage, bool),
        using: bool,
    ) {
        let key = (profile.skin_key(), using);
        if let Some(&(built_own, _)) = self.heads.get(&key)
            && (built_own || !own_skin)
        {
            return;
        }
        let name = stack.item.registry_key;
        let held =
            ItemModel::resolve_in_use(&self.assets_root, name, None, (using, Count::ONE), None)
                .and_then(|item| item.held_with_skin(&self.assets_root, Some(skin)))
                .and_then(|held| {
                    upload(renderer, held)
                        .map_err(|e| eprintln!("Kopf in der Hand nicht hochgeladen: {e:#}"))
                        .ok()
                });
        if let Some((_, Some(old))) = self.heads.insert(key, (own_skin, held)) {
            free(renderer, old);
        }
    }

    /// Makes sure the block model `model` can be drawn by [`Self::block`].
    pub fn prepare_block(&mut self, renderer: &mut Renderer, model: &'static str) {
        if self.blocks.contains_key(model) {
            return;
        }
        let held = ItemModel::block(&self.assets_root, model)
            .and_then(|item| item.held(&self.assets_root))
            .and_then(|held| {
                upload(renderer, held)
                    .map_err(|e| eprintln!("Block {model} nicht hochgeladen: {e:#}"))
                    .ok()
            });
        self.blocks.insert(model, held);
    }

    /// The mesh and texture of a prepared block model, centred on the
    pub fn block(&self, model: &str) -> Option<(MeshId, TextureId, bool)> {
        let uploaded = self.blocks.get(model)?.as_ref()?;
        Some((
            uploaded.meshes[HandPose::Ground as usize],
            uploaded.texture,
            uploaded.translucent,
        ))
    }

    /// Moves the animated items' textures to the frame showing at `tick`
    pub fn animate(&mut self, renderer: &mut Renderer, tick: u64) {
        let uploaded = self
            .items
            .values_mut()
            .chain(self.blocks.values_mut())
            .flatten()
            .chain(self.heads.values_mut().filter_map(|(_, u)| u.as_mut()));
        for item in uploaded {
            let Some((animation, shown)) = &mut item.animation else {
                continue;
            };
            let frame = animation.frame_at(tick);
            if frame != *shown {
                *shown = frame;
                renderer.update_texture(item.texture, 0, 0, 0, animation.frames[frame].clone());
            }
        }
    }

    /// Frees every uploaded item.
    pub fn release(&mut self, renderer: &mut Renderer) {
        item_model::forget_count_cuts();
        let blocks = self.blocks.drain().filter_map(|(_, u)| u);
        let heads = self.heads.drain().filter_map(|(_, (_, u))| u);
        for uploaded in blocks
            .chain(self.items.drain().filter_map(|(_, u)| u))
            .chain(heads)
        {
            free(renderer, uploaded);
        }
    }

    /// The mesh and texture for `stack` held in `pose`, in use or not, and
    pub fn get(
        &self,
        stack: &ItemStack,
        pose: HandPose,
        using: impl Into<HeldUse>,
    ) -> Option<(MeshId, TextureId, bool)> {
        let uploaded = self.uploaded(stack, using.into())?;
        Some((
            uploaded.meshes[pose as usize],
            uploaded.texture,
            uploaded.translucent,
        ))
    }

    /// The lowest point and the depth of `stack`'s model on the ground.
    pub fn ground_bounds(&self, stack: &ItemStack) -> Option<(f32, f32)> {
        Some(self.uploaded(stack, HeldUse::Idle)?.ground)
    }

    fn uploaded(&self, stack: &ItemStack, using: HeldUse) -> Option<&Uploaded> {
        if stack.is_empty() {
            return None;
        }
        let head = HeadProfile::of_stack(stack)
            .and_then(|profile| {
                self.heads
                    .get(&(profile.skin_key(), using == HeldUse::Using))
            })
            .and_then(|(_, uploaded)| uploaded.as_ref());
        let uploaded = match head {
            Some(head) => head,
            None => self
                .items
                .get(&(ItemKey::of(&self.assets_root, stack), using))?
                .as_ref()?,
        };
        Some(uploaded)
    }
}

fn free(renderer: &mut Renderer, uploaded: Uploaded) {
    renderer.free_texture(uploaded.texture);
    for mesh in uploaded.meshes {
        renderer.free_mesh(mesh);
    }
}

fn upload(renderer: &mut Renderer, held: HeldItem) -> anyhow::Result<Uploaded> {
    let texture = renderer.load_texture(&held.texture)?;
    let mut meshes = Vec::with_capacity(HandPose::ALL.len());
    for vertices in &held.meshes {
        match renderer.upload_mesh(vertices) {
            Ok(mesh) => meshes.push(mesh),
            Err(e) => {
                for mesh in meshes {
                    renderer.free_mesh(mesh);
                }
                return Err(e);
            }
        }
    }
    let meshes = <[MeshId; 10]>::try_from(meshes).unwrap_or_else(|_| unreachable!("one per pose"));
    let animation = held.animation.map(|animation| (animation, 0));
    let ground = &held.meshes[HandPose::Ground as usize];
    let fold = |axis: usize, init: f32, f: fn(f32, f32) -> f32| {
        ground.iter().fold(init, |acc, v| f(acc, v.pos[axis]))
    };
    let ground = if ground.is_empty() {
        (0.0, 0.0)
    } else {
        (
            fold(1, f32::MAX, f32::min),
            fold(2, f32::MIN, f32::max) - fold(2, f32::MAX, f32::min),
        )
    };
    Ok(Uploaded {
        ground,
        texture,
        translucent: held.translucent,
        meshes,
        animation,
    })
}
