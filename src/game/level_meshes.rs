//! The level's terrain on the GPU (vanilla `LevelRenderer`'s section

use std::collections::{HashSet, VecDeque};
use std::ops::Range;

use glam::{IVec3, Vec3};

use crate::math::{ChunkMap, SECTION_SIZE, section_coord};
use crate::renderer::{
    ChunkDraw, MeshId, QuadMeshId, Renderer, TerrainMesh, TerrainQuad, WorldVertex,
};
use crate::world::block::direction::Direction;
use crate::world::block_entity::BlockEntities;
use crate::world::block_models::BlockModels;
use crate::world::culling::Culler;
use crate::world::culling::occlusion::SectionPos;
use crate::world::dimension::Height;
use crate::world::level::{FinishedMesh, Level};
use crate::world::terrain::{self, LayerMesh, SectionMesh, TranslucentMesh};

/// How many mesh bytes go to the GPU per frame at most: a join or a fast
const UPLOAD_BUDGET: usize = 16 << 20;
/// `Octree.isClose`'s 32 blocks: sections this many sections from the
const NEARBY_SECTIONS: i32 = 2;
/// Bytes of sorted-again faces uploaded per frame at most, nearest
const RESORT_BUDGET: usize = 4 << 20;

/// Where section meshes go: the renderer, or ids only for the headless
trait MeshSink {
    fn quads(&mut self, quads: &[TerrainQuad]) -> anyhow::Result<QuadMeshId>;
    fn vertices(&mut self, vertices: &[WorldVertex]) -> anyhow::Result<MeshId>;
    fn free(&mut self, mesh: TerrainMesh);
}

impl MeshSink for Renderer {
    fn quads(&mut self, quads: &[TerrainQuad]) -> anyhow::Result<QuadMeshId> {
        self.upload_quads(quads)
    }

    fn vertices(&mut self, vertices: &[WorldVertex]) -> anyhow::Result<MeshId> {
        self.upload_mesh(vertices)
    }

    fn free(&mut self, mesh: TerrainMesh) {
        match mesh {
            TerrainMesh::Quads(id) => self.free_quads(id),
            TerrainMesh::Vertices(id) => self.free_mesh(id),
        }
    }
}

/// Hands out fresh ids and keeps nothing.
struct Headless<F>(F);

impl<F: FnMut() -> usize> MeshSink for Headless<F> {
    fn quads(&mut self, _: &[TerrainQuad]) -> anyhow::Result<QuadMeshId> {
        Ok(QuadMeshId((self.0)()))
    }

    fn vertices(&mut self, _: &[WorldVertex]) -> anyhow::Result<MeshId> {
        Ok(MeshId((self.0)()))
    }

    fn free(&mut self, _: TerrainMesh) {}
}

/// A section's solid or cutout quads on the GPU, by facing group.
struct QuadLayer {
    mesh: QuadMeshId,
    groups: [Range<u32>; terrain::FACINGS],
}

/// A section's translucent faces on the GPU, and the list they came from
struct Translucent {
    mesh: MeshId,
    faces: TranslucentMesh,
    /// `TranslucencyPointOfView`: where the camera's section was, relative
    view: IVec3,
    /// The camera's block when they were sorted: built on a worker, that
    block: IVec3,
}

/// One section's meshes; `None` for a layer with nothing to draw.
#[derive(Default)]
struct SectionMeshes {
    solid: Option<QuadLayer>,
    cutout: Option<QuadLayer>,
    /// Lava vertices and their count.
    lava: Option<(MeshId, u32)>,
    translucent: Option<Translucent>,
}

impl SectionMeshes {
    /// Uploads every layer; on a failure frees what went up.
    fn upload(
        pos: SectionPos,
        built: SectionMesh,
        sink: &mut impl MeshSink,
    ) -> anyhow::Result<Self> {
        let mut out = Self::default();
        let result = out.fill(pos, built, sink);
        match result {
            Ok(()) => Ok(out),
            Err(e) => {
                out.free(sink);
                Err(e)
            }
        }
    }

    fn fill(
        &mut self,
        pos: SectionPos,
        built: SectionMesh,
        sink: &mut impl MeshSink,
    ) -> anyhow::Result<()> {
        let quads = |layer: LayerMesh, sink: &mut _| -> anyhow::Result<Option<QuadLayer>> {
            if layer.items.is_empty() {
                return Ok(None);
            }
            Ok(Some(QuadLayer {
                mesh: MeshSink::quads(sink, &layer.items)?,
                groups: layer.groups,
            }))
        };
        self.solid = quads(built.solid, sink)?;
        self.cutout = quads(built.cutout, sink)?;
        if !built.lava.items.is_empty() {
            self.lava = Some((
                sink.vertices(&built.lava.items)?,
                built.lava.items.len() as u32,
            ));
        }
        let faces = built.translucent;
        if !faces.vertices.is_empty() {
            let eye = faces.sorted_from + corner_of(pos);
            self.translucent = Some(Translucent {
                mesh: sink.vertices(&faces.vertices)?,
                view: point_of_view(section_of_eye(eye, built.base_y), pos),
                block: eye.floor().as_ivec3(),
                faces,
            });
        }
        Ok(())
    }

