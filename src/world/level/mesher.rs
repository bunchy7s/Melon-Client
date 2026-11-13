//! The mesh worker threads (vanilla `SectionRenderDispatcher`): a section

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender};
use glam::Vec3;

use crate::math::SECTION_SIZE;
use crate::world::block_models::BlockModels;
use crate::world::block_tint::TintSettings;
use crate::world::chunk::Chunk;
use crate::world::culling::occlusion::SectionPos;
use crate::world::fluid::FluidSprites;
use crate::world::terrain::{self, MeshContext, Neighborhood, SectionMesh};

/// A section to mesh, as of `version`.
pub(super) struct Job {
    pub pos: SectionPos,
    pub version: u64,
    /// The section's chunk at the centre (index 4) and the eight around
    pub chunks: [Option<Arc<Chunk>>; 9],
    /// The "Smooth Lighting" option.
    pub smooth_lighting: bool,
    /// The dimension's face shading, relative to the default.
    pub cardinal: [f32; 6],
    /// Where the camera was, to sort the translucent faces from.
    pub camera: Vec3,
    /// Biome colours and the blend radius.
    pub tints: TintSettings,
}

pub(super) struct Done {
    pub pos: SectionPos,
    pub version: u64,
    pub mesh: SectionMesh,
}

/// Meshes section `s` of the chunk at the centre of `chunks`, its
pub(super) fn mesh(
    chunks: &[Option<Arc<Chunk>>; 9],
    (x, s, z): SectionPos,
    context: &MeshContext,
    camera: Vec3,
) -> Option<SectionMesh> {
    let center = chunks[4].as_deref()?;
    let context = &MeshContext {
        min_y: center.min_y(),
        ..*context
    };
    let mut neighborhood = Neighborhood::alone(center);
    for (slot, chunk) in neighborhood.chunks.iter_mut().zip(chunks) {
        *slot = chunk.as_deref();
    }
    let mut mesh = terrain::mesh_section(&neighborhood, usize::try_from(s).ok()?, context);
    let corner = Vec3::new((x * SECTION_SIZE) as f32, 0.0, (z * SECTION_SIZE) as f32);
    mesh.translucent.sort(camera - corner);
    Some(mesh)
}

/// Half the hardware threads, at most eight: the integrated server and the
pub(super) struct Mesher {
    jobs: Option<Sender<Job>>,
    results: Receiver<Done>,
    workers: Vec<JoinHandle<()>>,
    /// Set when the mesher is dropped: the workers leave the jobs still
    stop: Arc<AtomicBool>,
    pub in_flight: usize,
    /// Jobs handed out at a time: enough to keep the workers busy, few
    pub limit: usize,
}

impl Mesher {
    pub fn new(models: Arc<BlockModels>, fluids: FluidSprites) -> Self {
        let count = std::thread::available_parallelism()
            .map_or(2, |n| n.get() / 2)
            .clamp(1, 8);
        let (jobs, job_rx) = crossbeam_channel::unbounded::<Job>();
        let (done_tx, results) = crossbeam_channel::unbounded();
        let stop = Arc::new(AtomicBool::new(false));
        let workers = (0..count)
            .filter_map(|i| {
                let job_rx = job_rx.clone();
                let stop = Arc::clone(&stop);
                let done_tx = done_tx.clone();
                let models = Arc::clone(&models);
                std::thread::Builder::new()
                    .name(format!("chunk-mesher-{i}"))
                    .spawn(move || work(&job_rx, &done_tx, &models, fluids, &stop))
                    .ok()
            })
            .collect::<Vec<_>>();
        let limit = workers.len() * 32;
        Self {
            jobs: Some(jobs),
            results,
            workers,
            stop,
            in_flight: 0,
            limit,
        }
    }

    /// Room for more jobs.
    pub fn room(&self) -> usize {
        self.limit.saturating_sub(self.in_flight)
    }

    /// Hands out a job. `false` once the workers are gone.
    pub fn send(&mut self, job: Job) -> bool {
        let Some(jobs) = &self.jobs else {
            return false;
        };
        if jobs.send(job).is_err() {
            return false;
        }
        self.in_flight += 1;
        true
    }

    /// The meshes finished since the last call.
    pub fn finished(&mut self) -> Vec<Done> {
        let done: Vec<Done> = self.results.try_iter().collect();
        self.in_flight -= done.len();
        done
    }
}

/// One worker: takes jobs until the channel closes or `stop` is set.
fn work(
    jobs: &Receiver<Job>,
    done: &Sender<Done>,
    models: &BlockModels,
    fluids: FluidSprites,
    stop: &AtomicBool,
) {
    crate::platform::lower_thread_priority();
    for job in jobs {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let context = MeshContext {
            smooth_lighting: job.smooth_lighting,
            models,
            fluids: &fluids,
            min_y: 0,
            cardinal: job.cardinal,
            tints: &job.tints,
        };
        let Some(mesh) = mesh(&job.chunks, job.pos, &context, job.camera) else {
            continue;
        };
        let result = Done {
            pos: job.pos,
            version: job.version,
            mesh,
        };
        if done.send(result).is_err() {
            return;
        }
    }
}

impl Drop for Mesher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.jobs = None;
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}
