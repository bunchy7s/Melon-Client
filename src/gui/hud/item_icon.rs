//! Item icons for the hotbar and inventory, baked on the CPU from the

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use glam::{Mat3, Vec2, Vec3};
use image::{Rgba, RgbaImage};
use pumpkin_data::item_stack::ItemStack;

use crate::assets::json_model::{Display, TextureCache};
use crate::gui::sprite::GuiSprite;
use crate::math::angle::Rotation;
use crate::renderer::Renderer;
use crate::world::block_entity::models::SkullType;
use crate::world::head_profile::HeadProfile;
use crate::world::item_model::{self, Count, ItemKey, ItemModel, ItemQuad};
use crate::world::texture_animation;

/// Edge length of a baked block icon. Drawn at 16 GUI pixels, so this is
const BLOCK_ICON_SIZE: u32 = 64;
/// `ItemTransform`'s default for block models (`block/block.json`'s `gui`).
const DEFAULT_BLOCK_GUI: Display = Display {
    rotation: Vec3::new(30.0, 225.0, 0.0),
    translation: Vec3::ZERO,
    scale: Vec3::splat(0.625),
};
/// Baked icons by item and tint, uploaded on first use and kept.
pub struct ItemIcons {
    assets_root: PathBuf,
    icons: HashMap<ItemKey, Option<GuiSprite>>,
    /// Player heads with a profile, by [`HeadProfile::skin_key`]: their
    heads: HashMap<uuid::Uuid, (bool, Option<GuiSprite>)>,
}

impl ItemIcons {
    pub fn new(assets_root: &Path) -> Self {
        Self {
            assets_root: assets_root.to_path_buf(),
            icons: HashMap::new(),
            heads: HashMap::new(),
        }
    }

    /// Makes sure `stack`'s icon exists, baking it on first sight. Whether
    pub fn prepare(&mut self, renderer: &mut Renderer, stack: &ItemStack) -> bool {
        let Some((key, name)) = self.missing(stack) else {
            return false;
        };
        let sprite = bake_animated(&self.assets_root, name, Some(&key), None)
            .and_then(|icon| upload(renderer, name, icon));
        self.icons.insert(key, sprite);
        true
    }

    fn missing(&self, stack: &ItemStack) -> Option<(ItemKey, &'static str)> {
        let key = ItemKey::of(&self.assets_root, stack);
        if key.item == 0 || self.icons.contains_key(&key) {
            None
        } else {
            Some((key, stack.item.registry_key))
        }
    }

    /// CPU-bakes a parallel batch of missing icons from `stacks`, then
    pub fn prepare_batch(
        &mut self,
        renderer: &mut Renderer,
        stacks: impl FnOnce(&mut dyn FnMut(&ItemStack)),
    ) {
        let mut jobs = Vec::new();
        let mut seen = HashSet::new();
        stacks(&mut |stack| {
            if let Some((key, name)) = self.missing(stack)
                && seen.insert(key.clone())
            {
                jobs.push((key, name));
            }
        });
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .max(1);
        jobs.truncate(threads.saturating_mul(8).max(8));
        if jobs.is_empty() {
            return;
        }
        let root = &self.assets_root;
        let baked: Vec<_> = std::thread::scope(|scope| {
            let size = jobs.len().div_ceil(threads).max(1);
            let mut joins = Vec::new();
            for chunk in jobs.chunks(size) {
                joins.push(scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|(key, name)| {
                            (
                                key.clone(),
                                *name,
                                bake_animated(root, name, Some(key), None),
                            )
                        })
                        .collect::<Vec<_>>()
                }));
            }
            joins
                .into_iter()
                .flat_map(|job| job.join().expect("item icon bake"))
                .collect()
        });
        for (key, name, icon) in baked {
            let sprite = icon.and_then(|icon| upload(renderer, name, icon));
            self.icons.insert(key, sprite);
        }
    }

    /// The icon of a player head with `profile`, drawn with `skin` (its
    pub fn prepare_head(
        &mut self,
        renderer: &mut Renderer,
        stack: &ItemStack,
        profile: &HeadProfile,
        skin: &RgbaImage,
        own_skin: bool,
    ) -> bool {
        let key = profile.skin_key();
        if let Some(&(baked_own, _)) = self.heads.get(&key)
            && (baked_own || !own_skin)
        {
            return false;
        }
        let name = stack.item.registry_key;
        let sprite = bake_animated(&self.assets_root, name, None, Some(skin))
            .and_then(|icon| upload(renderer, name, icon));
        if let Some((_, Some(old))) = self.heads.insert(key, (own_skin, sprite)) {
            renderer.free_texture(old.texture);
        }
        true
    }

    /// Frees every baked icon.
    pub fn release(&mut self, renderer: &mut Renderer) {
        item_model::forget_count_cuts();
        let heads = self.heads.drain().filter_map(|(_, (_, s))| s);
        for sprite in self.icons.drain().filter_map(|(_, s)| s).chain(heads) {
            renderer.free_texture(sprite.texture);
        }
    }

    /// The icon, if [`Self::prepare`] managed to bake it; a player head's
    pub fn get(&self, stack: &ItemStack) -> Option<GuiSprite> {
        if let Some(profile) = HeadProfile::of_stack(stack)
            && let Some(&(_, Some(sprite))) = self.heads.get(&profile.skin_key())
        {
            return Some(sprite);
        }
        self.icons
            .get(&ItemKey::of(&self.assets_root, stack))
            .copied()
            .flatten()
    }
}

