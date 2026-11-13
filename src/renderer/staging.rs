//! A host-visible staging buffer per frame in flight, mapped for its

use ash::vk;

use super::create_buffer;

#[derive(Default)]
pub struct Staging {
    pub buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    pub mapped: Option<std::ptr::NonNull<u8>>,
    capacity: u64,
}

impl Staging {
    /// Room for `size` bytes, at least `min_capacity` when it has to grow.
    pub unsafe fn ensure(
        &mut self,
        device: &ash::Device,
        mem_props: &vk::PhysicalDeviceMemoryProperties,
        size: u64,
        min_capacity: u64,
    ) -> anyhow::Result<()> {
        if size <= self.capacity {
            return Ok(());
        }
        self.destroy(device);
        let capacity = size.next_power_of_two().max(min_capacity);
        let (buffer, memory) = create_buffer(
            device,
            mem_props,
            capacity,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        let mapped =
            device.map_memory(memory, 0, capacity, vk::MemoryMapFlags::empty())? as *mut u8;
        *self = Self {
            buffer,
            memory,
            mapped: std::ptr::NonNull::new(mapped),
            capacity,
        };
        Ok(())
    }

    pub unsafe fn destroy(&mut self, device: &ash::Device) {
        if self.buffer != vk::Buffer::null() {
            device.unmap_memory(self.memory);
            device.destroy_buffer(self.buffer, None);
            device.free_memory(self.memory, None);
        }
        *self = Self::default();
    }
}
