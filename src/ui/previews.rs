use cachecat::{cache::Texture, preview::PreviewWorker};
use gpui::{App, RenderImage, Window};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::Arc,
};

// GPUI's GPU copies and bookkeeping are additional to this decoded pixel budget.
const PREVIEW_BYTES: usize = 256 * 1024 * 1024;
// Bound metadata too, including failed previews with no pixel allocation.
const PREVIEW_ENTRIES: usize = 8192;

struct RecentPreviews<T> {
    values: HashMap<usize, T>,
    sizes: HashMap<usize, usize>,
    bytes: usize,
    order: VecDeque<usize>,
}

impl<T> RecentPreviews<T> {
    fn new() -> Self {
        Self {
            values: HashMap::new(),
            sizes: HashMap::new(),
            bytes: 0,
            order: VecDeque::new(),
        }
    }

    fn touch(&mut self, item: usize) {
        if self.values.contains_key(&item) {
            self.order.retain(|&i| i != item);
            self.order.push_back(item);
        }
    }

    fn insert(&mut self, item: usize, value: T, bytes: usize) -> Option<T> {
        self.bytes -= self.sizes.insert(item, bytes).unwrap_or(0);
        self.bytes += bytes;
        let previous = self.values.insert(item, value);
        self.touch(item);
        previous
    }

    fn trim(&mut self, byte_budget: usize, pinned: &[usize]) -> Vec<T> {
        let mut removed = Vec::new();
        // A very large viewport may exceed the normal budget; never evict its
        // visible images and cause a decode/evict loop.
        while self.bytes > byte_budget || self.values.len() > PREVIEW_ENTRIES.max(pinned.len()) {
            let Some(position) = self.order.iter().position(|i| !pinned.contains(i)) else {
                break;
            };
            let item = self.order.remove(position).unwrap();
            self.bytes -= self.sizes.remove(&item).unwrap_or(0);
            if let Some(value) = self.values.remove(&item) {
                removed.push(value);
            }
        }
        removed
    }

    fn clear(&mut self) -> Vec<T> {
        self.order.clear();
        self.sizes.clear();
        self.bytes = 0;
        self.values.drain().map(|(_, value)| value).collect()
    }
}

/// Keeps recently viewed previews in memory, with visible/selected items pinned.
pub(super) struct MemoryPreviews {
    worker: PreviewWorker,
    generation: u64,
    wanted: Vec<usize>,
    images: RecentPreviews<Result<Arc<RenderImage>, String>>,
}

impl MemoryPreviews {
    pub fn new() -> Self {
        Self {
            worker: PreviewWorker::start(),
            generation: 0,
            wanted: Vec::new(),
            images: RecentPreviews::new(),
        }
    }

    pub fn get(&self, item: usize) -> Option<&Result<Arc<RenderImage>, String>> {
        self.images.values.get(&item)
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
        let viewport_changed = generation != self.generation || wanted != self.wanted;
        if generation != self.generation {
            for image in self.images.clear().into_iter().flatten() {
                cx.drop_image(image, Some(window));
            }
            self.generation = generation;
            changed = true;
        }
        for preview in self.worker.events.try_iter() {
            if preview.generation != generation {
                continue;
            }
            // GPUI expects BGRA, while the cache decoder provides RGBA.
            let bytes = preview
                .image
                .as_ref()
                .map_or(0, |image| image.as_raw().len());
            let image = preview.image.map(|mut image| {
                for pixel in image.pixels_mut() {
                    pixel.0.swap(0, 2);
                }
                Arc::new(RenderImage::new(vec![image::Frame::new(image)]))
            });
            if let Some(Ok(previous)) = self.images.insert(preview.item, image, bytes) {
                cx.drop_image(previous, Some(window));
            }
            changed = true;
        }
        if viewport_changed {
            self.wanted = wanted;
            // Returning to a retained tile refreshes its recency without decoding.
            for &item in &self.wanted {
                self.images.touch(item);
            }
            self.worker.request(
                generation,
                path.unwrap_or_default(),
                items
                    .into_iter()
                    .filter(|(i, _)| !self.images.values.contains_key(i))
                    .collect(),
            );
        }
        for image in self
            .images
            .trim(PREVIEW_BYTES, &self.wanted)
            .into_iter()
            .flatten()
        {
            cx.drop_image(image, Some(window));
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrolling_back_reuses_previews_and_evicts_least_recently_viewed() {
        let mut cache = RecentPreviews::new();
        for i in 0..3 {
            cache.insert(i, i, 1);
        }
        assert!(cache.trim(3, &[2]).is_empty());
        cache.touch(0); // Return to an earlier row, then continue browsing.
        cache.insert(3, 3, 1);
        assert_eq!(cache.trim(3, &[3]), vec![1]);
        assert!(cache.values.contains_key(&0));
        assert_eq!(cache.values.len(), 3);
    }

    #[test]
    fn visible_and_selected_previews_survive_eviction() {
        let mut cache = RecentPreviews::new();
        for i in 0..5 {
            cache.insert(i, i, 1);
        }
        assert_eq!(cache.trim(3, &[0, 1]), vec![2, 3]);
        cache.insert(5, 5, 1);
        assert!(cache.trim(3, &[0, 1, 4, 5]).is_empty());
        assert_eq!(cache.trim(3, &[5]), vec![0]);
        assert_eq!(cache.values.len(), 3);
    }

    #[test]
    fn rescan_releases_all_previews_and_recency() {
        let mut cache = RecentPreviews::new();
        cache.insert(0, "old preview", 100);
        assert_eq!(cache.clear(), vec!["old preview"]);
        assert!(cache.values.is_empty());
        assert!(cache.order.is_empty());
        assert_eq!(cache.bytes, 0);
        cache.insert(0, "new preview", 50);
        assert_eq!(cache.values.get(&0), Some(&"new preview"));
    }

    #[test]
    fn budget_counts_actual_pixels_and_replacements() {
        let mut cache = RecentPreviews::new();
        cache.insert(0, "wide", 256 * 32 * 4);
        cache.insert(1, "square", 256 * 256 * 4);
        assert_eq!(cache.trim(256 * 256 * 4, &[]), vec!["wide"]);
        cache.insert(1, "small replacement", 16 * 16 * 4);
        assert_eq!(cache.bytes, 16 * 16 * 4);
        assert!(cache.trim(1024, &[]).is_empty());
    }
}