/// Uploads a baked icon as a sprite (a strip for an animated one).
fn upload(renderer: &mut Renderer, name: &str, mut icon: BakedIcon) -> Option<GuiSprite> {
    if icon.frames[0].height() * icon.frames.len() as u32 > MAX_STRIP_HEIGHT {
        icon.frames.truncate(1);
        icon.schedule = vec![(0, 1)];
    }
    let strip = icon.strip();
    let texture = renderer
        .load_texture(&strip)
        .map_err(|e| eprintln!("Item-Icon {name} nicht hochgeladen: {e:#}"))
        .ok()?;
    Some(GuiSprite::with_meta(
        texture,
        strip.dimensions(),
        &icon.meta(),
    ))
}

/// Bakes the icon for the item with registry key `key` (e.g. `stone`), its
pub fn bake(assets_root: &Path, key: &str) -> Option<RgbaImage> {
    bake_animated(assets_root, key, None, None).map(|mut icon| icon.frames.swap_remove(0))
}

/// The tallest strip of frames an animated icon is uploaded as.
const MAX_STRIP_HEIGHT: u32 = 8192;
/// A baked icon: one image, or the frames of an item whose textures are
struct BakedIcon {
    frames: Vec<RgbaImage>,
    /// `(frame, ticks)` in play order.
    schedule: Vec<(usize, u32)>,
}

impl BakedIcon {
    /// The frames stacked top to bottom, as an animated sprite is.
    fn strip(&self) -> RgbaImage {
        let (w, h) = self.frames[0].dimensions();
        let mut strip = RgbaImage::new(w, h * self.frames.len() as u32);
        for (i, frame) in self.frames.iter().enumerate() {
            image::imageops::replace(&mut strip, frame, 0, i64::from(h) * i as i64);
        }
        strip
    }

    /// The `.mcmeta` [`Self::strip`] would have.
    fn meta(&self) -> serde_json::Value {
        let (w, h) = self.frames[0].dimensions();
        let frames: Vec<_> = self
            .schedule
            .iter()
            .map(|&(index, time)| serde_json::json!({"index": index, "time": time}))
            .collect();
        serde_json::json!({"animation": {"width": w, "height": h, "frames": frames}})
    }
}

/// [`bake`] with a potion colour for the `minecraft:potion` tints, baked
fn bake_animated(
    assets_root: &Path,
    name: &str,
    key: Option<&ItemKey>,
    skin: Option<&RgbaImage>,
) -> Option<BakedIcon> {
    let (potion, count) = key.map_or((None, Count::ONE), |k| (k.potion_color, k.count()));
    let variant = key.and_then(|k| k.variant.as_deref());
    let item = ItemModel::resolve_in_use(assets_root, name, potion, (false, count), variant)?;
    let mut textures = TextureCache::new(assets_root);
    if let Some(skin) = skin {
        textures.insert(SkullType::Player.texture(), skin.clone());
    }
    let quads = if item.is_flat() {
        Vec::new()
    } else {
        item.quads(assets_root, &mut textures)
    };
    let first = bake_frame(&item, &quads, &mut textures)?;
    let changes = texture_animation::frame_changes(textures.animations());
    let mut icon = BakedIcon {
        frames: vec![first],
        schedule: Vec::new(),
    };
    let Some((starts, period)) = changes else {
        icon.schedule.push((0, 1));
        return Some(icon);
    };
    for (i, &start) in starts.iter().enumerate() {
        let end = starts.get(i + 1).copied().unwrap_or(period);
        let time = (end - start) as u32;
        if i > 0 {
            textures.set_tick(start);
            let Some(image) = bake_frame(&item, &quads, &mut textures) else {
                continue;
            };
            if *icon.frames.last().unwrap_or(&image) == image {
                if let Some(last) = icon.schedule.last_mut() {
                    last.1 += time;
                    continue;
                }
            } else {
                icon.frames.push(image);
            }
        }
        icon.schedule.push((icon.frames.len() - 1, time));
    }
    Some(icon)
}

