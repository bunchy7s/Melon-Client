//! Recording the UI quads over the scene.

use crate::renderer::quad::DrawCall;
use crate::renderer::{Frame, Renderer, pipeline, quad};
use ash::vk;

impl Renderer {
    /// UI quads, in GUI coordinates: `calls` of `frame.ui`.
    pub(in crate::renderer) unsafe fn record_ui(
        &self,
        cmd: vk::CommandBuffer,
        frame: &Frame<'_>,
        calls: &[DrawCall],
        full_scissor: vk::Rect2D,
    ) {
        if calls.is_empty() {
            return;
        }
        let push = [frame.ui_size.0, frame.ui_size.1];
        let bind = |pass: &pipeline::Pass| {
            self.device
                .cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pass.pipeline);
            self.device.cmd_push_constants(
                cmd,
                pass.layout,
                vk::ShaderStageFlags::VERTEX,
                0,
                pipeline::push_constant_bytes(&push, pipeline::ui::PUSH_CONSTANT_SIZE),
            );
        };
        let ui = &self.pipelines.ui;
        let mut applied = (quad::Program::Sprite, quad::BlendMode::Alpha);
        bind(ui.pass(applied.0, applied.1));
        self.device.cmd_bind_vertex_buffers(
            cmd,
            0,
            &[self.ui_vertices.buffers[self.frames.current]],
            &[0],
        );
        self.device.cmd_bind_index_buffer(
            cmd,
            self.ui_vertices.indices.0,
            0,
            vk::IndexType::UINT32,
        );
        self.device.cmd_bind_descriptor_sets(
            cmd,
            vk::PipelineBindPoint::GRAPHICS,
            ui.layout(),
            1,
            &[self.gui_uniforms.set(self.frames.current)],
            &[],
        );

        let scale_x = self.swapchain.extent.width as f32 / frame.ui_size.0;
        let scale_y = self.swapchain.extent.height as f32 / frame.ui_size.1;
        let mut applied_scissor = full_scissor;
        self.device.cmd_set_scissor(cmd, 0, &[full_scissor]);
        for call in calls {
            let wanted = match call.scissor {
                Some([x, y, w, h]) => vk::Rect2D {
                    offset: vk::Offset2D {
                        x: (x * scale_x).round() as i32,
                        y: (y * scale_y).round() as i32,
                    },
                    extent: vk::Extent2D {
                        width: (w * scale_x).round() as u32,
                        height: (h * scale_y).round() as u32,
                    },
                },
                None => full_scissor,
            };
            if wanted != applied_scissor {
                self.device.cmd_set_scissor(cmd, 0, &[wanted]);
                applied_scissor = wanted;
            }
            let wanted_pipeline = (call.program, call.blend);
            if wanted_pipeline != applied {
                let (from, to) = (
                    ui.pass(applied.0, applied.1),
                    ui.pass(call.program, call.blend),
                );
                if from.pipeline != to.pipeline {
                    bind(to);
                }
                applied = wanted_pipeline;
            }
            let set = self.textures.list[call.texture.0].descriptor_set;
            self.device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                ui.layout(),
                0,
                &[set],
                &[],
            );
            self.device
                .cmd_draw_indexed(cmd, call.count, 1, call.first, 0, 0);
        }
    }
}
