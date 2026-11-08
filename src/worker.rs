use crate::{
    cache::{self, Cache, Texture},
    index::{self, Index},
    model::{self, Model},
};
use anyhow::Result;
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct Item {
    pub texture: Texture,
    pub dimensions: Option<(u32, u32)>,
    pub error: Option<String>,
    pub indexed: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchMode {
    #[default]
    Semantic,
    Uuid,
}
impl SearchMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Semantic => "Semantic",
            Self::Uuid => "UUID",
        }
    }
}
pub enum Command {
    Open(u64, PathBuf),
    Search(u64, String, SearchMode),
    Download(Arc<AtomicBool>),
    Pause(bool),
}
pub enum Event {
    Loaded(u64, Vec<Item>),
    Item(u64, usize, Item),
    Results(u64, String, SearchMode, Vec<usize>),
    Status(String),
    Backend(String),
    Error(String),
    ModelReady(bool),
    DownloadProgress(u64, u64),
    DownloadFailed(String),
    DownloadCancelled,
}
pub struct Worker {
    pub commands: Sender<Command>,
    pub events: Receiver<Event>,
}
impl Worker {
    pub fn start(data: PathBuf, cpu: bool, legacy: Option<PathBuf>) -> Self {
        let (tx, rx) = mpsc::channel();
        let (events_tx, events_rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("cachecat-indexer".into())
            .spawn(move || {
                let result = (|| {
                    if let Some(legacy) = legacy {
                        crate::storage::import_legacy(&legacy, &data, |message| {
                            let _ = events_tx.send(Event::Status(message));
                        })?;
                    }
                    run(data, cpu, rx, &events_tx)
                })();
                if let Err(e) = result {
                    let _ = events_tx.send(Event::Error(format!("{e:#}")));
                }
            })
            .expect("start index worker");
        Self {
            commands: tx,
            events: events_rx,
        }
    }
}
#[derive(Default)]
struct State {
    generation: u64,
    cache: Option<Cache>,
    items: Vec<Item>,
    hashes: Vec<Option<String>>,
    vectors: Vec<Option<Vec<f32>>>,
    scan: VecDeque<usize>,
    embed: VecDeque<usize>,
    prepared: HashMap<usize, Vec<f32>>,
    query: String,
    mode: SearchMode,
    query_vector: Option<Vec<f32>>,
    paused: bool,
}
impl State {
    fn results(&self, tx: &Sender<Event>) {
        let mut results: Vec<usize> = (0..self.items.len()).collect();
        if self.mode == SearchMode::Uuid {
            let uuid = self.query.trim().to_lowercase().replace('-', "");
            results.retain(|&i| {
                self.items[i]
                    .texture
                    .id
                    .simple()
                    .to_string()
                    .contains(&uuid)
            });
        } else if !self.query.trim().is_empty() {
            if let Some(query) = &self.query_vector {
                results = index::rank(&self.vectors, query, 200);
            } else {
                results.clear();
            }
        }
        let _ = tx.send(Event::Results(
            self.generation,
            self.query.clone(),
            self.mode,
            results,
        ));
    }
}
fn run(data: PathBuf, cpu: bool, rx: Receiver<Command>, tx: &Sender<Event>) -> Result<()> {
    std::fs::create_dir_all(&data)?;
    let index = Index::open(&data.join("index.sqlite3"))?;
    let model_dir = data.join("models").join(model::MODEL_ID);
    let mut model: Option<Model> = None;
    let mut state = State {
        generation: 0,
        cache: None,
        items: vec![],
        hashes: vec![],
        vectors: vec![],
        scan: VecDeque::new(),
        embed: VecDeque::new(),
        prepared: HashMap::new(),
        query: String::new(),
        mode: SearchMode::Semantic,
        query_vector: None,
        paused: false,
    };
    let mut load_model = model::ready(&model_dir);
    let mut load_cancel: Option<Arc<AtomicBool>> = None;
    let mut since_embedding = 0_usize;
    let _ = tx.send(Event::ModelReady(load_model));
    let mut download: Option<(Receiver<Result<bool>>, Arc<AtomicBool>)> = None;
    loop {
        if let Some((rx, cancelled)) = &download
            && let Ok(result) = rx.try_recv()
        {
            match result {
                Ok(true) if !cancelled.load(Ordering::Relaxed) => {
                    load_model = true;
                    load_cancel = Some(cancelled.clone());
                    state.paused = false;
                }
                Ok(_) => {
                    let _ = tx.send(Event::DownloadCancelled);
                }
                Err(_) if cancelled.load(Ordering::Relaxed) => {
                    let _ = tx.send(Event::DownloadCancelled);
                }
                Err(e) => {
                    let _ = tx.send(Event::DownloadFailed(format!(
                        "Model download failed: {e:#}"
                    )));
                }
            }
            download = None;
        }
        let pending =
            !state.paused && (!state.scan.is_empty() || model.is_some() && !state.embed.is_empty());
        let command = match rx.recv_timeout(if pending || load_model {
            Duration::ZERO
        } else {
            Duration::from_millis(150)
        }) {
            Ok(c) => Some(c),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Some((_, cancelled)) = &download {
                    cancelled.store(true, Ordering::Relaxed);
                }
                return Ok(());
            }
            Err(_) => None,
        };
        if let Some(command) = command {
            match command {
                Command::Open(generation, path) => {
                    state.generation = generation;
                    // Drop previous work even when the new folder cannot be read.
                    state.cache = None;
                    state.items.clear();
                    state.scan.clear();
                    state.embed.clear();
                    state.prepared.clear();
                    state.hashes.clear();
                    state.vectors.clear();
                    match Cache::open(&path) {
                        Ok(cache) => {
                            state.items = cache
                                .textures
                                .iter()
                                .map(|t| Item {
                                    texture: t.clone(),
                                    dimensions: None,
                                    error: None,
                                    indexed: false,
                                })
                                .collect();
                            let n = state.items.len();
                            state.scan = (0..n).collect();
                            state.hashes = vec![None; n];
                            state.vectors = vec![None; n];
                            let _ = tx.send(Event::Loaded(generation, state.items.clone()));
                            state.cache = Some(cache);
                            state.results(tx);
                        }
                        Err(e) => {
                            let _ = tx.send(Event::Error(format!("{e:#}")));
                        }
                    }
                }
                Command::Pause(paused) => state.paused = paused,
                Command::Search(generation, query, mode) => {
                    if generation != state.generation {
                        continue;
                    }
                    state.query = query;
                    state.mode = mode;
                    state.query_vector = None;
                    if !state.query.trim().is_empty()
                        && state.mode == SearchMode::Semantic
                        && let Some(model) = model.as_mut()
                    {
                        match model.text(&state.query) {
                            Ok(v) => state.query_vector = Some(v),
                            Err(e) => {
                                let _ = tx.send(Event::Error(format!("Search: {e:#}")));
                            }
                        }
                    }
                    state.results(tx);
                }
                Command::Download(cancelled) => {
                    if download.is_none() {
                        let (done_tx, done_rx) = mpsc::channel();
                        let events = tx.clone();
                        let dir = model_dir.clone();
                        let cancel = cancelled.clone();
                        std::thread::spawn(move || {
                            let result =
                                model::download_cancellable(&dir, &cancel, |done, total| {
                                    let _ = events.send(Event::DownloadProgress(done, total));
                                });
                            let _ = done_tx.send(result);
                        });
                        download = Some((done_rx, cancelled));
                    }
                }
            }
            continue;
        }
        // Scan headers and saved embeddings without decoding off-screen previews.
        // Full image decoding happens only when an embedding is actually missing.
        let scanned = state.items.len() - state.scan.len();
        let scan_turn = scanned < 8
            || !load_model && (model.is_none() || since_embedding < 1 || state.embed.is_empty());
        if !state.paused
            && scan_turn
            && let Some(i) = state.scan.pop_front()
        {
            since_embedding += 1;
            let result = (|| -> Result<()> {
                let bytes = state
                    .cache
                    .as_ref()
                    .unwrap()
                    .read(&state.items[i].texture)?;
                let hash = cache::fingerprint(&bytes);
                state.items[i].dimensions = Some(cache::dimensions(&bytes)?);
                state.vectors[i] = index.get(&hash)?;
                if state.vectors[i].is_none()
                    && (load_model || model.is_some())
                    && state.prepared.len() < 16
                {
                    let image = cache::decode(&bytes)?;
                    state.items[i].dimensions = Some((image.width(), image.height()));
                    state.prepared.insert(i, model::preprocess(&image));
                }
                state.items[i].indexed = state.vectors[i].is_some();
                state.hashes[i] = Some(hash);
                if state.vectors[i].is_none() {
                    state.embed.push_back(i);
                }
                Ok(())
            })();
            if let Err(e) = result {
                state.items[i].error = Some(format!("{e:#}"));
            }
            let _ = tx.send(Event::Item(state.generation, i, state.items[i].clone()));
            if state.scan.len().is_multiple_of(16) {
                state.results(tx);
                let _ = tx.send(Event::Status("Reading cache metadata…".into()));
            }
            continue;
        }
        if load_model {
            load_model = false;
            if load_cancel
                .as_ref()
                .is_some_and(|c| c.load(Ordering::Relaxed))
            {
                load_cancel = None;
                let _ = tx.send(Event::DownloadCancelled);
                continue;
            }
            let _ = tx.send(Event::Status("Loading SigLIP…".into()));
            match Model::load(&model_dir, cpu) {
                Ok(mut m) => {
                    if load_cancel
                        .take()
                        .is_some_and(|c| c.load(Ordering::Relaxed))
                    {
                        let _ = tx.send(Event::DownloadCancelled);
                        continue;
                    }
                    if !state.query.trim().is_empty() && state.mode == SearchMode::Semantic {
                        state.query_vector = m.text(&state.query).ok();
                    }
                    let _ = tx.send(Event::Status("Indexing textures…".into()));
                    let _ = tx.send(Event::Backend(m.backend.clone()));
                    model = Some(m);
                    let _ = tx.send(Event::ModelReady(true));
                    state.results(tx);
                }
                Err(e) => {
                    let _ = tx.send(Event::Error(format!("Cannot load SigLIP: {e:#}")));
                    let _ = tx.send(Event::ModelReady(false));
                }
            }
        }
        if !state.paused
            && let Some(model) = model.as_mut()
            && let Some(i) = state.embed.pop_front()
        {
            since_embedding = 0;
            let result = (|| -> Result<()> {
                let bytes = state
                    .cache
                    .as_ref()
                    .unwrap()
                    .read(&state.items[i].texture)?;
                // Viewer may update this entry while CacheCat is open.
                anyhow::ensure!(
                    Some(&cache::fingerprint(&bytes)) == state.hashes[i].as_ref(),
                    "Texture changed; rescan to index its latest version"
                );
                // Duplicate content may have been indexed since this item was queued.
                let hash = state.hashes[i].as_ref().unwrap();
                let prepared = state.prepared.remove(&i);
                let vector = if let Some(existing) = index.get(hash)? {
                    existing
                } else {
                    let pixels = match prepared {
                        Some(pixels) => pixels,
                        None => model::preprocess(&cache::decode(&bytes)?),
                    };
                    let vector = model.image_prepared(pixels)?;
                    index.put(hash, &vector)?;
                    vector
                };
                state.vectors[i] = Some(vector);
                state.items[i].indexed = true;
                Ok(())
            })();
            if let Err(e) = result {
                state.prepared.remove(&i);
                state.items[i].error = Some(format!("Indexing: {e:#}"));
            }
            let _ = tx.send(Event::Item(state.generation, i, state.items[i].clone()));
            if !state.query.is_empty() {
                state.results(tx);
            }
            if state.embed.len().is_multiple_of(8) {
                state.results(tx);
                let _ = tx.send(Event::Backend(model.backend.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalogue_scan_reuses_embeddings_without_writing_thumbnails() {
        let data = tempfile::tempdir().unwrap();
        let cache = Cache::open(std::path::Path::new("tests/fixtures/texturecache")).unwrap();
        let index = Index::open(&data.path().join("index.sqlite3")).unwrap();
        let vector = model::normalize(vec![1.; 768]).unwrap();
        for texture in &cache.textures {
            index
                .put(&cache::fingerprint(&cache.read(texture).unwrap()), &vector)
                .unwrap();
        }
        let worker = Worker::start(data.path().to_owned(), true, None);
        worker.commands.send(Command::Open(1, cache.root)).unwrap();
        let mut indexed = std::collections::HashSet::new();
        while indexed.len() < 4 {
            match worker.events.recv_timeout(Duration::from_secs(5)).unwrap() {
                Event::Item(1, i, item) => {
                    assert!(item.indexed);
                    assert!(item.dimensions.is_some());
                    assert!(item.error.is_none());
                    indexed.insert(i);
                }
                Event::Error(error) => panic!("{error}"),
                _ => {}
            }
        }
        assert!(!data.path().join("thumbnails").exists());
        assert!(std::fs::read_dir(data.path()).unwrap().all(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("index.sqlite3")
        }));
    }

    #[test]
    fn search_mode_is_explicit_and_results_include_it() {
        let cache = Cache::open(std::path::Path::new("tests/fixtures/texturecache")).unwrap();
        let items = cache
            .textures
            .into_iter()
            .map(|texture| Item {
                texture,
                dimensions: None,
                error: None,
                indexed: false,
            })
            .collect();
        let mut state = State {
            items,
            query: "00000000".into(),
            ..Default::default()
        };
        let (tx, rx) = mpsc::channel();
        state.results(&tx);
        assert!(
            matches!(rx.recv().unwrap(), Event::Results(_, _, SearchMode::Semantic, hits) if hits.is_empty())
        );
        state.mode = SearchMode::Uuid;
        state.results(&tx);
        assert!(
            matches!(rx.recv().unwrap(), Event::Results(_, _, SearchMode::Uuid, hits) if hits.len() == 4)
        );
        state.query = "not-a-uuid".into();
        state.results(&tx);
        assert!(
            matches!(rx.recv().unwrap(), Event::Results(_, _, SearchMode::Uuid, hits) if hits.is_empty())
        );
    }
}