    fn is_empty(&self) -> bool {
        self.solid.is_none()
            && self.cutout.is_none()
            && self.lava.is_none()
            && self.translucent.is_none()
    }

    fn free(self, sink: &mut impl MeshSink) {
        for layer in [self.solid, self.cutout].into_iter().flatten() {
            sink.free(TerrainMesh::Quads(layer.mesh));
        }
        if let Some((mesh, _)) = self.lava {
            sink.free(TerrainMesh::Vertices(mesh));
        }
        if let Some(translucent) = self.translucent {
            sink.free(TerrainMesh::Vertices(translucent.mesh));
        }
    }
}

/// A chunk column's sections.
#[derive(Default)]
struct Column {
    /// By section, bottom to top.
    sections: Vec<SectionMeshes>,
    /// Bit `y` set for each section with anything to draw.
    filled: u32,
    /// Bit `y` set for each section a mesh came for, empty or not
    compiled: u32,
}

/// The chunk-local origin of section `pos`'s vertices: its chunk's corner,
fn corner_of((x, _, z): SectionPos) -> Vec3 {
    Vec3::new((x * SECTION_SIZE) as f32, 0.0, (z * SECTION_SIZE) as f32)
}

/// The section `eye` is in, `y` counted from the world's bottom `min_y`.
fn section_of_eye(eye: Vec3, min_y: i32) -> IVec3 {
    let block = eye.floor().as_ivec3();
    IVec3::new(
        section_coord(block.x),
        section_coord(block.y - min_y),
        section_coord(block.z),
    )
}

/// `TranslucencyPointOfView`: the camera's section relative to `pos`, each
fn point_of_view(camera: IVec3, (x, y, z): SectionPos) -> IVec3 {
    (camera - IVec3::new(x, y, z)).clamp(IVec3::NEG_ONE, IVec3::ONE)
}

/// This frame's section draws, per layer. Solid and cutout ones are sorted
#[derive(Default)]
pub struct VisibleChunks {
    pub solid: Vec<ChunkDraw>,
    pub cutout: Vec<ChunkDraw>,
    pub translucent: Vec<ChunkDraw>,
    pub count: SectionCount,
}

pub struct LevelMeshes {
    /// The world's bottom, where section 0 starts.
    min_y: i32,
    columns: ChunkMap<Column>,
    /// Built meshes over the upload budget, uploaded in order on the next
    waiting: VecDeque<FinishedMesh>,
    /// Draws with their distance, sorted before they go out: solid ones
    solid: SortedDraws,
    cutout: SortedDraws,
    translucent: SortedDraws,
    /// Last frame's draw lists, handed back by [`Self::recycle`] to be
    spare_solid: Vec<ChunkDraw>,
    spare_cutout: Vec<ChunkDraw>,
    spare_translucent: Vec<ChunkDraw>,
    /// The sections with translucent faces the last frame drew, for
    seen_translucent: Vec<SectionPos>,
}

impl Default for LevelMeshes {
    fn default() -> Self {
        Self {
            min_y: Height::OVERWORLD.min_y,
            columns: ChunkMap::default(),
            waiting: VecDeque::new(),
            solid: SortedDraws::default(),
            cutout: SortedDraws::default(),
            translucent: SortedDraws::default(),
            spare_solid: Vec::new(),
            spare_cutout: Vec::new(),
            spare_translucent: Vec::new(),
            seen_translucent: Vec::new(),
        }
    }
}

impl LevelMeshes {
    /// The level's bottom: where its first section starts. Set when the
    pub fn set_min_y(&mut self, min_y: i32) {
        self.min_y = min_y;
    }

    /// Uploads finished meshes in place of the sections' old ones: the
    pub fn upload(&mut self, renderer: &mut Renderer, finished: Vec<FinishedMesh>) {
        self.upload_to(renderer, finished);
    }

    fn upload_to(&mut self, sink: &mut impl MeshSink, finished: Vec<FinishedMesh>) {
        if !finished.is_empty() && !self.waiting.is_empty() {
            let newer: crate::math::SectionSet = finished.iter().map(|mesh| mesh.pos).collect();
            self.waiting.retain(|waiting| !newer.contains(&waiting.pos));
        }
        for mesh in finished {
            if mesh.sync {
                self.install(sink, mesh);
            } else {
                self.waiting.push_back(mesh);
            }
        }
        let mut bytes = 0;
        while bytes < UPLOAD_BUDGET {
            let Some(mesh) = self.waiting.pop_front() else {
                break;
            };
            bytes += mesh.mesh.bytes();
            self.install(sink, mesh);
        }
    }

