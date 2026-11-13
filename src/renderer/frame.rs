//! Contract between the three layers: the only path world and UI take into the

use glam::Mat4;

use super::quad::DrawList;
use super::{CubeMapId, MeshId, QuadMeshId, TextureId};

/// One frame: scene first, then UI on top.
pub struct Frame<'a> {
    /// World. `None`: no 3D background, only [`Frame::clear_color`].
    pub scene: Option<&'a SceneFrame>,
    pub ui: &'a DrawList,
    /// GUI coordinate system of the `ui` vertices (`gui::screen::virtual_size`).
    pub ui_size: (f32, f32),
    pub clear_color: [f32; 4],
    /// Blur what was drawn before a point in the UI list, then draw the rest
    pub blur: Option<Blur>,
}

/// `Screen.extractBlurredBackground`: the frame so far, blurred, under a menu.
#[derive(Clone, Copy)]
pub struct Blur {
    /// The box width in pixels (`Options.menuBackgroundBlurriness`).
    pub radius: f32,
    /// The scene and the UI calls before this index are blurred; the calls
    pub ui_split: usize,
}

/// World snapshot for one frame: sky first (background, no depth), then
pub struct SceneFrame {
    pub sky: Option<Sky>,
    pub world: Option<WorldDraw>,
    /// Extra meshes drawn with the same camera as `world` (terrain), each
    pub entities: Vec<EntityDraw>,
    /// Cloud boxes, camera-relative, white texture, vertex colour.
    pub clouds: Option<EntityDraw>,
    /// Rain then snow columns, camera-relative, wrapping environment textures.
    pub weather: Vec<EntityDraw>,
    /// `textures/misc/enchanted_glint_{item,armor}.png`, for the overlay.
    pub glint_item: Option<TextureId>,
    pub glint_armor: Option<TextureId>,
    /// The entities (indices into [`Self::entities`]) that glow, with the
    pub glowing: Vec<(usize, [u8; 3])>,
    /// Post effect chains the scene asks for (`creeper`, `invert`...), by
    pub post_effects: Vec<String>,
}

/// One non-terrain mesh in world space: `model` places [`EntityDraw::mesh`]
#[derive(Clone)]
pub struct EntityDraw {
    pub mesh: MeshId,
    pub atlas: TextureId,
    /// From the mesh to world space less [`Self::origin`].
    pub model: Mat4,
    /// The block `model` is relative to. Far from the world's origin a
    pub origin: glam::IVec3,
    /// [`RenderLayer::Entities`] or [`RenderLayer::Hand`].
    pub layer: RenderLayer,
    /// Blended, without writing depth: an item with see-through texels.
    pub translucent: bool,
    /// The light on this draw (`EntityRenderer.getPackedLightCoords` at
    pub light: Option<[f32; 3]>,
    /// Hurt red or creeper white, see [`DrawLight::overlay`](crate::renderer::pipeline::world::DrawLight::overlay).
    pub overlay: f32,
    /// Enchantment glint overlay (`ItemStack.hasFoil`).
    pub glint: Glint,
}

/// Which enchanted-glint overlay a draw wants (`TextureTransform` scale
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Glint {
    #[default]
    None,
    /// GUI and first-person items: sprite-local UVs, item texture.
    Item,
    /// Items held in the world: sprite-local UVs, item texture.
    Entity,
    /// Worn armor: scale 0.16, armor texture.
    Armor,
}

impl Glint {
    pub const fn scale(self) -> f32 {
        match self {
            Self::None => 0.0,
            Self::Item => crate::world::glint::ITEM_SPRITE_SCALE,
            Self::Entity => crate::world::glint::ENTITY_SPRITE_SCALE,
            Self::Armor => crate::world::glint::ARMOR_SCALE,
        }
    }

    pub const fn on(self) -> bool {
        !matches!(self, Self::None)
    }

    pub const fn is_armor(self) -> bool {
        matches!(self, Self::Armor)
    }

    /// [`Self::Item`] when `foil`, else none.
    pub const fn item(foil: bool) -> Self {
        if foil { Self::Item } else { Self::None }
    }

    /// [`Self::Entity`] when `foil`, else none.
    pub const fn entity(foil: bool) -> Self {
        if foil { Self::Entity } else { Self::None }
    }

    /// [`Self::Armor`] when `foil`, else none.
    pub const fn armor(foil: bool) -> Self {
        if foil { Self::Armor } else { Self::None }
    }
}

