//! Where terrain and entity meshes live on the GPU: a few large

use ash::vk;

use super::create_buffer;
use super::staging::Staging;
use super::vk_util::{DEVICE_OR_HOST, create_buffer_in};

/// Bytes per arena block. A mesh larger than that gets a block of its own
const BLOCK_BYTES: u64 = 64 << 20;
/// A staging buffer up to this size is kept however little of it is used.
const STAGING_KEPT: u64 = 4 << 20;
/// Quads one indexed draw can reach with `u16` indices.
pub const QUADS_PER_DRAW: u32 = 1 << 14;
/// `0, 1, 2, 0, 2, 3` per quad: both triangles counter-clockwise.
const QUAD_INDICES: [u16; 6] = [0, 1, 2, 0, 2, 3];

/// First-fit free list over a block's vertex range, coalescing on free.
struct FreeList {
    /// Free `(offset, len)` regions, sorted by offset, never adjacent.
    free: Vec<(u32, u32)>,
}

impl FreeList {
    fn new(capacity: u32) -> Self {
        Self {
            free: vec![(0, capacity)],
        }
    }

    fn alloc(&mut self, n: u32) -> Option<u32> {
        let i = self.free.iter().position(|&(_, len)| len >= n)?;
        let (offset, len) = self.free[i];
        if len == n {
            self.free.remove(i);
        } else {
            self.free[i] = (offset + n, len - n);
        }
        Some(offset)
    }

    fn release(&mut self, offset: u32, n: u32) {
        let at = self.free.partition_point(|&(o, _)| o < offset);
        self.free.insert(at, (offset, n));
        if at + 1 < self.free.len() {
            let (o, l) = self.free[at];
            let (next, next_len) = self.free[at + 1];
            if o + l == next {
                self.free[at].1 += next_len;
                self.free.remove(at + 1);
            }
        }
        if at > 0 {
            let (prev, prev_len) = self.free[at - 1];
            let (o, l) = self.free[at];
            if prev + prev_len == o {
                self.free[at - 1].1 += l;
                self.free.remove(at);
            }
        }
    }
}

/// What an arena's elements are.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Vertices, read as vertex input; the arena holds the shared quad
    Vertices,
    /// Terrain quads, read by the shader as a storage buffer.
    Quads,
}

struct Block {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    capacity: u32,
    free: FreeList,
}

/// Where one mesh sits.
#[derive(Clone, Copy)]
pub struct Slot {
    block: usize,
    /// First element in the block.
    offset: u32,
    /// Elements: vertices or quads.
    pub count: u32,
}

/// One queued copy: `bytes` of [`MeshArena::pending_bytes`] into a buffer.
struct PendingCopy {
    dst: vk::Buffer,
    src_offset: u64,
    dst_offset: u64,
    size: u64,
}

pub struct MeshArena {
    /// Bytes per element.
    element_size: u64,
    /// Elements per block.
    block_elements: u32,
    blocks: Vec<Block>,
    /// By `MeshId`; `None` for a free id.
    slots: Vec<Option<Slot>>,
    free_ids: Vec<usize>,
    /// Freed slots with the frame they were freed in: a frame still in
    retired: Vec<(u64, Slot)>,
    pending: Vec<PendingCopy>,
    pending_bytes: Vec<u8>,
    staging: Vec<Staging>,
    /// [`QUAD_INDICES`] for [`QUADS_PER_DRAW`] quads, for vertex meshes.
    index: Option<(vk::Buffer, vk::DeviceMemory)>,
    /// How blocks are bound.
    usage: vk::BufferUsageFlags,
    /// The queued copies are in the frame being recorded.
    recorded: bool,
}

impl MeshArena {
    /// An arena for `T`s of `kind`.
    pub unsafe fn new<T: Copy>(
        device: &ash::Device,
        mem_props: &vk::PhysicalDeviceMemoryProperties,
        frames_in_flight: usize,
        kind: Kind,
    ) -> anyhow::Result<Self> {
        let element_size = std::mem::size_of::<T>() as u64;
        let mut arena = Self {
            element_size,
            block_elements: (BLOCK_BYTES / element_size) as u32,
            blocks: Vec::new(),
            slots: Vec::new(),
            free_ids: Vec::new(),
            retired: Vec::new(),
            pending: Vec::new(),
            pending_bytes: Vec::new(),
            staging: (0..frames_in_flight).map(|_| Staging::default()).collect(),
            index: None,
            usage: match kind {
                Kind::Vertices => vk::BufferUsageFlags::VERTEX_BUFFER,
                Kind::Quads => vk::BufferUsageFlags::STORAGE_BUFFER,
            },
            recorded: false,
        };
        if kind == Kind::Vertices {
            let indices: Vec<u16> = (0..QUADS_PER_DRAW)
                .flat_map(|q| QUAD_INDICES.map(|i| (q * 4) as u16 + i))
                .collect();
            let size = std::mem::size_of_val(indices.as_slice()) as u64;
            let (buffer, memory) = create_buffer(
                device,
                mem_props,
                size,
                vk::BufferUsageFlags::INDEX_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
            )?;
            arena.index = Some((buffer, memory));
            arena.queue_copy(buffer, 0, as_bytes(&indices));
        }
        Ok(arena)
    }

