//! Headless Vulkan check: Khronos validation layers, every shader module,

use super::{
    globals, indirect, pipeline, quad_table,
    vk_util::{PassStart, create_render_pass, find_depth_format},
};
use ash::vk;
use std::ffi::{CStr, CString};
use std::path::PathBuf;
use std::sync::Mutex;

static MESSAGES: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn local_layer_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".vulkan/validation")
}

fn ensure_layer_search_path() {
    if std::env::var_os("VK_LAYER_PATH").is_some() {
        return;
    }
    let dir = local_layer_dir();
    if dir.join("VkLayer_khronos_validation.json").is_file() {
        unsafe {
            std::env::set_var("VK_LAYER_PATH", dir);
        }
    }
}

unsafe extern "system" fn collect_messages(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _types: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _user_data: *mut std::ffi::c_void,
) -> vk::Bool32 {
    let wanted = vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
        | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR;
    if !severity.intersects(wanted) {
        return vk::FALSE;
    }
    let message = CStr::from_ptr((*data).p_message).to_string_lossy();
    MESSAGES
        .lock()
        .expect("validation log")
        .push(message.into_owned());
    vk::FALSE
}

struct Gpu {
    _entry: ash::Entry,
    instance: ash::Instance,
    debug: ash::ext::debug_utils::Instance,
    messenger: vk::DebugUtilsMessengerEXT,
    physical: vk::PhysicalDevice,
    device: ash::Device,
}

impl Gpu {
    unsafe fn new() -> anyhow::Result<Self> {
        ensure_layer_search_path();
        let entry = ash::Entry::load()?;
        let layer = c"VK_LAYER_KHRONOS_validation";
        let found = entry
            .enumerate_instance_layer_properties()?
            .iter()
            .any(|l| CStr::from_ptr(l.layer_name.as_ptr()) == layer);
        anyhow::ensure!(
            found,
            "VK_LAYER_KHRONOS_validation missing. Install vulkan-validation-layers, \
             or put the Khronos layer in .vulkan/validation (VkLayer_khronos_validation.json), \
             or set VK_LAYER_PATH. See https://github.com/KhronosGroup/Vulkan-ValidationLayers"
        );

        let app_name = c"Melon";
        let app_info = vk::ApplicationInfo::default()
            .application_name(app_name)
            .application_version(0)
            .engine_name(app_name)
            .engine_version(0)
            .api_version(vk::API_VERSION_1_1);
        let layers = [layer.as_ptr()];
        let extensions = [
            ash::ext::debug_utils::NAME.as_ptr(),
            ash::khr::surface::NAME.as_ptr(),
        ];
        let mut messenger_info = vk::DebugUtilsMessengerCreateInfoEXT::default()
            .message_severity(
                vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                    | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR,
            )
            .message_type(
                vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                    | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                    | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
            )
            .pfn_user_callback(Some(collect_messages));
        let instance_info = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_layer_names(&layers)
            .enabled_extension_names(&extensions)
            .push_next(&mut messenger_info);
        let instance = entry.create_instance(&instance_info, None)?;
        let debug = ash::ext::debug_utils::Instance::new(&entry, &instance);
        let messenger = debug.create_debug_utils_messenger(&messenger_info, None)?;

        let physical = instance
            .enumerate_physical_devices()?
            .into_iter()
            .find(|&pd| {
                instance
                    .get_physical_device_queue_family_properties(pd)
                    .iter()
                    .any(|q| q.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            })
            .ok_or_else(|| anyhow::anyhow!("keine Grafik-GPU gefunden"))?;
        let family = instance
            .get_physical_device_queue_family_properties(physical)
            .iter()
            .position(|q| q.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .expect("graphics queue") as u32;

        let priorities = [1.0f32];
        let queue_info = vk::DeviceQueueCreateInfo::default()
            .queue_family_index(family)
            .queue_priorities(&priorities);
        let queues = [queue_info];
        let supported = instance.get_physical_device_features(physical);
        let mut supported_11 = vk::PhysicalDeviceVulkan11Features::default();
        let mut supported_2 = vk::PhysicalDeviceFeatures2::default().push_next(&mut supported_11);
        instance.get_physical_device_features2(physical, &mut supported_2);
        let multi_draw = supported.multi_draw_indirect == vk::TRUE
            && supported.draw_indirect_first_instance == vk::TRUE
            && supported_11.shader_draw_parameters == vk::TRUE;
        let features = vk::PhysicalDeviceFeatures::default()
            .multi_draw_indirect(multi_draw)
            .draw_indirect_first_instance(multi_draw);
        let mut features_11 =
            vk::PhysicalDeviceVulkan11Features::default().shader_draw_parameters(multi_draw);
        let device_extensions = [ash::khr::swapchain::NAME.as_ptr()];
        let device_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queues)
            .enabled_features(&features)
            .enabled_extension_names(&device_extensions)
            .push_next(&mut features_11);
        let device = instance.create_device(physical, &device_info, None)?;

        Ok(Self {
            _entry: entry,
            instance,
            debug,
            messenger,
            physical,
            device,
        })
    }
}

impl Drop for Gpu {
    fn drop(&mut self) {
        unsafe {
            self.device.device_wait_idle().ok();
            self.device.destroy_device(None);
            self.debug
                .destroy_debug_utils_messenger(self.messenger, None);
            self.instance.destroy_instance(None);
        }
    }
}

unsafe fn color_only_pass(
    device: &ash::Device,
    format: vk::Format,
) -> anyhow::Result<vk::RenderPass> {
    let attachments = [vk::AttachmentDescription::default()
        .format(format)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::DONT_CARE)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
    let color = [vk::AttachmentReference {
        attachment: 0,
        layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
    }];
    let subpasses = [vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&color)];
    Ok(device.create_render_pass(
        &vk::RenderPassCreateInfo::default()
            .attachments(&attachments)
            .subpasses(&subpasses),
        None,
    )?)
}