/// The world's draw order, vanilla's (`LevelRenderer.renderLevel`, then
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenderLayer {
    /// The cubemap or sky, behind everything, no depth.
    Sky,
    /// Chunk meshes, depth-tested and alpha-cut.
    Terrain,
    /// Bodies in the world, depth-tested against the terrain.
    Entities,
    /// Entity blob shadows (`ShadowFeatureRenderer`): blended, no depth write.
    Shadows,
    /// Cracks on blocks being broken (`RenderPipelines.CRUMBLING`):
    Crumbling,
    /// Depth only, before the water: what keeps it out of a boat.
    WaterMask,
    /// Water, glass and ice: blended over everything solid, far to near.
    Translucent,
    /// Particles (`ParticleEngine.extract`): after the translucent
    Particles,
    /// Clouds and rain/snow, over the translucent terrain.
    Atmosphere,
    /// The first-person hand: on top of the world, never inside a wall.
    Hand,
}

impl RenderLayer {
    pub const ORDER: [RenderLayer; 10] = [
        RenderLayer::Sky,
        RenderLayer::Terrain,
        RenderLayer::Entities,
        RenderLayer::Shadows,
        RenderLayer::Crumbling,
        RenderLayer::WaterMask,
        RenderLayer::Translucent,
        RenderLayer::Particles,
        RenderLayer::Atmosphere,
        RenderLayer::Hand,
    ];

    /// `renderItemInHand` clears the depth buffer first, so terrain the
    pub const fn clears_depth(self) -> bool {
        matches!(self, RenderLayer::Hand)
    }

    /// Whether the layer has a projection FOV of its own: the hand keeps
    pub const fn has_own_fov(self) -> bool {
        matches!(self, RenderLayer::Hand)
    }
}

/// Cubemap sky around the camera (vanilla `CubeMap.render`).
pub struct Sky {
    /// Title-screen panorama. In a world the sky colour is the clear colour
    pub cubemap: Option<CubeMapId>,
    /// Camera orientation, no translation: the cubemap sits at infinity.
    pub view: Mat4,
    pub fov_y_degrees: f32,
    /// Sun, moon and stars, in the same rotation-only view.
    pub celestials: Vec<CelestialDraw>,
}

/// How a [`CelestialDraw`] is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkyKind {
    /// The sky's flat colour (`tint`'s rgb), faded into the fog
    Disc,
    /// Over the sky colour, blended like `BlendFunction.TRANSLUCENT`: the
    Glow,
    /// Sun, moon and stars (`BlendFunction.OVERLAY`).
    Celestial,
}

/// One mesh of the sky, camera-local (`SkyRenderer`): the disc, the glow
pub struct CelestialDraw {
    pub kind: SkyKind,
    pub mesh: MeshId,
    pub atlas: TextureId,
    pub model: Mat4,
    /// Vertex colour multiplier: rain fade on the sun/moon, star
    pub tint: [f32; 4],
}

/// The depth-tested terrain, drawn with a real (translating) camera.
pub struct WorldDraw {
    /// The visible parts of the chunk meshes' solid layer, near to far.
    pub chunks: Vec<ChunkDraw>,
    /// Their cutout layer (leaves, plants), near to far.
    pub cutout: Vec<ChunkDraw>,
    /// Their translucent layer, far to near.
    pub translucent: Vec<ChunkDraw>,
    pub atlas: TextureId,
    /// Translate by `-cam_pos` then rotate (vanilla `Camera` order), unlike
    pub view: Mat4,
    /// The camera position the view translates by. The renderer moves each
    pub eye: glam::Vec3,
    pub fov_y_degrees: f32,
    /// The hand's: 70 degrees, narrowed while dying or in a fluid
    pub hand_fov_y_degrees: f32,
    /// Far plane distance in blocks (vanilla `GameRenderer.depthFar`).
    pub far: f32,
    /// The lightmap colour at the player's eye, multiplied onto entity
    pub ambient: [f32; 3],
    /// Time of day and brightness option, applied at draw time so day and
    pub lightmap: crate::world::lightmap::LightmapParams,
    /// The fog every drawn thing but the sky bodies is mixed into.
    pub fog: crate::world::fog::FogData,
    /// The dimension's shading of faces by direction (down, up, north,
    pub cardinal: [f32; 6],
    /// The warp the nether portal and nausea put on the view, applied to
    pub distortion: Mat4,
    /// `GameRenderer.bobView`: the walking sway, in view space, for the
    pub bob: Mat4,
}