/// The icon with `textures` as they show now.
fn bake_frame(
    item: &ItemModel,
    quads: &[ItemQuad],
    textures: &mut TextureCache,
) -> Option<RgbaImage> {
    if item.is_flat() {
        let mut out: Option<RgbaImage> = None;
        for (texture, tint) in item.flat_layers() {
            let Some(img) = textures.get(&texture) else {
                continue;
            };
            let mut img = img.clone();
            crate::assets::tint(&mut img, tint);
            match &mut out {
                None => out = Some(img),
                Some(base) => image::imageops::overlay(base, &img, 0, 0),
            }
        }
        return out;
    }
    if quads.is_empty() {
        let texture = item.model.texture("particle")?;
        return textures.get(&texture).cloned();
    }
    let display = item.model.gui.unwrap_or(DEFAULT_BLOCK_GUI);
    Some(rasterize(quads, display, item.model.front_light, textures))
}

/// Draws the faces orthographically through the GUI display transform,
fn rasterize(
    quads: &[ItemQuad],
    display: Display,
    front_light: bool,
    textures: &mut TextureCache,
) -> RgbaImage {
    let size = BLOCK_ICON_SIZE;
    let rotation = Mat3::rot_x_deg(display.rotation.x)
        * Mat3::rot_y_deg(display.rotation.y)
        * Mat3::rot_z_deg(display.rotation.z);
    let to_view = |p: Vec3| -> Vec3 {
        let centred = p - Vec3::splat(0.5);
        rotation * (centred * display.scale) + display.translation / 16.0
    };

    let mut out = RgbaImage::new(size, size);
    let mut depth = vec![f32::NEG_INFINITY; (size * size) as usize];
    let mut faces: Vec<(&ItemQuad, bool)> = quads
        .iter()
        .filter_map(|quad| Some((quad, partly_transparent(textures.get(&quad.texture)?))))
        .collect();
    faces.sort_by_key(|&(_, see_through)| see_through);
    for (quad, _) in faces {
        let Some(texture) = textures.get(&quad.texture) else {
            continue;
        };
        let normal = (rotation * quad.normal).normalize_or_zero();
        if normal.z < -1.0e-4 {
            continue;
        }
        let shade = gui_light(normal, front_light);
        let view = quad.corners.map(to_view);
        let screen =
            view.map(|v| Vec3::new((v.x + 0.5) * size as f32, (0.5 - v.y) * size as f32, v.z));
        for [a, b, c] in [[0, 1, 2], [0, 2, 3]] {
            draw_triangle(
                &mut out,
                &mut depth,
                [screen[a], screen[b], screen[c]],
                [quad.uvs[a], quad.uvs[b], quad.uvs[c]],
                texture,
                shade,
                quad.tint,
                None,
            );
        }
    }
    out
}

/// `Lighting.setupFor3DItems`: two directional lights plus ambient, in view
fn gui_light(normal: Vec3, front: bool) -> f32 {
    let mut l0 = Vec3::new(0.2, 1.0, -0.7).normalize();
    let mut l1 = Vec3::new(-0.2, 1.0, 0.7).normalize();
    if front {
        let turn = Mat3::rot_y(-22.5f32.to_radians()) * Mat3::rot_x_deg(135.0f32);
        l0 = turn * l0;
        l1 = turn * l1;
    }
    (0.4 + 0.6 * (normal.dot(l0).max(0.0) + normal.dot(l1).max(0.0))).min(1.0)
}

/// Whether some texel is neither clear nor opaque (`Transparency.TRANSLUCENT`).
fn partly_transparent(texture: &RgbaImage) -> bool {
    texture.pixels().any(|p| (26..255).contains(&p[3]))
}