    /// Whether a mesh was ever built for the section, drawn or not
    pub fn section_compiled(&self, (x, y, z): SectionPos) -> bool {
        usize::try_from(y)
            .ok()
            .filter(|&y| y < 32)
            .is_some_and(|y| {
                self.columns
                    .get(&(x, z))
                    .is_some_and(|column| column.compiled & (1 << y) != 0)
            })
    }

    /// Whether the section has anything to draw
    /// (`SectionMesh.hasRenderableLayers`).
    pub fn section_filled(&self, (x, y, z): SectionPos) -> bool {
        usize::try_from(y)
            .ok()
            .filter(|&y| y < 32)
            .is_some_and(|y| {
                self.columns
                    .get(&(x, z))
                    .is_some_and(|column| column.filled & (1 << y) != 0)
            })
    }

    /// Each chunk column with something to draw and which of its sections
    /// do (bit per section): what [`Self::visible_sections`] culls, a
    /// column's box first, then its sections.
    pub fn filled_columns(&self) -> impl Iterator<Item = ((i32, i32), u32)> + '_ {
        self.columns
            .iter()
            .filter(|(_, c)| c.filled != 0)
            .map(|(&pos, c)| (pos, c.filled))
    }

    /// Puts a built section in place of its old meshes.
    fn install(&mut self, sink: &mut impl MeshSink, finished: FinishedMesh) {
        let FinishedMesh { pos, mesh, .. } = finished;
        let Ok(y) = usize::try_from(pos.1) else {
            return;
        };
        if y >= 32 {
            return;
        }
        let meshes = match SectionMeshes::upload(pos, mesh, sink) {
            Ok(meshes) => meshes,
            Err(e) => {
                eprintln!("Section-Mesh konnte nicht hochgeladen werden: {e:#}");
                SectionMeshes::default()
            }
        };
        let column = self.columns.entry((pos.0, pos.2)).or_default();
        if column.sections.len() <= y {
            column.sections.resize_with(y + 1, SectionMeshes::default);
        }
        column.compiled |= 1 << y;
        if meshes.is_empty() {
            column.filled &= !(1 << y);
        } else {
            column.filled |= 1 << y;
        }
        std::mem::replace(&mut column.sections[y], meshes).free(sink);
    }

    /// Takes a section's meshes without a GPU, `ids` handing out an id for
    pub fn insert_headless(&mut self, mesh: FinishedMesh, ids: impl FnMut() -> usize) {
        self.install(&mut Headless(ids), mesh);
    }

    /// A chunk was unloaded or replaced: its meshes go.
    pub fn remove(&mut self, renderer: &mut Renderer, key: (i32, i32)) {
        self.waiting
            .retain(|waiting| (waiting.pos.0, waiting.pos.2) != key);
        if let Some(column) = self.columns.remove(&key) {
            for section in column.sections {
                section.free(renderer);
            }
        }
    }

    /// Frees every mesh. Call when leaving the world.
    pub fn clear(&mut self, renderer: &mut Renderer) {
        self.waiting.clear();
        self.seen_translucent.clear();
        for (_, column) in self.columns.drain() {
            for section in column.sections {
                section.free(renderer);
            }
        }
    }

    /// Sorts the translucent faces of sections the camera now sees from
    pub fn resort(&mut self, renderer: &mut Renderer, eye: Vec3) {
        self.resort_with(renderer, eye);
    }

    fn resort_with(&mut self, sink: &mut impl MeshSink, eye: Vec3) {
        let block = eye.floor().as_ivec3();
        let seen = std::mem::take(&mut self.seen_translucent);
        let camera = section_of_eye(eye, self.min_y);
        let mut due = Vec::new();
        let mut queued = HashSet::new();
        for &pos in &seen {
            if self.due_to_resort(pos, eye, camera, block, true) {
                queued.insert(pos);
                let offset = IVec3::new(pos.0, pos.1, pos.2) - camera;
                let nearby = offset.abs().max_element() <= NEARBY_SECTIONS;
                due.push((offset.length_squared(), pos, nearby));
            }
        }
        for dx in -NEARBY_SECTIONS..=NEARBY_SECTIONS {
            for dy in -NEARBY_SECTIONS..=NEARBY_SECTIONS {
                for dz in -NEARBY_SECTIONS..=NEARBY_SECTIONS {
                    let pos = (camera.x + dx, camera.y + dy, camera.z + dz);
                    if queued.contains(&pos) {
                        continue;
                    }
                    if self.due_to_resort(pos, eye, camera, block, false) {
                        queued.insert(pos);
                        due.push((IVec3::new(dx, dy, dz).length_squared(), pos, true));
                    }
                }
            }
        }
        due.sort_unstable_by_key(|&(distance, ..)| distance);
        let mut bytes = 0;
        for (_, pos, nearby) in due {
            if nearby || bytes < RESORT_BUDGET {
                bytes += self.resort_section(sink, pos, eye, camera);
            }
        }
        self.seen_translucent = seen;
    }

    /// Vanilla's block / point-of-view tests when `from_seen`, plus a face
    fn due_to_resort(
        &self,
        pos: SectionPos,
        eye: Vec3,
        camera: IVec3,
        block: IVec3,
        from_seen: bool,
    ) -> bool {
        let Some(translucent) = self.translucent(pos) else {
            return false;
        };
        if translucent
            .faces
            .camera_crossed_a_face(eye - corner_of(pos))
        {
            return true;
        }
        if !from_seen {
            return false;
        }
        let view = point_of_view(camera, pos);
        let offset = IVec3::new(pos.0, pos.1, pos.2) - camera;
        let nearby = offset.abs().max_element() <= NEARBY_SECTIONS;
        let moved = block.cmpne(translucent.block);
        let lined_up = (0..3).any(|axis| moved.test(axis) && view[axis] == 0);
        view != translucent.view || (moved.any() && nearby) || lined_up
    }

    fn translucent(&self, (x, y, z): SectionPos) -> Option<&Translucent> {
        self.columns
            .get(&(x, z))
            .and_then(|column| column.sections.get(y as usize))
            .and_then(|section| section.translucent.as_ref())
    }

    fn translucent_mut(&mut self, (x, y, z): SectionPos) -> Option<&mut Translucent> {
        self.columns
            .get_mut(&(x, z))
            .and_then(|column| column.sections.get_mut(y as usize))
            .and_then(|section| section.translucent.as_mut())
    }

    /// Sorts one section's translucent faces from `eye` and uploads them
    fn resort_section(
        &mut self,
        sink: &mut impl MeshSink,
        pos: SectionPos,
        eye: Vec3,
        camera: IVec3,
    ) -> usize {
        let Some(translucent) = self.translucent_mut(pos) else {
            return 0;
        };
        translucent.faces.sort(eye - corner_of(pos));
        translucent.view = point_of_view(camera, pos);
        translucent.block = eye.floor().as_ivec3();
        match sink.vertices(&translucent.faces.vertices) {
            Ok(mesh) => {
                sink.free(TerrainMesh::Vertices(std::mem::replace(
                    &mut translucent.mesh,
                    mesh,
                )));
            }
            Err(e) => eprintln!("Translucent-Mesh konnte nicht neu sortiert werden: {e:#}"),
        }
        std::mem::size_of_val(translucent.faces.vertices.as_slice())
    }

    /// The parts of meshes `culler` lets through: per section, its solid
    pub fn visible_sections(&mut self, culler: &Culler, eye: Vec3) -> VisibleChunks {
        let mut visible = VisibleChunks {
            solid: std::mem::take(&mut self.spare_solid),
            cutout: std::mem::take(&mut self.spare_cutout),
            translucent: std::mem::take(&mut self.spare_translucent),
            count: SectionCount::default(),
        };
        visible.solid.clear();
        visible.cutout.clear();
        visible.translucent.clear();
        self.solid.clear();
        self.cutout.clear();
        self.translucent.clear();
        self.seen_translucent.clear();
        let min_y = self.min_y;
        let vertical = |group: usize| {
            (0..32).fold(0u32, |mask, y| {
                mask | u32::from(faces_camera(group, Vec3::ZERO, y, eye, min_y)) << y
            })
        };
        let up = vertical(Direction::Up.index());
        let down = vertical(Direction::Down.index());
        for (&(cx, cz), column) in &self.columns {
            let filled = column.filled;
            if filled == 0 {
                continue;
            }
            visible.count.total += filled.count_ones() as usize;
            let (x, z) = ((cx * SECTION_SIZE) as f32, (cz * SECTION_SIZE) as f32);
            let bottom = |y: u32| (min_y + y as i32 * SECTION_SIZE) as f32;
            let min = Vec3::new(x, bottom(filled.trailing_zeros()), z);
            let max = Vec3::new(
                x + SECTION_SIZE as f32,
                bottom(32 - filled.leading_zeros()),
                z + SECTION_SIZE as f32,
            );
            if !culler.sees_box(min, max) {
                continue;
            }
            let framed = culler.frames_box(min, max);
            let origin = Vec3::new(x, 0.0, z);
            let sides: [bool; terrain::FACINGS] =
                std::array::from_fn(|group| faces_camera(group, origin, 0, eye, min_y));
            let corner = (cx * SECTION_SIZE, cz * SECTION_SIZE);
            let quads_origin = IVec3::new(corner.0, min_y, corner.1);
            let vertices_origin = IVec3::new(corner.0, 0, corner.1);
            let mut rest = filled;
            while rest != 0 {
                let y = rest.trailing_zeros();
                rest &= rest - 1;
                if !culler.sees_section_within((cx, y as i32, cz), framed) {
                    continue;
                }
                let Some(section) = column.sections.get(y as usize) else {
                    continue;
                };
                visible.count.drawn += 1;
                let center = Vec3::new(x + 8.0, bottom(y) + 8.0, z + 8.0);
                let distance = center.distance_squared(eye);
                let shown = |group: usize| {
                    if group == Direction::Up.index() {
                        up >> y & 1 != 0
                    } else if group == Direction::Down.index() {
                        down >> y & 1 != 0
                    } else {
                        sides[group]
                    }
                };
                for (layer, list) in [
                    (&section.solid, &mut self.solid),
                    (&section.cutout, &mut self.cutout),
                ] {
                    let Some(layer) = layer else { continue };
                    let mut run: Option<Range<u32>> = None;
                    let mut flush = |run: &mut Option<Range<u32>>| {
                        if let Some(range) = run.take() {
                            let mesh = TerrainMesh::Quads(layer.mesh);
                            list.push(
                                distance,
                                ChunkDraw {
                                    mesh,
                                    origin: quads_origin,
                                    range,
                                },
                            );
                        }
                    };
                    for (group, range) in layer.groups.iter().enumerate() {
                        if range.is_empty() {
                            continue;
                        }
                        if !shown(group) {
                            flush(&mut run);
                            continue;
                        }
                        match &mut run {
                            Some(open) if open.end == range.start => open.end = range.end,
                            _ => {
                                flush(&mut run);
                                run = Some(range.clone());
                            }
                        }
                    }
                    flush(&mut run);
                }
                if let Some((mesh, count)) = section.lava {
                    let mesh = TerrainMesh::Vertices(mesh);
                    self.solid.push(
                        distance,
                        ChunkDraw {
                            mesh,
                            origin: vertices_origin,
                            range: 0..count,
                        },
                    );
                }
                if let Some(translucent) = &section.translucent {
                    self.seen_translucent.push((cx, y as i32, cz));
                    let mesh = TerrainMesh::Vertices(translucent.mesh);
                    self.translucent.push_far_first(
                        distance,
                        ChunkDraw {
                            mesh,
                            origin: vertices_origin,
                            range: 0..translucent.faces.vertices.len() as u32,
                        },
                    );
                }
            }
        }
        for draw in self
            .solid
            .draws
            .iter()
            .chain(&self.cutout.draws)
            .chain(&self.translucent.draws)
        {
            visible.count.vertices += draw.vertex_count();
            visible.count.draws += 1;
        }
        self.solid.drain_sorted_into(&mut visible.solid);
        self.cutout.drain_sorted_into(&mut visible.cutout);
        self.translucent.drain_sorted_into(&mut visible.translucent);
        visible
    }

    /// Takes back the draw lists [`Self::visible_sections`] handed out,
    pub fn recycle(
        &mut self,
        solid: Vec<ChunkDraw>,
        cutout: Vec<ChunkDraw>,
        translucent: Vec<ChunkDraw>,
    ) {
        self.spare_solid = solid;
        self.spare_cutout = cutout;
        self.spare_translucent = translucent;
    }
}