impl WorldDraw {
    /// The vertical field of view `layer` is projected with.
    pub fn fov_for(&self, layer: RenderLayer) -> f32 {
        if layer.has_own_fov() {
            self.hand_fov_y_degrees
        } else {
            self.fov_y_degrees
        }
    }

    /// A chunk's `origin` relative to the camera's block, worked out in
    pub fn offset(&self, origin: glam::IVec3) -> glam::Vec3 {
        (origin.as_dvec3() - self.eye.floor().as_dvec3()).as_vec3()
    }

    /// `model` moved by the camera's block, as [`Self::offset`] moves the
    pub fn relative(&self, model: Mat4) -> Mat4 {
        self.relative_to(model, glam::IVec3::ZERO)
    }

    /// `model`, relative to the block `origin`, made relative to the
    pub fn relative_to(&self, model: Mat4, origin: glam::IVec3) -> Mat4 {
        let blocks = (origin - self.eye.floor().as_ivec3()).as_vec3();
        let mut relative = model;
        relative.w_axis += blocks.extend(0.0) * model.w_axis.w;
        relative
    }

    /// Where the camera is within its block: the part of the view's
    pub fn eye_fraction(&self) -> glam::Vec3 {
        self.eye - self.eye.floor()
    }

    /// This frame's light for a world draw, with `ambient` for its
    pub fn light(&self, ambient: [f32; 4]) -> crate::renderer::pipeline::world::DrawLight {
        crate::renderer::pipeline::world::DrawLight {
            ambient,
            lightmap: self.lightmap,
            fog: crate::renderer::pipeline::world::FogInput::None,
            overlay: 0.0,
            glint_scale: 0.0,
        }
    }
}

/// A run of one chunk mesh - one or more neighbouring sections the culling
pub struct ChunkDraw {
    pub mesh: TerrainMesh,
    /// Where the chunk-local mesh starts in the world, in blocks.
    pub origin: glam::IVec3,
    /// Quads of a [`TerrainMesh::Quads`] mesh, vertices of a
    pub range: std::ops::Range<u32>,
}

impl ChunkDraw {
    /// Vertices this draw shades: four per quad.
    pub fn vertex_count(&self) -> usize {
        let len = self.range.len();
        match self.mesh {
            TerrainMesh::Quads(_) => len * 4,
            TerrainMesh::Vertices(_) => len,
        }
    }
}

/// A chunk mesh: block faces as [`TerrainQuad`]s, or fluids as plain
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TerrainMesh {
    Quads(QuadMeshId),
    Vertices(MeshId),
}

/// Light and ambient occlusion at one corner of a [`TerrainQuad`], in the
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CornerLight {
    pub block: u8,
    pub sky: u8,
    pub ao: u8,
}

impl CornerLight {
    /// Quarter levels in a whole level.
    pub const STEPS_PER_LEVEL: u8 = 4;
    /// AO's unit: `ao / 60`.
    pub const AO_ONE: u8 = 60;

    /// Whole light levels `0..=15`, no occlusion: flat lighting.
    pub fn flat(block: u8, sky: u8) -> Self {
        Self {
            block: block.min(15) * Self::STEPS_PER_LEVEL,
            sky: sky.min(15) * Self::STEPS_PER_LEVEL,
            ao: Self::AO_ONE,
        }
    }
}

/// One block face of a terrain mesh: which baked quad it is (an index into
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TerrainQuad(pub [u32; 5]);

impl TerrainQuad {
    /// Quads the 21-bit index reaches.
    pub const MAX_QUADS: u32 = 1 << 21;
    /// Block heights the 12-bit y reaches.
    pub const MAX_HEIGHT: u32 = 1 << 12;

    /// `index` into the quad table; `pos` chunk-local x, z (`0..16`) and y
    #[inline]
    pub fn new(index: u32, [x, y, z]: [u32; 3], corners: [CornerLight; 4], offset: u16) -> Self {
        debug_assert!(index < Self::MAX_QUADS && x < 16 && z < 16 && y < Self::MAX_HEIGHT);
        let light = |c: CornerLight| u32::from(c.block.min(60)) | u32::from(c.sky.min(60)) << 6;
        let ao = corners
            .iter()
            .enumerate()
            .fold(0u32, |ao, (i, c)| ao | u32::from(c.ao.min(60)) << (6 * i));
        let offset = u32::from(offset);
        Self([
            index | (offset >> 12) << 21 | (ao & 0xFF) << 24,
            x | z << 4 | y << 8 | (offset & 0xFFF) << 20,
            light(corners[0]) | light(corners[1]) << 12 | (ao >> 8 & 0xFF) << 24,
            light(corners[2]) | light(corners[3]) << 12 | (ao >> 16) << 24,
            u32::from_le_bytes([255, 255, 255, 255]),
        ])
    }