#[test]
fn validation_layers_accept_every_pipeline() {
    MESSAGES.lock().expect("validation log").clear();
    unsafe {
        let gpu = Gpu::new().expect("Vulkan instance with validation");
        let depth = find_depth_format(&gpu.instance, gpu.physical).expect("depth format");
        let color = vk::Format::B8G8R8A8_UNORM;
        let render_pass = create_render_pass(&gpu.device, color, depth, PassStart::Clear)
            .expect("scene render pass");
        let blur_pass = color_only_pass(&gpu.device, color).expect("blur render pass");
        let tex_layout =
            pipeline::create_descriptor_set_layout(&gpu.device).expect("texture set layout");
        let layouts = pipeline::world::Layouts {
            table: quad_table::create_layout(&gpu.device).expect("quad table layout"),
            draws: indirect::create_layout(&gpu.device).expect("draw offset layout"),
            globals: globals::create_layout(&gpu.device).expect("globals layout"),
        };

        let mut passes = Vec::new();
        let mut keep = |pass: pipeline::Pass| passes.push(pass);

        keep(pipeline::sky::create(&gpu.device, render_pass, tex_layout).expect("sky"));
        let gui_layout = super::gui_uniforms::create_layout(&gpu.device).expect("gui layout");
        for mode in pipeline::ui::BLENDS {
            keep(
                pipeline::ui::create(
                    &gpu.device,
                    render_pass,
                    tex_layout,
                    gui_layout,
                    mode,
                    pipeline::ui::BUILT_IN,
                )
                .expect("ui"),
            );
        }
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        if assets.join("minecraft/shaders/core/text.vsh").is_file() {
            let shaders = crate::gui::pack_shaders::compile_all(&assets).expect("GUI shaders");
            for shader in &shaders {
                let bytes = |words: &[u32]| -> Vec<u8> {
                    words.iter().flat_map(|w| w.to_le_bytes()).collect()
                };
                let (vert, frag) = (bytes(&shader.vert), bytes(&shader.frag));
                let shaders = pipeline::ui::Shaders {
                    vert: &vert,
                    frag: &frag,
                    inputs: Some(&shader.inputs),
                };
                let built = pipeline::ui::create_all(
                    &gpu.device,
                    render_pass,
                    tex_layout,
                    gui_layout,
                    shaders,
                )
                .expect("pack GUI pipeline");
                built.into_iter().for_each(&mut keep);
            }
        }
        keep(pipeline::blur::create(&gpu.device, blur_pass, tex_layout).expect("blur x"));
        keep(pipeline::blur::create(&gpu.device, render_pass, tex_layout).expect("blur y"));

        use pipeline::world::{Geometry, Mode};
        let world = |mode, geometry| {
            pipeline::world::create(
                &gpu.device,
                render_pass,
                tex_layout,
                &layouts,
                mode,
                geometry,
            )
        };
        for mode in [
            Mode::Solid,
            Mode::Opaque,
            Mode::Translucent,
            Mode::TranslucentTerrain,
            Mode::Clouds,
            Mode::Weather,
            Mode::Celestial,
            Mode::SkyDisc,
            Mode::SkyGlow,
            Mode::Crumbling,
            Mode::WaterMask,
        ] {
            keep(world(mode, Geometry::Vertices).expect("world vertices"));
        }
        keep(
            pipeline::world::create(
                &gpu.device,
                blur_pass,
                tex_layout,
                &layouts,
                Mode::Outline,
                Geometry::Vertices,
            )
            .expect("outline"),
        );
        for mode in [Mode::Solid, Mode::Opaque] {
            keep(world(mode, Geometry::Quads { indirect: false }).expect("terrain quads"));
            keep(world(mode, Geometry::Quads { indirect: true }).expect("terrain indirect"));
        }

        gpu.device.device_wait_idle().expect("idle after pipelines");
        for pass in &passes {
            pass.destroy(&gpu.device);
        }
        gpu.device.destroy_descriptor_set_layout(tex_layout, None);
        gpu.device.destroy_descriptor_set_layout(gui_layout, None);
        gpu.device
            .destroy_descriptor_set_layout(layouts.table, None);
        gpu.device
            .destroy_descriptor_set_layout(layouts.draws, None);
        gpu.device
            .destroy_descriptor_set_layout(layouts.globals, None);
        gpu.device.destroy_render_pass(render_pass, None);
        gpu.device.destroy_render_pass(blur_pass, None);
    }

    let messages = MESSAGES.lock().expect("validation log");
    assert!(
        messages.is_empty(),
        "Vulkan validation:\n{}",
        messages.join("\n")
    );
}

