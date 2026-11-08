use cachecat::{cache::Texture, preview::PreviewWorker};
use gpui::{App, RenderImage, Window};
use std::{collections::HashMap, path::PathBuf, sync::Arc};

/// Retains only the grid viewport and selected detail image, never the catalogue.
pub(super) struct MemoryPreviews {
    worker: PreviewWorker,
    generation: u64,
    wanted: Vec<usize>,
    images: HashMap<usize, Result<Arc<RenderImage>, String>>,
}

impl MemoryPreviews {
    pub fn new() -> Self {
        Self {
            worker: PreviewWorker::start(),
            generation: 0,
            wanted: Vec::new(),
            images: HashMap::new(),
        }
    }

    pub fn get(&self, item: usize) -> Option<&Result<Arc<RenderImage>, String>> {
        self.images.get(&item)
    }

    pub fn update(
        &mut self,
        generation: u64,
        path: Option<PathBuf>,
        items: Vec<(usize, Texture)>,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let wanted: Vec<_> = items.iter().map(|(i, _)| *i).collect();
        let mut changed = false;
        if generation != self.generation || wanted != self.wanted {
            let evict: Vec<_> = self
                .images
                .keys()
                .copied()
                .filter(|i| generation != self.generation || !wanted.contains(i))
                .collect();
            for i in evict {
                if let Some(Ok(image)) = self.images.remove(&i) {
                    cx.drop_image(image, Some(window));
                }
                changed = true;
            }
            self.generation = generation;
            self.wanted = wanted;
            self.worker.request(
                generation,
                path.unwrap_or_default(),
                items
                    .into_iter()
                    .filter(|(i, _)| !self.images.contains_key(i))
                    .collect(),
            );
        }
        for preview in self.worker.events.try_iter() {
            if preview.generation != generation || !self.wanted.contains(&preview.item) {
                continue;
            }
            // GPUI expects BGRA, while the cache decoder provides RGBA.
            let image = preview.image.map(|mut image| {
                for pixel in image.pixels_mut() {
                    pixel.0.swap(0, 2);
                }
                Arc::new(RenderImage::new(vec![image::Frame::new(image)]))
            });
            if let Some(Ok(previous)) = self.images.insert(preview.item, image) {
                cx.drop_image(previous, Some(window));
            }
            changed = true;
        }
        changed
    }
}
