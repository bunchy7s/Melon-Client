//! GPU textures: 2D (UI sprites, font pages) and cubemaps (panorama).

use ash::vk;
use image::RgbaImage;

use super::{create_buffer, find_memory_type};

#[derive(Clone, Copy)]
pub struct Texture {
    pub image: vk::Image,
    pub memory: vk::DeviceMemory,
    pub view: vk::ImageView,
    pub descriptor_set: vk::DescriptorSet,
    /// 1 plus the smaller levels below the full-size image.
    pub mip_levels: u32,
    /// The handles belong to another slot (a freed slot showing the white
    pub shared: bool,
}

/// Vulkan handles every texture upload needs, bundled so callers do not
#[derive(Clone, Copy)]
pub struct Uploader<'a> {
    pub device: &'a ash::Device,
    pub mem_props: &'a vk::PhysicalDeviceMemoryProperties,
    pub command_pool: vk::CommandPool,
    pub queue: vk::Queue,
    pub descriptor_pool: vk::DescriptorPool,
    pub descriptor_set_layout: vk::DescriptorSetLayout,
    pub sampler: vk::Sampler,
}

impl Uploader<'_> {
    pub unsafe fn image_2d(&self, rgba: &RgbaImage) -> anyhow::Result<Texture> {
        self.upload(std::slice::from_ref(rgba), &[], vk::ImageViewType::TYPE_2D)
    }

    /// A 2D texture with its smaller levels, each half the size of the one
    pub unsafe fn image_2d_mipmapped(
        &self,
        rgba: &RgbaImage,
        mips: &[RgbaImage],
    ) -> anyhow::Result<Texture> {
        self.upload(std::slice::from_ref(rgba), mips, vk::ImageViewType::TYPE_2D)
    }

    /// Six equal-size faces in Vulkan cubemap order (+X, -X, +Y, -Y, +Z, -Z).
    pub unsafe fn cubemap(&self, faces: &[RgbaImage; 6]) -> anyhow::Result<Texture> {
        self.upload(faces, &[], vk::ImageViewType::CUBE)
    }

    /// Device-local `R8G8B8A8_UNORM` via a host-visible staging buffer, waited out.
    unsafe fn upload(
        &self,
        layers: &[RgbaImage],
        mips: &[RgbaImage],
        view_type: vk::ImageViewType,
    ) -> anyhow::Result<Texture> {
        let device = self.device;
        let first = layers
            .first()
            .ok_or_else(|| anyhow::anyhow!("Textur-Upload ohne Bilddaten"))?;
        let (width, height) = (first.width(), first.height());
        if layers
            .iter()
            .any(|l| l.width() != width || l.height() != height)
        {
            anyhow::bail!("Textur-Layer haben unterschiedliche Groessen");
        }
        if !mips.is_empty() && layers.len() != 1 {
            anyhow::bail!("Mipmaps nur für einzelne Texturen");
        }
        for (level, mip) in mips.iter().enumerate() {
            let shift = level + 1;
            if mip.dimensions() != ((width >> shift).max(1), (height >> shift).max(1)) {
                anyhow::bail!("Mipmap-Stufe {shift} hat die falsche Größe");
            }
        }
        let parts: Vec<(u32, u32, &RgbaImage)> = layers
            .iter()
            .enumerate()
            .map(|(layer, image)| (layer as u32, 0, image))
            .chain(
                mips.iter()
                    .enumerate()
                    .map(|(level, image)| (0, level as u32 + 1, image)),
            )
            .collect();
        let byte_size: vk::DeviceSize = parts
            .iter()
            .map(|(_, _, image)| image.as_raw().len() as vk::DeviceSize)
            .sum();

        // Everything made on the way is freed again when a later step
        // fails (the descriptor pool full, the VRAM taken by another
        // program): an upload that fails must not leave its image behind.
        let mut made = Made::default();
        let result = self.upload_parts(&parts, byte_size, (width, height), view_type, &mut made);
        if made.cmd != vk::CommandBuffer::null() {
            device.free_command_buffers(self.command_pool, &[made.cmd]);
        }
        if made.staging_buffer != vk::Buffer::null() {
            device.destroy_buffer(made.staging_buffer, None);
        }
        if made.staging_memory != vk::DeviceMemory::null() {
            device.free_memory(made.staging_memory, None);
        }
        if result.is_err() {
            if made.view != vk::ImageView::null() {
                device.destroy_image_view(made.view, None);
            }
            if made.image != vk::Image::null() {
                device.destroy_image(made.image, None);
            }
            if made.memory != vk::DeviceMemory::null() {
                device.free_memory(made.memory, None);
            }
        }
        result
    }

    /// [`Self::upload`]'s work; what it creates goes into `made` as soon as
    /// it exists, for the caller to free.
    unsafe fn upload_parts(
        &self,
        parts: &[(u32, u32, &RgbaImage)],
        byte_size: vk::DeviceSize,
        (width, height): (u32, u32),
        view_type: vk::ImageViewType,
        made: &mut Made,
    ) -> anyhow::Result<Texture> {
        let device = self.device;
        let layer_count = parts.iter().filter(|&&(_, level, _)| level == 0).count() as u32;
        let mip_levels = 1 + parts.iter().map(|&(_, level, _)| level).max().unwrap_or(0);
        let (staging_buffer, staging_memory) = create_buffer(
            device,
            self.mem_props,
            byte_size,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        (made.staging_buffer, made.staging_memory) = (staging_buffer, staging_memory);
        let mapped = device.map_memory(staging_memory, 0, byte_size, vk::MemoryMapFlags::empty())?
            as *mut u8;
        let mut offsets = Vec::with_capacity(parts.len());
        let mut offset = 0usize;
        for (_, _, image) in parts {
            let bytes = image.as_raw();
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), mapped.add(offset), bytes.len());
            offsets.push(offset as vk::DeviceSize);
            offset += bytes.len();
        }
        device.unmap_memory(staging_memory);

        let mut image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk::Format::R8G8B8A8_UNORM)
            .extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            })
            .mip_levels(mip_levels)
            .array_layers(layer_count)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        if view_type == vk::ImageViewType::CUBE {
            image_info = image_info.flags(vk::ImageCreateFlags::CUBE_COMPATIBLE);
        }
        let image = device.create_image(&image_info, None)?;
        made.image = image;
        let image_req = device.get_image_memory_requirements(image);
        let memory = device.allocate_memory(
            &vk::MemoryAllocateInfo::default()
                .allocation_size(image_req.size)
                .memory_type_index(find_memory_type(
                    self.mem_props,
                    image_req.memory_type_bits,
                    vk::MemoryPropertyFlags::DEVICE_LOCAL,
                )?),
            None,
        )?;
        made.memory = memory;
        device.bind_image_memory(image, memory, 0)?;

        let cmd = device.allocate_command_buffers(
            &vk::CommandBufferAllocateInfo::default()
                .command_pool(self.command_pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(1),
        )?[0];
        made.cmd = cmd;
        device.begin_command_buffer(
            cmd,
            &vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
        )?;

        let subresource = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .base_mip_level(0)
            .level_count(mip_levels)
            .base_array_layer(0)
            .layer_count(layer_count);

        let to_transfer_dst = vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(image)
            .subresource_range(subresource)
            .src_access_mask(vk::AccessFlags::empty())
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE);
        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[to_transfer_dst],
        );

        let regions: Vec<vk::BufferImageCopy> = parts
            .iter()
            .zip(&offsets)
            .map(|(&(layer, level, image), &offset)| {
                vk::BufferImageCopy::default()
                    .buffer_offset(offset)
                    .buffer_row_length(0)
                    .buffer_image_height(0)
                    .image_subresource(vk::ImageSubresourceLayers {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        mip_level: level,
                        base_array_layer: layer,
                        layer_count: 1,
                    })
                    .image_offset(vk::Offset3D::default())
                    .image_extent(vk::Extent3D {
                        width: image.width(),
                        height: image.height(),
                        depth: 1,
                    })
            })
            .collect();
        device.cmd_copy_buffer_to_image(
            cmd,
            staging_buffer,
            image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &regions,
        );

        let to_shader_read = vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(image)
            .subresource_range(subresource)
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ);
        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[to_shader_read],
        );

        device.end_command_buffer(cmd)?;
        let cmds = [cmd];
        let submit = vk::SubmitInfo::default().command_buffers(&cmds);
        device.queue_submit(self.queue, &[submit], vk::Fence::null())?;
        device.queue_wait_idle(self.queue)?;

        let view = device.create_image_view(
            &vk::ImageViewCreateInfo::default()
                .image(image)
                .view_type(view_type)
                .format(vk::Format::R8G8B8A8_UNORM)
                .subresource_range(subresource),
            None,
        )?;
        made.view = view;

        let set_layouts = [self.descriptor_set_layout];
        let descriptor_set = device.allocate_descriptor_sets(
            &vk::DescriptorSetAllocateInfo::default()
                .descriptor_pool(self.descriptor_pool)
                .set_layouts(&set_layouts),
        )?[0];

        let descriptor_image = [vk::DescriptorImageInfo::default()
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .image_view(view)
            .sampler(self.sampler)];
        let write = vk::WriteDescriptorSet::default()
            .dst_set(descriptor_set)
            .dst_binding(0)
            .dst_array_element(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(&descriptor_image);
        device.update_descriptor_sets(&[write], &[]);

        Ok(Texture {
            image,
            memory,
            view,
            descriptor_set,
            mip_levels,
            shared: false,
        })
    }
}

/// What an upload has created so far (null: not yet).
#[derive(Default)]
struct Made {
    staging_buffer: vk::Buffer,
    staging_memory: vk::DeviceMemory,
    cmd: vk::CommandBuffer,
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
}