    /// [`Self::new`] with a biome colour the shader multiplies onto the
    pub fn tinted(
        index: u32,
        pos: [u32; 3],
        corners: [CornerLight; 4],
        offset: u16,
        tint: [u8; 3],
    ) -> Self {
        let mut quad = Self::new(index, pos, corners, offset);
        quad.0[4] = u32::from_le_bytes([tint[0], tint[1], tint[2], 255]);
        quad
    }

    /// RGB the shader multiplies onto the baked shade.
    pub fn tint(self) -> [u8; 3] {
        let [r, g, b, _] = self.0[4].to_le_bytes();
        [r, g, b]
    }

    pub fn index(self) -> u32 {
        self.0[0] & (Self::MAX_QUADS - 1)
    }

    /// Light and occlusion at corner `i` (`0..4`).
    pub fn corner(self, i: usize) -> CornerLight {
        let light = self.0[2 + i / 2] >> (12 * (i % 2)) & 0xFFF;
        let ao = (self.0[0] >> 24 | (self.0[2] >> 24) << 8 | (self.0[3] >> 24) << 16) >> (6 * i);
        CornerLight {
            block: (light & 63) as u8,
            sky: (light >> 6 & 63) as u8,
            ao: (ao & 63) as u8,
        }
    }

    /// Chunk-local x, y above the base, z.
    pub fn block(self) -> [u32; 3] {
        let w = self.0[1];
        [w & 15, w >> 8 & 0xFFF, w >> 4 & 15]
    }

    /// What [`Self::new`] got as `offset`.
    pub fn offset(self) -> u16 {
        ((self.0[0] >> 21 & 7) << 12 | self.0[1] >> 20) as u16
    }
}

const _: () = assert!(std::mem::size_of::<TerrainQuad>() == 20);

/// One vertex of a terrain or entity mesh: an atlas UV, a colour (the
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldVertex {
    pub pos: [f32; 3],
    /// Atlas or wrapping weather UV.
    pub uv: [f32; 2],
    /// RGB as unsigned normalized bytes; alpha is 255 for terrain, the
    pub color: [u8; 4],
    /// Block then sky in vanilla's light-coordinate units (`UV2`): level
    pub light: [u8; 2],
    /// The direction (`Direction` ordinal, `0..6`) a baked block quad's
    pub face: u8,
    pub _pad: u8,
}

impl WorldVertex {
    /// Above any light coordinate (240): the shader takes `pc.ambient`.
    pub const SENTINEL_LIGHT: [u8; 2] = [255, 255];

    pub fn new(pos: [f32; 3], uv: [f32; 2], color: [f32; 3]) -> Self {
        let unorm8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Self {
            pos,
            uv,
            color: [unorm8(color[0]), unorm8(color[1]), unorm8(color[2]), 255],
            light: Self::SENTINEL_LIGHT,
            face: 0,
            _pad: 0,
        }
    }

    /// [`Self::new`] with an alpha other than opaque.
    pub fn rgba(pos: [f32; 3], uv: [f32; 2], color: [f32; 4]) -> Self {
        let mut v = Self::new(pos, uv, [color[0], color[1], color[2]]);
        v.color[3] = (color[3].clamp(0.0, 1.0) * 255.0).round() as u8;
        v
    }

    /// The direction its shade was taken for.
    pub fn with_face(mut self, face: u8) -> Self {
        self.face = face;
        self
    }

    /// Block and sky light levels `0..=15`.
    pub fn with_light(mut self, block: u8, sky: u8) -> Self {
        self.light = [block.min(15) << 4, sky.min(15) << 4];
        self
    }
}

const _: () = assert!(std::mem::size_of::<WorldVertex>() == 28);

/// A baked quad corner in the quad table `terrain.vert` reads: a
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuadCorner {
    pub pos: [f32; 3],
    pub uv: [u16; 2],
    pub color: [u8; 4],
    /// [`WorldVertex::face`], as the shader's `uint`.
    pub face: u32,
}

