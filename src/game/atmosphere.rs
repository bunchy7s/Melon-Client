//! Cloud and weather meshes. The weather's vertices are camera-relative; the

use super::InWorld;
use super::camera::Camera;
use super::environment::FrameEnvironment;
use super::world_resources::WorldResources;
use crate::renderer::{EntityDraw, MeshId, Renderer, WorldVertex};
use crate::settings::options::Config;
use crate::world::clouds::{self, CloudFrame};
use crate::world::weather::{self, WeatherQuery};
use glam::{Mat4, Vec3};

impl InWorld {
    /// Cloud boxes, then rain, then snow, matching vanilla's atmosphere pass.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn atmosphere_draws(
        &mut self,
        renderer: &mut Renderer,
        assets: &WorldResources,
        camera: &Camera,
        config: &Config,
        look: &FrameEnvironment,
        lightmap_params: &crate::world::lightmap::LightmapParams,
        partial: f32,
        aspect: f32,
    ) -> (Option<EntityDraw>, Vec<EntityDraw>) {
        let model = Mat4::from_translation(camera.position);
        let cloud_frame = CloudFrame {
            status: config.clouds,
            color: look.atmosphere.cloud_color,
            bottom_y: look.atmosphere.cloud_height,
            range_chunks: config.cloud_range,
            camera: camera.position,
            game_time: self.clocks.game_time(),
            partial_tick: partial,
        };
        let cloud_key = clouds::key(&assets.clouds, &cloud_frame);
        if cloud_key != self.cloud_key || (cloud_key.is_some() && self.cloud_mesh.is_none()) {
            let verts = clouds::build_anchored(&assets.clouds, cloud_frame);
            replace_mesh(renderer, &mut self.cloud_mesh, &verts);
            self.cloud_key = cloud_key;
        }
        let clouds = self.cloud_mesh.map(|mesh| EntityDraw {
            mesh,
            atlas: renderer.white_texture(),
            model: Mat4::from_translation(clouds::anchor(&assets.clouds, &cloud_frame)),
            origin: glam::IVec3::ZERO,
            layer: crate::renderer::RenderLayer::Atmosphere,
            translucent: true,
            light: None,
            overlay: 0.0,
            glint: crate::renderer::Glint::None,
        });

        let mut weather = Vec::new();
        let intensity = self.weather.rain;
        let radius = i32::from(config.weather_radius);
        if intensity > 0.0 && radius > 0 {
            let level = &self.level;
            let biomes = &self.biomes;
            let at = camera.position.floor().as_ivec3();
            let key = weather::PrepareKey {
                block: at.to_array(),
                revision: level.revision(),
                radius,
            };
            if self.weather_columns.as_ref().is_none_or(|(k, _)| *k != key) {
                let height = |x, z| level.motion_blocking_height(x, z);
                let precipitation = |x, y, z| {
                    level
                        .biome(x, y, z)
                        .and_then(|id| biomes.biome(id))
                        .map(|biome| biome.precipitation())
                        .unwrap_or(weather::Precipitation::None)
                };
                let light = |x, y, z| level.light_at(x, y, z);
                let prepared = weather::prepare(&WeatherQuery {
                    intensity,
                    radius,
                    camera: camera.position,
                    game_time: 0,
                    partial_tick: 0.0,
                    height: &height,
                    precipitation: &precipitation,
                    light: &light,
                });
                self.weather_columns = Some((key, prepared));
            }
            let frustum = camera.frustum(aspect);
            let game_time = self.clocks.game_time();
            let columns: Vec<_> = self
                .weather_columns
                .iter()
                .flat_map(|(_, prepared)| prepared)
                .map(|p| p.column(game_time, partial))
                .filter(|c| {
                    let min = Vec3::new(c.x as f32 - 0.5, c.bottom_y as f32, c.z as f32 - 0.5);
                    let max = Vec3::new(c.x as f32 + 1.5, c.top_y as f32, c.z as f32 + 1.5);
                    frustum.sees_box(min, max)
                })
                .collect();
            let (rain, snow) = weather::mesh(
                &columns,
                camera.position,
                radius,
                intensity,
                lightmap_params,
            );
            if let Some(mesh) = replace_mesh(renderer, &mut self.rain_mesh, &rain) {
                weather.push(EntityDraw {
                    mesh,
                    atlas: assets.rain,
                    model,
                    origin: glam::IVec3::ZERO,
                    layer: crate::renderer::RenderLayer::Atmosphere,
                    translucent: true,
                    light: None,
                    overlay: 0.0,
                    glint: crate::renderer::Glint::None,
                });
            }
            if let Some(mesh) = replace_mesh(renderer, &mut self.snow_mesh, &snow) {
                weather.push(EntityDraw {
                    mesh,
                    atlas: assets.snow,
                    model,
                    origin: glam::IVec3::ZERO,
                    layer: crate::renderer::RenderLayer::Atmosphere,
                    translucent: true,
                    light: None,
                    overlay: 0.0,
                    glint: crate::renderer::Glint::None,
                });
            }
        } else {
            replace_mesh(renderer, &mut self.rain_mesh, &[]);
            replace_mesh(renderer, &mut self.snow_mesh, &[]);
        }
        (clouds, weather)
    }
}

fn replace_mesh(
    renderer: &mut Renderer,
    slot: &mut Option<MeshId>,
    verts: &[WorldVertex],
) -> Option<MeshId> {
    if let Some(old) = slot.take() {
        renderer.free_mesh(old);
    }
    if verts.is_empty() {
        return None;
    }
    match renderer.upload_mesh(verts) {
        Ok(mesh) => {
            *slot = Some(mesh);
            Some(mesh)
        }
        Err(e) => {
            eprintln!("Atmosphären-Mesh fehlgeschlagen: {e:#}");
            None
        }
    }
}