/// One layer's draws in the order culling found them, and a sort key for
/// each ([`draw_key`]). Sorting the 8-byte keys and then picking the draws
/// moves a sixth of the bytes that sorting the draws themselves did.
#[derive(Default)]
struct SortedDraws {
    draws: Vec<ChunkDraw>,
    keys: Vec<u64>,
}

impl SortedDraws {
    /// Adds `draw`, `distance` (squared) from the eye, near ones first.
    fn push(&mut self, distance: f32, draw: ChunkDraw) {
        self.keys.push(draw_key(distance, self.draws.len()));
        self.draws.push(draw);
    }

    /// Adds `draw`, far ones first: translucent faces blend back to front.
    fn push_far_first(&mut self, distance: f32, draw: ChunkDraw) {
        self.keys
            .push(draw_key(distance, self.draws.len()) ^ !0 << 32);
        self.draws.push(draw);
    }

    fn clear(&mut self) {
        self.draws.clear();
        self.keys.clear();
    }

    /// Appends the draws to `out` in key order and empties `self`.
    fn drain_sorted_into(&mut self, out: &mut Vec<ChunkDraw>) {
        self.keys.sort_unstable();
        out.reserve(self.keys.len());
        out.extend(
            self.keys
                .iter()
                .map(|&key| self.draws[(key & 0xFFFF_FFFF) as usize].clone()),
        );
        self.clear();
    }
}

