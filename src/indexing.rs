//! Selectable indexing devices. Helpers stay warm so switching needs no reload.
use crate::model::Model;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexingMode {
    #[default]
    #[serde(alias = "GpuCpu")]
    Gpu,
    Cpu,
}

impl IndexingMode {
    pub const ALL: [Self; 2] = [Self::Gpu, Self::Cpu];

    pub fn label(self) -> &'static str {
        match self {
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
        1
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

    /// Process images only on the selected device. The other helper stays idle
    /// so changing devices does not need to reload the model.
    pub fn images(&mut self, pixels: Vec<Vec<f32>>) -> Vec<Result<Vec<f32>>> {
        assert!(pixels.len() <= self.batch_size());
        let results = pixels
            .into_iter()
            .map(|pixels| match (&mut self.gpu, self.mode) {
                (Some(gpu), IndexingMode::Gpu) => gpu.image_prepared(pixels),
                _ => self.cpu.image_prepared(pixels),
            })
            .collect();
        self.reconcile();
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_mixed_mode_migrates_to_gpu_without_losing_cpu_preferences() {
        assert_eq!(IndexingMode::ALL, [IndexingMode::Gpu, IndexingMode::Cpu]);
        assert_eq!(IndexingMode::default(), IndexingMode::Gpu);
        let migrated: IndexingMode = serde_json::from_str(r#""GpuCpu""#).unwrap();
        assert_eq!(migrated, IndexingMode::Gpu);
        assert_eq!(migrated.supported(false), IndexingMode::Cpu);
        assert_eq!(serde_json::to_string(&migrated).unwrap(), r#""Gpu""#);
        assert_eq!(
            serde_json::from_str::<IndexingMode>(r#""Cpu""#).unwrap(),
            IndexingMode::Cpu
        );
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
