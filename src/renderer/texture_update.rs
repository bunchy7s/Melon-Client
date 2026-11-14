//! Rewrites of parts of an existing texture, for animated atlas sprites.

use ash::vk;
use image::RgbaImage;

use super::staging::Staging;

/// One queued region.
pub struct PendingUpdate {
    pub image: vk::Image,
    /// The mip level, with `x`/`y` in its pixels.
    pub level: u32,
    pub x: u32,
    pub y: u32,
    pub pixels: RgbaImage,
}

/// Room in `staging` for `updates`, before the frame acquires an image.
pub unsafe fn prepare(
    staging: &mut Staging,
    device: &ash::Device,
    mem_props: &vk::PhysicalDeviceMemoryProperties,
    updates: &[PendingUpdate],
) -> anyhow::Result<()> {
    let total: u64 = updates.iter().map(|u| u.pixels.as_raw().len() as u64).sum();
    staging.trim(device, total, 1 << 20);
    if updates.is_empty() {
        return Ok(());
    }
    staging.ensure(device, mem_props, total, 64 * 1024)
}

/// Copies `updates` into `staging` and records their uploads into `cmd`,
pub unsafe fn record(
    staging: &mut Staging,
    device: &ash::Device,
    mem_props: &vk::PhysicalDeviceMemoryProperties,
    cmd: vk::CommandBuffer,
    updates: &[PendingUpdate],
) -> anyhow::Result<()> {
    if updates.is_empty() {
        return Ok(());
    }
    let total: u64 = updates.iter().map(|u| u.pixels.as_raw().len() as u64).sum();
    staging.ensure(device, mem_props, total, 64 * 1024)?;
    let Some(mapped) = staging.mapped else {
        return Ok(());
    };
    let mut images: Vec<vk::Image> = Vec::new();
    for update in updates {
        if !images.contains(&update.image) {
            images.push(update.image);
        }
    }
    let barriers = |old, new, src, dst| -> Vec<vk::ImageMemoryBarrier> {
        images
            .iter()
            .map(|&image| {
                vk::ImageMemoryBarrier::default()
                    .old_layout(old)
                    .new_layout(new)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(image)
                    .subresource_range(
                        vk::ImageSubresourceRange::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .level_count(vk::REMAINING_MIP_LEVELS)
                            .layer_count(1),
                    )
                    .src_access_mask(src)
                    .dst_access_mask(dst)
            })
            .collect()
    };
    device.cmd_pipeline_barrier(
        cmd,
        vk::PipelineStageFlags::FRAGMENT_SHADER,
        vk::PipelineStageFlags::TRANSFER,
        vk::DependencyFlags::empty(),
        &[],
        &[],
        &barriers(
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            vk::AccessFlags::SHADER_READ,
            vk::AccessFlags::TRANSFER_WRITE,
        ),
    );
    let mut offset = 0u64;
    let mut regions: Vec<Vec<vk::BufferImageCopy>> = vec![Vec::new(); images.len()];
    for update in updates {
        let bytes = update.pixels.as_raw();
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            mapped.as_ptr().add(offset as usize),
            bytes.len(),
        );
        let (width, height) = update.pixels.dimensions();
        let image = images
            .iter()
            .position(|&i| i == update.image)
            .expect("collected above");
        regions[image].push(
            vk::BufferImageCopy::default()
                .buffer_offset(offset)
                .image_subresource(vk::ImageSubresourceLayers {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: update.level,
                    base_array_layer: 0,
                    layer_count: 1,
                })
                .image_offset(vk::Offset3D {
                    x: update.x as i32,
                    y: update.y as i32,
                    z: 0,
                })
                .image_extent(vk::Extent3D {
                    width,
                    height,
                    depth: 1,
                }),
        );
        offset += bytes.len() as u64;
    }
    for (&image, regions) in images.iter().zip(&regions) {
        device.cmd_copy_buffer_to_image(
            cmd,
            staging.buffer,
            image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            regions,
        );
    }
    device.cmd_pipeline_barrier(
        cmd,
        vk::PipelineStageFlags::TRANSFER,
        vk::PipelineStageFlags::FRAGMENT_SHADER,
        vk::DependencyFlags::empty(),
        &[],
        &[],
        &barriers(
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            vk::AccessFlags::TRANSFER_WRITE,
            vk::AccessFlags::SHADER_READ,
        ),
    );
    Ok(())
}