/// A draw's place in the near-to-far order, as one integer: the squared
/// distance's bits on top (a non-negative float orders like its bits),
/// then where the draw was found, which also tells the draw apart.
fn draw_key(distance: f32, index: usize) -> u64 {
    u64::from(distance.max(0.0).to_bits()) << 32 | index as u64 & 0xFFFF_FFFF
}

/// What the culling let through this frame, for F3's `C:` and `V:` lines.
#[derive(Clone, Copy, Default)]
pub struct SectionCount {
    pub drawn: usize,
    pub total: usize,
    /// Terrain vertices and draw calls this frame, all layers.
    pub vertices: usize,
    pub draws: usize,
}

/// Whether some face of facing group `group` in section `y` of the chunk at
fn faces_camera(group: usize, origin: Vec3, y: usize, eye: Vec3, min_y: i32) -> bool {
    let size = SECTION_SIZE as f32;
    let min = Vec3::new(origin.x, (min_y + y as i32 * SECTION_SIZE) as f32, origin.z);
    let max = min + Vec3::splat(size);
    let Some(direction) = Direction::ALL.get(group) else {
        return true;
    };
    let axis = direction.axis();
    if direction.is_positive() {
        eye[axis] > min[axis]
    } else {
        eye[axis] < max[axis]
    }
}