/// The post chains of the packs (vanilla's, and `MELON_PACK`'s on top)
#[test]
fn post_chains_pass_validation() {
    use super::post::gpu::Post;
    use super::post::load;
    use super::texture::Texture;
    use crate::assets::packs::{self, Pack, ResourceStack};
    use std::path::Path;

    let Some(root) = std::env::var_os("MELON_ASSETS").map(PathBuf::from) else {
        eprintln!("MELON_ASSETS fehlt, Test uebersprungen");
        return;
    };
    MESSAGES.lock().expect("validation log").clear();
    if let Some(pack) = std::env::var_os("MELON_PACK") {
        let pack = Pack::open("test".into(), "test".into(), Path::new(&pack)).expect("pack");
        packs::activate(ResourceStack {
            packs: vec![std::sync::Arc::new(pack)],
        });
    }
    unsafe {
        let gpu = Gpu::new().expect("Vulkan instance with validation");
        let mem_props = gpu
            .instance
            .get_physical_device_memory_properties(gpu.physical);
        let depth = find_depth_format(&gpu.instance, gpu.physical).expect("depth format");
        let color = vk::Format::B8G8R8A8_UNORM;
        let scene = create_render_pass(&gpu.device, color, depth, PassStart::Clear).expect("clear");
        let split =
            create_render_pass(&gpu.device, color, depth, PassStart::SceneSplit).expect("split");
        let resume =
            create_render_pass(&gpu.device, color, depth, PassStart::Resume).expect("resume");
        let gui_layout = super::gui_uniforms::create_layout(&gpu.device).expect("gui layout");
        let mut post = Post::new(&gpu.device, color).expect("post");

        let (chains, blit) = load::load(&root);
        assert!(!chains.is_empty(), "no chains in {}", root.display());
        let sizes = |chain: &super::post::ChainSource| -> Vec<(u32, u32)> {
            chain
                .config
                .passes
                .iter()
                .flat_map(|p| p.inputs.iter())
                .filter_map(|i| match i {
                    super::post::config::Input::Texture { width, height, .. } => {
                        Some((*width, *height))
                    }
                    _ => None,
                })
                .collect()
        };
        let extent = vk::Extent2D {
            width: 640,
            height: 360,
        };
        let mut prepared = Vec::new();
        for chain in chains {
            let textures = sizes(&chain)
                .into_iter()
                .map(|(width, height)| {
                    let image = super::blur::create_image(
                        &gpu.device,
                        &mem_props,
                        vk::Format::R8G8B8A8_UNORM,
                        vk::Extent2D {
                            width: 8,
                            height: 8,
                        },
                        vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
                    )
                    .expect("effect texture");
                    let texture = Texture {
                        image: image.image,
                        memory: image.memory,
                        view: image.view,
                        descriptor_set: vk::DescriptorSet::null(),
                        mip_levels: 1,
                        shared: false,
                    };
                    (texture, width, height)
                })
                .collect();
            prepared.push((chain, textures));
        }
        let device = &gpu.device;
        let free = |t: Texture| {
            device.destroy_image_view(t.view, None);
            device.destroy_image(t.image, None);
            device.free_memory(t.memory, None);
        };
        let failed = post.set_chains(device, &mem_props, gui_layout, prepared, &free);
        assert!(failed.is_empty(), "chains failed: {failed:?}");
        post.set_blit(device, scene, blit.as_ref()).expect("blit");
        let (image, memory, view) =
            super::vk_util::create_depth_image(device, &mem_props, depth, extent, true)
                .expect("depth");
        post.resize(device, &mem_props, extent, &[], Some((image, depth)))
            .expect("resize");
        post.resize(device, &mem_props, extent, &[], Some((image, depth)))
            .expect("resize again");

        device.device_wait_idle().expect("idle");
        post.destroy(device, &free);
        device.destroy_image_view(view, None);
        device.destroy_image(image, None);
        device.free_memory(memory, None);
        device.destroy_descriptor_set_layout(gui_layout, None);
        for pass in [scene, split, resume] {
            device.destroy_render_pass(pass, None);
        }
    }
    packs::activate(ResourceStack::default());
    let messages = MESSAGES.lock().expect("validation log");
    assert!(
        messages.is_empty(),
        "Vulkan validation:\n{}",
        messages.join("\n")
    );
}

#[test]
fn khronos_validation_layer_is_on_the_loader_path() {
    ensure_layer_search_path();
    let entry = unsafe { ash::Entry::load() }.expect("libvulkan");
    let layer = CString::new("VK_LAYER_KHRONOS_validation").unwrap();
    let found = unsafe {
        entry
            .enumerate_instance_layer_properties()
            .expect("instance layers")
            .iter()
            .any(|l| CStr::from_ptr(l.layer_name.as_ptr()) == layer.as_c_str())
    };
    assert!(
        found,
        "VK_LAYER_KHRONOS_validation not visible to the loader (VK_LAYER_PATH={:?})",
        std::env::var_os("VK_LAYER_PATH")
    );
}