impl From<WorldVertex> for QuadCorner {
    fn from(v: WorldVertex) -> Self {
        let unorm16 = |t: f32| (t.clamp(0.0, 1.0) * 65535.0).round() as u16;
        Self {
            pos: v.pos,
            uv: v.uv.map(unorm16),
            color: v.color,
            face: u32::from(v.face),
        }
    }
}

const _: () = assert!(std::mem::size_of::<QuadCorner>() == 24);

/// The world camera's near plane, in blocks (vanilla `GameRenderer.PROJECTION_Z_NEAR`).
pub const WORLD_Z_NEAR: f32 = 0.05;

/// The world camera's projection (Vulkan clip space: depth `0..1`, Y down),
pub fn world_projection(fov_y_degrees: f32, aspect: f32, far: f32) -> Mat4 {
    glam::camera::rh::proj::vulkan::perspective(
        fov_y_degrees.to_radians(),
        aspect,
        far,
        WORLD_Z_NEAR,
    )
}

/// What the depth buffer is cleared to: the far plane, with reversed depth.
pub const DEPTH_CLEAR: f32 = 0.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_is_reversed_and_keeps_millimetres_apart_far_out() {
        let projection = world_projection(70.0, 1.0, 512.0);
        let depth = |distance: f32| {
            let clip = projection * glam::Vec4::new(0.0, 0.0, -distance, 1.0);
            clip.z / clip.w
        };
        assert!((depth(WORLD_Z_NEAR) - 1.0).abs() < 1e-5);
        assert!(depth(512.0).abs() < 1e-5);
        assert!(depth(100.0) > depth(100.001));
    }

    #[test]
    fn neighbouring_chunks_share_their_corners_exactly() {
        let world = WorldDraw {
            chunks: Vec::new(),
            cutout: Vec::new(),
            translucent: Vec::new(),
            atlas: crate::renderer::TextureId(0),
            view: Mat4::IDENTITY,
            eye: glam::Vec3::new(1234.567, 70.31, -987.654),
            fov_y_degrees: 70.0,
            hand_fov_y_degrees: 70.0,
            far: 256.0,
            ambient: [1.0; 3],
            lightmap: crate::world::lightmap::LightmapParams::overworld([1.0; 3], 0.5),
            fog: crate::world::fog::FogData::NONE,
            cardinal: [1.0; 6],
            distortion: Mat4::IDENTITY,
            bob: Mat4::IDENTITY,
        };
        for x in [1216, 1232, -1008, -992] {
            let a = world.offset(glam::IVec3::new(x, 0, -992));
            let b = world.offset(glam::IVec3::new(x + 16, 0, -992));
            let corner = glam::Vec3::new(16.0, 62.888_89, 3.0);
            assert_eq!(corner + a, corner - glam::Vec3::X * 16.0 + b);
        }
        let fraction = world.eye_fraction();
        assert!(fraction.min_element() >= 0.0 && fraction.max_element() < 1.0);
    }

    #[test]
    fn terrain_quads_keep_every_field() {
        let corners = [
            CornerLight {
                block: 60,
                sky: 0,
                ao: 12,
            },
            CornerLight {
                block: 3,
                sky: 57,
                ao: 60,
            },
            CornerLight {
                block: 0,
                sky: 60,
                ao: 0,
            },
            CornerLight {
                block: 41,
                sky: 22,
                ao: 37,
            },
        ];
        let offset = 3 << 12 | 0xABC;
        let quad = TerrainQuad::new(TerrainQuad::MAX_QUADS - 1, [15, 4095, 9], corners, offset);
        assert_eq!(quad.index(), TerrainQuad::MAX_QUADS - 1);
        assert_eq!(quad.block(), [15, 4095, 9]);
        assert_eq!(quad.offset(), offset);
        for (i, corner) in corners.into_iter().enumerate() {
            assert_eq!(quad.corner(i), corner, "corner {i}");
        }
    }

    #[test]
    fn flat_light_is_whole_levels_without_occlusion() {
        let quad = TerrainQuad::new(7, [1, 2, 3], [CornerLight::flat(5, 15); 4], 0);
        for i in 0..4 {
            assert_eq!(
                quad.corner(i),
                CornerLight {
                    block: 20,
                    sky: 60,
                    ao: 60
                }
            );
        }
        assert_eq!(quad.offset(), 0);
    }
}