/// A vertex list on the GPU, uploaded again only when it changed.
#[derive(Default)]
struct ChangingMesh {
    mesh: Option<MeshId>,
    /// What `mesh` holds.
    uploaded: Vec<WorldVertex>,
    /// The latest build, kept between builds for its allocation.
    building: Vec<WorldVertex>,
}

impl ChangingMesh {
    /// Builds the vertices with `build` and uploads them if they differ
    fn update(
        &mut self,
        renderer: &mut Renderer,
        build: impl FnOnce(&mut Vec<WorldVertex>),
    ) -> Option<MeshId> {
        self.building.clear();
        build(&mut self.building);
        if self.building != self.uploaded {
            std::mem::swap(&mut self.building, &mut self.uploaded);
            if let Some(old) = self.mesh.take() {
                renderer.free_mesh(old);
            }
            if !self.uploaded.is_empty() {
                match renderer.upload_mesh(&self.uploaded) {
                    Ok(mesh) => self.mesh = Some(mesh),
                    Err(e) => {
                        eprintln!("Block-Entity-Mesh fehlgeschlagen: {e:#}");
                        self.uploaded.clear();
                    }
                }
            }
        }
        self.mesh
    }

    fn clear(&mut self, renderer: &mut Renderer) {
        if let Some(mesh) = self.mesh.take() {
            renderer.free_mesh(mesh);
        }
        self.uploaded.clear();
    }
}

/// How far the camera moves before the still block entities are gathered
const STILL_STEP: f32 = 8.0;

/// The block entities' geometry (chests, bells, books...), in two meshes:
#[derive(Default)]
pub struct BlockEntityMeshes {
    still: ChangingMesh,
    /// The level's and the animations' revision and the camera position
    still_for: Option<(u64, u64, Vec3)>,
    /// The block `still`'s vertices are relative to.
    still_origin: IVec3,
    moving: ChangingMesh,
}

/// Makes `vertices` relative to the block `origin`, so they are drawn
fn shift(vertices: &mut [WorldVertex], origin: IVec3) {
    let origin = origin.as_vec3();
    for vertex in vertices {
        vertex.pos = (Vec3::from_array(vertex.pos) - origin).to_array();
    }
}

impl BlockEntityMeshes {
    /// This frame's meshes, rebuilt as needed, each with the block its
    pub fn update(
        &mut self,
        renderer: &mut Renderer,
        block_entities: &BlockEntities,
        level: &Level,
        models: &BlockModels,
        culler: &Culler,
        partial: f32,
    ) -> [Option<(MeshId, IVec3)>; 2] {
        let eye = culler.eye();
        let origin = eye.floor().as_ivec3();
        let revisions = (level.revision(), block_entities.still_revision());
        let stale = self.still_for.is_none_or(|(level_rev, anim_rev, center)| {
            (level_rev, anim_rev) != revisions || center.distance_squared(eye) > STILL_STEP.powi(2)
        });
        if stale {
            self.still.update(renderer, |out| {
                block_entities.build_still(level, models, eye, out);
                shift(out, origin);
            });
            self.still_for = Some((revisions.0, revisions.1, eye));
            self.still_origin = origin;
        }
        let moving = self.moving.update(renderer, |out| {
            block_entities.build_moving(level, models, culler, eye, partial, out);
            shift(out, origin);
        });
        [
            self.still.mesh.map(|mesh| (mesh, self.still_origin)),
            moving.map(|mesh| (mesh, origin)),
        ]
    }