    /// The shared quad index buffer; null for an arena without one.
    pub fn index_buffer(&self) -> vk::Buffer {
        self.index.map_or(vk::Buffer::null(), |(buffer, _)| buffer)
    }

    pub fn slot(&self, id: usize) -> Option<Slot> {
        self.slots.get(id).copied().flatten()
    }

    /// Every block's buffer, oldest first.
    pub fn buffers(&self) -> impl Iterator<Item = vk::Buffer> + '_ {
        self.blocks.iter().map(|block| block.buffer)
    }

    /// The buffer a slot is in and its first element there.
    pub fn location(&self, slot: Slot) -> (vk::Buffer, u32) {
        (self.blocks[slot.block].buffer, slot.offset)
    }

    /// Total and used bytes of the blocks, for F3.
    pub fn usage(&self) -> (u64, u64) {
        let total: u64 = self.blocks.iter().map(|b| u64::from(b.capacity)).sum();
        let free: u64 = self
            .blocks
            .iter()
            .flat_map(|b| b.free.free.iter())
            .map(|&(_, len)| u64::from(len))
            .sum();
        (
            total * self.element_size,
            (total - free) * self.element_size,
        )
    }

    /// Queues `bytes` to be copied into `dst` at `dst_offset` at the start
    pub fn queue_copy(&mut self, dst: vk::Buffer, dst_offset: u64, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.pending.push(PendingCopy {
            dst,
            src_offset: self.pending_bytes.len() as u64,
            dst_offset,
            size: bytes.len() as u64,
        });
        self.pending_bytes.extend_from_slice(bytes);
    }

    /// Places `elements` in a block and queues their upload; returns the
    pub unsafe fn upload<T: Copy>(
        &mut self,
        device: &ash::Device,
        mem_props: &vk::PhysicalDeviceMemoryProperties,
        elements: &[T],
    ) -> anyhow::Result<usize> {
        debug_assert_eq!(std::mem::size_of::<T>() as u64, self.element_size);
        debug_assert!(
            self.index.is_none() || elements.len().is_multiple_of(4),
            "vertex meshes are lists of quads"
        );
        let count = u32::try_from(elements.len())?;
        let slot = if count == 0 {
            Slot {
                block: 0,
                offset: 0,
                count: 0,
            }
        } else {
            let found = self
                .blocks
                .iter_mut()
                .enumerate()
                .find_map(|(i, block)| block.free.alloc(count).map(|offset| (i, offset)));
            let (block, offset) = match found {
                Some(found) => found,
                None => {
                    let capacity = count.max(self.block_elements);
                    let (buffer, memory) = create_buffer_in(
                        device,
                        mem_props,
                        u64::from(capacity) * self.element_size,
                        self.usage | vk::BufferUsageFlags::TRANSFER_DST,
                        &DEVICE_OR_HOST,
                    )?;
                    let mut free = FreeList::new(capacity);
                    let offset = free.alloc(count).expect("a new block fits its mesh");
                    self.blocks.push(Block {
                        buffer,
                        memory,
                        capacity,
                        free,
                    });
                    (self.blocks.len() - 1, offset)
                }
            };
            let dst = self.blocks[block].buffer;
            self.queue_copy(
                dst,
                u64::from(offset) * self.element_size,
                as_bytes(elements),
            );
            Slot {
                block,
                offset,
                count,
            }
        };
        let id = match self.free_ids.pop() {
            Some(id) => {
                self.slots[id] = Some(slot);
                id
            }
            None => {
                self.slots.push(Some(slot));
                self.slots.len() - 1
            }
        };
        Ok(id)
    }

    /// Frees a mesh; its space is reused once no frame in flight can draw
    pub fn free(&mut self, id: usize, frame_number: u64) {
        if let Some(slot) = self.slots.get_mut(id).and_then(Option::take) {
            self.free_ids.push(id);
            if slot.count > 0 {
                self.retired.push((frame_number, slot));
            }
        }
    }

    /// Room in this frame's staging buffer for the queued copies: done
    /// before the frame acquires an image, so running out of memory skips
    /// the frame instead of breaking it halfway.
    pub unsafe fn prepare(
        &mut self,
        device: &ash::Device,
        mem_props: &vk::PhysicalDeviceMemoryProperties,
        frame: usize,
    ) -> anyhow::Result<()> {
        let needed = self.pending_bytes.len() as u64;
        self.staging[frame].trim(device, needed, STAGING_KEPT);
        if self.pending.is_empty() {
            return Ok(());
        }
        self.staging[frame].ensure(device, mem_props, needed, 1 << 20)
    }

    /// The frame with the copies [`Self::record`] recorded was submitted:
    /// they are done. Until then they stay queued, so a frame that fails
    /// after recording them doesn't leave meshes drawn that were never
    /// filled (whose garbage quads index past the quad table).
    pub fn submitted(&mut self) {
        if !std::mem::take(&mut self.recorded) {
            return;
        }
        self.pending.clear();
        self.pending_bytes.clear();
        if self.pending_bytes.capacity() > 64 << 20 {
            self.pending_bytes = Vec::new();
        }
    }

    /// Releases retired slots no frame in flight can still draw, then
    pub unsafe fn record(
        &mut self,
        device: &ash::Device,
        mem_props: &vk::PhysicalDeviceMemoryProperties,
        cmd: vk::CommandBuffer,
        frame: usize,
        frame_number: u64,
        frames_in_flight: u64,
    ) -> anyhow::Result<()> {
        let blocks = &mut self.blocks;
        self.retired.retain(|&(freed_at, slot)| {
            let done = frame_number >= freed_at + frames_in_flight;
            if done {
                blocks[slot.block].free.release(slot.offset, slot.count);
            }
            !done
        });
        if self.pending.is_empty() {
            return Ok(());
        }
        let staging = &mut self.staging[frame];
        staging.ensure(device, mem_props, self.pending_bytes.len() as u64, 1 << 20)?;
        let Some(mapped) = staging.mapped else {
            return Ok(());
        };
        std::ptr::copy_nonoverlapping(
            self.pending_bytes.as_ptr(),
            mapped.as_ptr(),
            self.pending_bytes.len(),
        );
        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::VERTEX_INPUT | vk::PipelineStageFlags::VERTEX_SHADER,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[],
        );
        for copy in &self.pending {
            device.cmd_copy_buffer(
                cmd,
                staging.buffer,
                copy.dst,
                &[vk::BufferCopy {
                    src_offset: copy.src_offset,
                    dst_offset: copy.dst_offset,
                    size: copy.size,
                }],
            );
        }
        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::VERTEX_INPUT | vk::PipelineStageFlags::VERTEX_SHADER,
            vk::DependencyFlags::empty(),
            &[vk::MemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(
                    vk::AccessFlags::VERTEX_ATTRIBUTE_READ
                        | vk::AccessFlags::INDEX_READ
                        | vk::AccessFlags::SHADER_READ,
                )],
            &[],
            &[],
        );
        self.recorded = true;
        Ok(())
    }

    pub unsafe fn destroy(&mut self, device: &ash::Device) {
        for block in self.blocks.drain(..) {
            device.destroy_buffer(block.buffer, None);
            device.free_memory(block.memory, None);
        }
        for staging in &mut self.staging {
            staging.destroy(device);
        }
        if let Some((buffer, memory)) = self.index.take() {
            device.destroy_buffer(buffer, None);
            device.free_memory(memory, None);
        }
    }
}

pub(super) fn as_bytes<T: Copy>(data: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(data.as_ptr().cast(), std::mem::size_of_val(data)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_list_reuses_and_coalesces() {
        let mut list = FreeList::new(100);
        let a = list.alloc(10).unwrap();
        let b = list.alloc(20).unwrap();
        let c = list.alloc(30).unwrap();
        assert_eq!((a, b, c), (0, 10, 30));
        list.release(b, 20);
        assert_eq!(list.alloc(15), Some(10));
        list.release(10, 15);
        list.release(a, 10);
        list.release(c, 30);
        assert_eq!(list.free, vec![(0, 100)]);
        assert_eq!(list.alloc(101), None);
    }
}
