//! Selectable indexing devices. Helpers stay warm so switching needs no reload.
use crate::model::Model;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Instant};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexingMode {
    #[default]
    GpuCpu,
    Gpu,
    Cpu,
}

impl IndexingMode {
    pub const ALL: [Self; 3] = [Self::GpuCpu, Self::Gpu, Self::Cpu];

    pub fn label(self) -> &'static str {
        match self {
            Self::GpuCpu => "GPU+CPU",
            Self::Gpu => "GPU",
            Self::Cpu => "CPU",
        }
    }

    pub fn available(self, gpu: bool) -> bool {
        self == Self::Cpu || gpu
    }

    pub fn supported(self, gpu: bool) -> Self {
        if self.available(gpu) { self } else { Self::Cpu }
    }
}

pub struct Indexers {
    cpu: Model,
    gpu: Option<Model>,
    pub mode: IndexingMode,
    gpu_jobs: usize,
    timings: Option<(f64, f64)>,
}

impl Indexers {
    pub fn load(dir: &Path, mode: IndexingMode, try_gpu: bool) -> Result<Self> {
        let cpu = Model::load(dir, true)?;
        let gpu = if try_gpu {
            Model::load(dir, false).ok().filter(|m| m.accelerated())
        } else {
            None
        };
        let mut pool = Self {
            cpu,
            gpu,
            mode,
            gpu_jobs: 1,
            timings: None,
        };
        // Validate actual execution, not just provider registration. This also
        // warms the first GPU graph before users switch devices during indexing.
        if let Some(gpu) = &mut pool.gpu
            && (gpu.image_prepared(vec![0.; 3 * 224 * 224]).is_err()
                || gpu.text("texture").is_err()
                || !gpu.accelerated())
        {
            pool.gpu = None;
        }
        pool.reconcile();
        Ok(pool)
    }

    pub fn gpu_available(&self) -> bool {
        self.gpu.as_ref().is_some_and(Model::accelerated)
    }

    pub fn select(&mut self, mode: IndexingMode) {
        self.mode = mode.supported(self.gpu_available());
    }

    fn reconcile(&mut self) {
        if !self.gpu_available() {
            self.gpu = None;
            self.mode = IndexingMode::Cpu;
        }
    }

    pub fn batch_size(&self) -> usize {
        if self.mode == IndexingMode::GpuCpu {
            self.gpu_jobs + 1
        } else {
            1
        }
    }

    pub fn backend(&self) -> String {
        match (&self.gpu, self.mode) {
            (_, IndexingMode::Cpu) | (None, _) => "CPU".into(),
            (Some(gpu), mode) => format!(
                "{} · {}",
                mode.label(),
                gpu.backend.trim_end_matches(" + CPU")
            ),
        }
    }

    pub fn text(&mut self, query: &str) -> Result<Vec<f32>> {
        let result = match (&mut self.gpu, self.mode) {
            (Some(gpu), IndexingMode::Gpu) => gpu.text(query),
            _ => self.cpu.text(query),
        };
        self.reconcile();
        result
    }

    /// At most one in-flight image per helper. In mixed mode the faster GPU
    /// can process several images while the CPU processes one. Batch size is
    /// bounded to keep device changes and searches responsive.
    pub fn images(&mut self, pixels: Vec<Vec<f32>>) -> Vec<Result<Vec<f32>>> {
        assert!(pixels.len() <= self.batch_size());
        let mut pixels = pixels.into_iter();
        let Some(first) = pixels.next() else {
            return vec![];
        };
        let mut timing = None;
        let results = match (&mut self.gpu, self.mode) {
            (Some(gpu), IndexingMode::GpuCpu) => {
                if let Some(second) = pixels.next() {
                    std::thread::scope(|scope| {
                        let cpu = &mut self.cpu;
                        let task = scope.spawn(move || {
                            let start = Instant::now();
                            let result = cpu.image_prepared(second);
                            (result, start.elapsed().as_secs_f64())
                        });
                        let start = Instant::now();
                        let first = gpu.image_prepared(first);
                        let mut rest: Vec<_> =
                            pixels.map(|pixels| gpu.image_prepared(pixels)).collect();
                        let gpu_seconds = start.elapsed().as_secs_f64() / (rest.len() + 1) as f64;
                        let (second, cpu_seconds) = task.join().unwrap_or_else(|_| {
                            (Err(anyhow::anyhow!("CPU indexing worker panicked")), 0.)
                        });
                        if first.is_ok() && second.is_ok() && rest.iter().all(Result::is_ok) {
                            timing = Some((gpu_seconds, cpu_seconds));
                        }
                        let mut results = vec![first, second];
                        results.append(&mut rest);
                        results
                    })
                } else {
                    vec![gpu.image_prepared(first)]
                }
            }
            (Some(gpu), IndexingMode::Gpu) => vec![gpu.image_prepared(first)],
            _ => vec![self.cpu.image_prepared(first)],
        };
        if let Some((gpu, cpu)) = timing {
            let (gpu, cpu) = match self.timings {
                Some((old_gpu, old_cpu)) => {
                    (old_gpu * 0.75 + gpu * 0.25, old_cpu * 0.75 + cpu * 0.25)
                }
                None => (gpu, cpu),
            };
            self.timings = Some((gpu, cpu));
            self.gpu_jobs = balanced_gpu_jobs(gpu, cpu);
        }
        self.reconcile();
        results
    }
}

fn balanced_gpu_jobs(gpu_seconds: f64, cpu_seconds: f64) -> usize {
    // Also cap the GPU's share to roughly 100 ms per batch.
    let time_limit = (0.1 / gpu_seconds).floor().clamp(1., 7.) as usize;
    (cpu_seconds / gpu_seconds)
        .round()
        .clamp(1., time_limit as f64) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mixed_batches_balance_devices_and_bound_latency() {
        assert_eq!(balanced_gpu_jobs(0.01, 0.023), 2);
        assert_eq!(balanced_gpu_jobs(0.03, 0.01), 1);
        assert_eq!(balanced_gpu_jobs(0.01, 1.), 7);
        assert_eq!(balanced_gpu_jobs(0.04, 1.), 2);
        assert_eq!(balanced_gpu_jobs(0.2, 1.), 1);
    }

    #[test]
    fn unsupported_gpu_modes_select_cpu() {
        for mode in IndexingMode::ALL {
            assert_eq!(mode.supported(false), IndexingMode::Cpu);
            assert_eq!(mode.supported(true), mode);
            assert_eq!(mode.available(false), mode == IndexingMode::Cpu);
        }
    }
}