    /// Frees the meshes. Call when leaving the world.
    pub fn clear(&mut self, renderer: &mut Renderer) {
        self.still.clear(renderer);
        self.still_for = None;
        self.moving.clear(renderer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draw_keys_order_like_distance_then_index() {
        let mut seed = 0x0bad_5eed_1234_5678u64;
        let mut random = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let draws: Vec<(f32, usize)> = (0..400)
            .map(|index| ((random() % 50) as f32 * 37.25, index))
            .collect();
        for a in &draws {
            for b in &draws {
                let by_compare = a.0.total_cmp(&b.0).then(a.1.cmp(&b.1));
                assert_eq!(draw_key(a.0, a.1).cmp(&draw_key(b.0, b.1)), by_compare);
                let far_first = b.0.total_cmp(&a.0).then(a.1.cmp(&b.1));
                let far = |d: &(f32, usize)| draw_key(d.0, d.1) ^ !0 << 32;
                assert_eq!(far(a).cmp(&far(b)), far_first);
            }
        }
    }

    #[test]
    fn sorted_draws_come_out_like_a_stable_sort_by_distance() {
        let draw = |id: usize| ChunkDraw {
            mesh: TerrainMesh::Quads(QuadMeshId(id)),
            origin: IVec3::new(id as i32, 0, 0),
            range: 0..id as u32,
        };
        let distances = [9.0, 1.0, 4.0, 1.0, 0.0, 9.0, 2.5];
        let mut near = SortedDraws::default();
        let mut far = SortedDraws::default();
        for (id, &distance) in distances.iter().enumerate() {
            near.push(distance, draw(id));
            far.push_far_first(distance, draw(id));
        }
        let mut by_sort: Vec<(f32, usize)> = distances.iter().copied().zip(0..).collect();
        by_sort.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut out = Vec::new();
        near.drain_sorted_into(&mut out);
        let expected: Vec<ChunkDraw> = by_sort.iter().map(|&(_, id)| draw(id)).collect();
        assert_eq!(out, expected);
        by_sort.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut out = Vec::new();
        far.drain_sorted_into(&mut out);
        let expected: Vec<ChunkDraw> = by_sort.iter().map(|&(_, id)| draw(id)).collect();
        assert_eq!(out, expected);
        assert!(near.draws.is_empty() && near.keys.is_empty());
    }

    /// A section at `pos` with two flat translucent quads, at heights
    fn two_quads(pos: SectionPos, low: f32, high: f32, eye: Vec3) -> FinishedMesh {
        let quad = |y: f32| {
            [[0.0, y, 0.0], [0.0, y, 1.0], [1.0, y, 1.0], [1.0, y, 0.0]]
                .map(|p| WorldVertex::new(p, [0.0, 0.0], [1.0; 3]))
        };
        let mut faces = TranslucentMesh {
            vertices: [quad(low), quad(high)].concat(),
            ..TranslucentMesh::default()
        };
        faces.sort(eye - corner_of(pos));
        FinishedMesh {
            pos,
            mesh: SectionMesh {
                base_y: Height::OVERWORLD.min_y,
                translucent: faces,
                ..SectionMesh::default()
            },
            sync: true,
        }
    }

    /// The heights of a section's translucent quads, in drawing order.
    fn heights(meshes: &LevelMeshes, pos: SectionPos) -> Vec<f32> {
        let translucent = meshes.columns[&(pos.0, pos.2)].sections[pos.1 as usize]
            .translucent
            .as_ref()
            .unwrap();
        translucent
            .faces
            .vertices
            .chunks(4)
            .map(|q| q[0].pos[1])
            .collect()
    }

    #[test]
    fn the_point_of_view_is_clamped_to_the_neighbours() {
        assert_eq!(
            point_of_view(IVec3::new(5, 4, -3), (0, 4, 0)),
            IVec3::new(1, 0, -1)
        );
        assert_eq!(
            section_of_eye(Vec3::new(-0.5, -64.0, 15.9), Height::OVERWORLD.min_y),
            IVec3::new(-1, 0, 0)
        );
    }

    #[test]
    fn translucent_faces_are_sorted_again_as_the_camera_moves() {
        let mut next = 0;
        let mut sink = Headless(|| {
            next += 1;
            next
        });
        let pos = (0, 4, 0);
        let above = Vec3::new(0.5, 40.0, 0.5);
        let mut meshes = LevelMeshes::default();
        meshes.upload_to(&mut sink, vec![two_quads(pos, 2.0, 12.0, above)]);
        assert_eq!(heights(&meshes, pos), [2.0, 12.0]);
        meshes.seen_translucent = vec![pos];
        meshes.resort_with(&mut sink, above);
        meshes.resort_with(&mut sink, Vec3::new(0.5, -30.0, 0.5));
        assert_eq!(heights(&meshes, pos), [12.0, 2.0]);
    }

    #[test]
    fn an_unchanged_view_is_not_sorted_again() {
        let mut next = 0;
        let mut sink = Headless(|| {
            next += 1;
            next
        });
        let pos = (40, 4, 40);
        let eye = Vec3::new(0.5, 40.0, 0.5);
        let mut meshes = LevelMeshes::default();
        meshes.upload_to(&mut sink, vec![two_quads(pos, 2.0, 12.0, eye)]);
        meshes.seen_translucent = vec![pos];
        let mesh = |meshes: &LevelMeshes| {
            meshes.columns[&(40, 40)].sections[4]
                .translucent
                .as_ref()
                .unwrap()
                .mesh
        };
        let before = mesh(&meshes);
        meshes.resort_with(&mut sink, eye);
        meshes.resort_with(&mut sink, eye + Vec3::X);
        assert_eq!(mesh(&meshes), before);
    }

    #[test]
    fn diving_within_a_section_sorts_far_sections_again() {
        let mut next = 0;
        let mut sink = Headless(|| {
            next += 1;
            next
        });
        let pos = (10, 4, 0);
        let above = Vec3::new(0.5, 13.5, 8.5);
        let mut meshes = LevelMeshes::default();
        meshes.upload_to(&mut sink, vec![two_quads(pos, 2.0, 12.0, above)]);
        meshes.seen_translucent = vec![pos];
        meshes.resort_with(&mut sink, above);
        assert_eq!(heights(&meshes, pos), [2.0, 12.0]);
        meshes.resort_with(&mut sink, Vec3::new(0.5, 6.5, 8.5));
        assert_eq!(heights(&meshes, pos), [12.0, 2.0]);
    }

    #[test]
    fn a_mesh_sorted_before_the_camera_moved_is_sorted_again() {
        let mut next = 0;
        let mut sink = Headless(|| {
            next += 1;
            next
        });
        let pos = (10, 4, 0);
        let below = Vec3::new(0.5, 6.5, 8.5);
        let mut meshes = LevelMeshes::default();
        meshes.resort_with(&mut sink, below);
        meshes.upload_to(
            &mut sink,
            vec![two_quads(pos, 2.0, 12.0, Vec3::new(0.5, 13.5, 8.5))],
        );
        meshes.seen_translucent = vec![pos];
        meshes.resort_with(&mut sink, below);
        assert_eq!(heights(&meshes, pos), [12.0, 2.0]);
    }

    #[test]
    fn diving_past_the_water_plane_in_the_same_block_sorts_again() {
        let mut next = 0;
        let mut sink = Headless(|| {
            next += 1;
            next
        });
        let pos = (0, 4, 0);
        let above = Vec3::new(0.5, 8.95, 0.5);
        let mut meshes = LevelMeshes::default();
        meshes.upload_to(&mut sink, vec![two_quads(pos, 8.888889, 9.0, above)]);
        meshes.seen_translucent = vec![pos];
        meshes.resort_with(&mut sink, above);
        assert_eq!(heights(&meshes, pos), [8.888889, 9.0]);
        meshes.resort_with(&mut sink, Vec3::new(0.5, 8.5, 0.5));
        assert_eq!(heights(&meshes, pos), [9.0, 8.888889]);
    }

    #[test]
    fn looking_up_after_a_dive_sorts_ice_that_was_not_seen() {
        let mut next = 0;
        let mut sink = Headless(|| {
            next += 1;
            next
        });
        let pos = (0, 4, 0);
        let above = Vec3::new(0.5, 8.95, 0.5);
        let mut meshes = LevelMeshes::default();
        meshes.upload_to(&mut sink, vec![two_quads(pos, 8.888889, 9.0, above)]);
        assert_eq!(heights(&meshes, pos), [8.888889, 9.0]);
        meshes.resort_with(&mut sink, Vec3::new(0.5, 8.5, 0.5));
        assert_eq!(heights(&meshes, pos), [9.0, 8.888889]);
    }

    #[test]
    fn a_newer_mesh_replaces_the_one_waiting() {
        let mut sink = Headless(|| 0);
        let mut meshes = LevelMeshes::default();
        let eye = Vec3::ZERO;
        let mut waiting = two_quads((0, 4, 0), 2.0, 12.0, eye);
        waiting.sync = false;
        meshes.waiting.push_back(waiting);
        let newer = FinishedMesh {
            sync: false,
            ..two_quads((0, 4, 0), 3.0, 13.0, eye)
        };
        meshes.upload_to(&mut sink, vec![newer]);
        assert!(meshes.waiting.is_empty());
        assert!(heights(&meshes, (0, 4, 0)).contains(&3.0));
    }

    #[test]
    fn faces_are_seen_only_from_their_front_side() {
        let origin = Vec3::new(16.0, 0.0, 32.0);
        let up = Direction::Up.index();
        let down = Direction::Down.index();
        let east = Direction::East.index();
        assert!(faces_camera(
            up,
            origin,
            5,
            Vec3::new(20.0, 40.0, 40.0),
            Height::OVERWORLD.min_y
        ));
        assert!(!faces_camera(
            up,
            origin,
            5,
            Vec3::new(20.0, 10.0, 40.0),
            Height::OVERWORLD.min_y
        ));
        assert!(faces_camera(
            down,
            origin,
            5,
            Vec3::new(20.0, 10.0, 40.0),
            Height::OVERWORLD.min_y
        ));
        assert!(faces_camera(
            up,
            origin,
            5,
            Vec3::new(20.0, 20.0, 40.0),
            Height::OVERWORLD.min_y
        ));
        assert!(faces_camera(
            down,
            origin,
            5,
            Vec3::new(20.0, 20.0, 40.0),
            Height::OVERWORLD.min_y
        ));
        assert!(!faces_camera(
            east,
            origin,
            5,
            Vec3::new(10.0, 20.0, 40.0),
            Height::OVERWORLD.min_y
        ));
        assert!(faces_camera(
            terrain::SLANTED,
            origin,
            5,
            Vec3::ZERO,
            Height::OVERWORLD.min_y
        ));
    }
}