/// Fills one triangle with the nearest texel, keeping the nearest surface
pub(crate) fn draw_triangle(
    out: &mut RgbaImage,
    depth: &mut [f32],
    p: [Vec3; 3],
    uv: [Vec2; 3],
    texture: &RgbaImage,
    shade: f32,
    tint: [u8; 3],
    glint: Option<&RgbaImage>,
) {
    let area = (p[1].x - p[0].x) * (p[2].y - p[0].y) - (p[2].x - p[0].x) * (p[1].y - p[0].y);
    if area.abs() < 1.0e-6 {
        return;
    }
    let (w, h) = (out.width() as i32, out.height() as i32);
    let min_x = p
        .iter()
        .map(|v| v.x)
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as i32;
    let max_x = p
        .iter()
        .map(|v| v.x)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(w as f32) as i32;
    let min_y = p
        .iter()
        .map(|v| v.y)
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as i32;
    let max_y = p
        .iter()
        .map(|v| v.y)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min(h as f32) as i32;
    let (tw, th) = (texture.width() as f32, texture.height() as f32);
    for y in min_y..max_y {
        for x in min_x..max_x {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let w0 = ((p[1].x - px) * (p[2].y - py) - (p[2].x - px) * (p[1].y - py)) / area;
            let w1 = ((p[2].x - px) * (p[0].y - py) - (p[0].x - px) * (p[2].y - py)) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 < -1.0e-4 || w1 < -1.0e-4 || w2 < -1.0e-4 {
                continue;
            }
            let z = w0 * p[0].z + w1 * p[1].z + w2 * p[2].z;
            let index = (y * w + x) as usize;
            if z <= depth[index] {
                continue;
            }
            let t = uv[0] * w0 + uv[1] * w1 + uv[2] * w2;
            let tx = ((t.x * tw) as i32).clamp(0, tw as i32 - 1) as u32;
            let ty = ((t.y * th) as i32).clamp(0, th as i32 - 1) as u32;
            let texel = texture.get_pixel(tx, ty);
            if texel[3] < 26 {
                continue;
            }
            let lit = |c: u8, t: u8| f32::from(c) * f32::from(t) / 255.0 * shade;
            let color = [
                lit(texel[0], tint[0]),
                lit(texel[1], tint[1]),
                lit(texel[2], tint[2]),
            ];
            if texel[3] == 255 {
                depth[index] = z;
                out.put_pixel(
                    x as u32,
                    y as u32,
                    Rgba([
                        color[0].round() as u8,
                        color[1].round() as u8,
                        color[2].round() as u8,
                        255,
                    ]),
                );
                add_glint(out, x as u32, y as u32, t, glint);
                continue;
            }
            let alpha = f32::from(texel[3]) / 255.0;
            let below = out.get_pixel(x as u32, y as u32);
            let below_alpha = f32::from(below[3]) / 255.0;
            let total = alpha + below_alpha * (1.0 - alpha);
            let mix = |c: f32, b: u8| {
                ((c * alpha + f32::from(b) * below_alpha * (1.0 - alpha)) / total).round() as u8
            };
            out.put_pixel(
                x as u32,
                y as u32,
                Rgba([
                    mix(color[0], below[0]),
                    mix(color[1], below[1]),
                    mix(color[2], below[2]),
                    (total * 255.0).round() as u8,
                ]),
            );
            add_glint(out, x as u32, y as u32, t, glint);
        }
    }
}

/// `BlendFunction.GLINT`: `dst + src * src` of the scrolling armor glint.
fn add_glint(out: &mut RgbaImage, x: u32, y: u32, uv: Vec2, glint: Option<&RgbaImage>) {
    let Some(glint) = glint else {
        return;
    };
    let sample = crate::world::glint::sample(
        glint,
        crate::world::glint::uv(
            uv,
            crate::world::glint::ARMOR_SCALE,
            crate::world::glint::millis(),
        ),
    );
    if sample[3] < 26 {
        return;
    }
    let pixel = out.get_pixel_mut(x, y);
    for i in 0..3 {
        let src = f32::from(sample[i]) / 255.0;
        pixel[i] = (f32::from(pixel[i]) + src * src * 255.0).min(255.0).round() as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assets() -> Option<PathBuf> {
        let root = crate::assets::default_root();
        root.join("minecraft/items/stone.json")
            .is_file()
            .then_some(root)
    }

    #[test]
    fn bakes_a_block_and_a_flat_item() {
        let Some(root) = assets() else {
            eprintln!("no extracted assets, skipping");
            return;
        };
        let stone = bake(&root, "stone").expect("stone icon");
        assert_eq!(stone.width(), BLOCK_ICON_SIZE);
        assert_eq!(stone.get_pixel(32, 32)[3], 255);
        assert_eq!(stone.get_pixel(0, 0)[3], 0);
        let diamond = bake(&root, "diamond").expect("diamond icon");
        assert_eq!(diamond.width(), 16);
        for key in [
            "grass_block",
            "oak_stairs",
            "oak_fence",
            "torch",
            "chest",
            "compass",
        ] {
            assert!(bake(&root, key).is_some(), "{key} has no icon");
        }
    }

    #[test]
    fn block_entity_items_are_drawn_as_models() {
        let Some(root) = assets() else {
            eprintln!("no extracted assets, skipping");
            return;
        };
        for key in [
            "chest",
            "ender_chest",
            "red_shulker_box",
            "blue_banner",
            "creeper_head",
            "player_head",
            "decorated_pot",
            "conduit",
            "white_bed",
            "shield",
        ] {
            let icon = bake(&root, key).unwrap_or_else(|| panic!("{key} has no icon"));
            assert_eq!(icon.width(), BLOCK_ICON_SIZE, "{key}");
            assert!(icon.pixels().any(|p| p[3] == 255), "{key} is empty");
        }
        assert_eq!(bake(&root, "trident").expect("trident").width(), 16);
    }
}
