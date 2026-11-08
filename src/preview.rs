//! Viewport-only, read-only preview decoding. No image files are written.
use crate::cache::{self, Cache, Texture};
use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{self, Receiver},
    },
};

pub struct Preview {
    pub generation: u64,
    pub item: usize,
    pub image: Result<image::RgbaImage, String>,
}

#[derive(Default)]
struct Pending {
    generation: u64,
    path: PathBuf,
    wanted: HashSet<usize>,
    queue: VecDeque<(usize, Texture)>,
    stopped: bool,
}

impl Pending {
    fn replace(&mut self, generation: u64, path: PathBuf, items: Vec<(usize, Texture)>) {
        self.generation = generation;
        self.path = path;
        self.wanted = items.iter().map(|(i, _)| *i).collect();
        self.queue = items.into();
    }
}

pub struct PreviewWorker {
    pending: Arc<(Mutex<Pending>, Condvar)>,
    pub events: Receiver<Preview>,
}

impl Default for PreviewWorker {
    fn default() -> Self {
        Self::start()
    }
}

impl PreviewWorker {
    pub fn start() -> Self {
        let pending = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
        let shared = pending.clone();
        // At most two completed images plus one active decode can wait for the UI.
        let (tx, events) = mpsc::sync_channel(2);
        std::thread::Builder::new()
            .name("cachecat-previews".into())
            .spawn(move || {
                let (lock, wake) = &*shared;
                let mut cache: Option<(u64, Result<Cache, String>)> = None;
                loop {
                    let (generation, path, item, texture) = {
                        let mut pending = lock.lock().unwrap();
                        while pending.queue.is_empty() && !pending.stopped {
                            pending = wake.wait(pending).unwrap();
                        }
                        if pending.stopped {
                            return;
                        }
                        let (item, texture) = pending.queue.pop_front().unwrap();
                        (pending.generation, pending.path.clone(), item, texture)
                    };
                    if cache.as_ref().is_none_or(|(g, _)| *g != generation) {
                        cache =
                            Some((generation, Cache::open(&path).map_err(|e| format!("{e:#}"))));
                    }
                    let image = match &cache.as_ref().unwrap().1 {
                        Ok(cache) => cache
                            .read(&texture)
                            .and_then(|bytes| cache::decode(&bytes))
                            .map(|image| image.thumbnail(256, 256).into_rgba8())
                            .map_err(|e| format!("{e:#}")),
                        Err(error) => Err(error.clone()),
                    };
                    let current = {
                        let pending = lock.lock().unwrap();
                        !pending.stopped
                            && pending.generation == generation
                            && pending.wanted.contains(&item)
                    };
                    if current
                        && tx
                            .send(Preview {
                                generation,
                                item,
                                image,
                            })
                            .is_err()
                    {
                        return;
                    }
                }
            })
            .expect("start preview worker");
        Self { pending, events }
    }

    /// Replace queued work instead of accumulating requests while the user scrolls.
    pub fn request(&self, generation: u64, path: PathBuf, items: Vec<(usize, Texture)>) {
        let (lock, wake) = &*self.pending;
        lock.lock().unwrap().replace(generation, path, items);
        wake.notify_one();
    }
}

impl Drop for PreviewWorker {
    fn drop(&mut self) {
        let (lock, wake) = &*self.pending;
        lock.lock().unwrap().stopped = true;
        wake.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn requests_replace_old_viewport_work() {
        let cache = Cache::open(std::path::Path::new("tests/fixtures/texturecache")).unwrap();
        let mut pending = Pending::default();
        pending.replace(
            1,
            cache.root.clone(),
            cache.textures.iter().cloned().enumerate().collect(),
        );
        pending.replace(2, cache.root, vec![(3, cache.textures[3].clone())]);
        assert_eq!(pending.generation, 2);
        assert_eq!(pending.queue.len(), 1);
        assert_eq!(pending.wanted, HashSet::from([3]));
    }

    #[test]
    fn decodes_only_requested_texture_without_creating_files() {
        let cache = Cache::open(std::path::Path::new("tests/fixtures/texturecache")).unwrap();
        let worker = PreviewWorker::start();
        assert!(
            worker
                .events
                .recv_timeout(Duration::from_millis(30))
                .is_err()
        );
        worker.request(7, cache.root.clone(), vec![(2, cache.textures[2].clone())]);
        let preview = worker.events.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!((preview.generation, preview.item), (7, 2));
        let image = preview.image.unwrap();
        assert!(image.width() <= 256 && image.height() <= 256);
        let expected = cache::decode(&cache.read(&cache.textures[2]).unwrap())
            .unwrap()
            .thumbnail(256, 256)
            .into_rgba8();
        assert_eq!(image, expected);
        assert!(
            worker
                .events
                .recv_timeout(Duration::from_millis(30))
                .is_err()
        );
        assert!(!cache.root.join("thumbnails").exists());
    }
}
