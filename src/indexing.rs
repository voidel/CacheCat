//! Selectable indexing devices. Helpers stay warm so switching needs no reload.
use crate::model::Model;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

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
}

impl Indexers {
    pub fn load(dir: &Path, mode: IndexingMode, try_gpu: bool) -> Result<Self> {
        let cpu = Model::load(dir, true)?;
        let gpu = if try_gpu {
            Model::load(dir, false).ok().filter(|m| m.accelerated())
        } else {
            None
        };
        let mut pool = Self { cpu, gpu, mode };
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
            2
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

    /// At most one in-flight image per helper. Both use independent processes,
    /// and no embeddings are discarded when the selection changes.
    pub fn images(&mut self, pixels: Vec<Vec<f32>>) -> Vec<Result<Vec<f32>>> {
        assert!(pixels.len() <= self.batch_size());
        let mut pixels = pixels.into_iter();
        let Some(first) = pixels.next() else {
            return vec![];
        };
        let results = match (&mut self.gpu, self.mode) {
            (Some(gpu), IndexingMode::GpuCpu) => {
                if let Some(second) = pixels.next() {
                    std::thread::scope(|scope| {
                        let cpu = &mut self.cpu;
                        let task = scope.spawn(move || cpu.image_prepared(second));
                        let first = gpu.image_prepared(first);
                        let second = task.join().unwrap_or_else(|_| {
                            Err(anyhow::anyhow!("CPU indexing worker panicked"))
                        });
                        vec![first, second]
                    })
                } else {
                    vec![gpu.image_prepared(first)]
                }
            }
            (Some(gpu), IndexingMode::Gpu) => vec![gpu.image_prepared(first)],
            _ => vec![self.cpu.image_prepared(first)],
        };
        self.reconcile();
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_gpu_modes_select_cpu() {
        for mode in IndexingMode::ALL {
            assert_eq!(mode.supported(false), IndexingMode::Cpu);
            assert_eq!(mode.supported(true), mode);
            assert_eq!(mode.available(false), mode == IndexingMode::Cpu);
        }
    }
}
