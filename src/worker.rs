use crate::{
    cache::{self, Cache, Texture},
    index::{self, Index},
    indexing::{Indexers, IndexingMode},
    model,
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
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct Item {
    pub texture: Texture,
    pub dimensions: Option<(u32, u32)>,
    pub error: Option<String>,
    pub indexed: bool,
    pub partial: Option<bool>,
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
    ShowPartial(bool),
    IndexingMode(IndexingMode),
}
pub enum Event {
    Loaded(u64, Vec<Item>),
    Item(u64, usize, Item),
    Results(u64, String, SearchMode, bool, Vec<usize>),
    Status(String),
    Backend(String),
    IndexingMode(IndexingMode, bool),
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
    pub fn start(data: PathBuf, cpu: bool, mode: IndexingMode, legacy: Option<PathBuf>) -> Self {
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
                    run(data, cpu, mode, rx, &events_tx)
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
    hide_partial: bool,
    results_dirty: bool,
    last_results: Option<Instant>,
}
impl State {
    fn flush_results(&mut self, tx: &Sender<Event>) {
        if self.results_dirty
            && self
                .last_results
                .is_none_or(|t| t.elapsed() >= Duration::from_millis(150))
        {
            self.results(tx);
        }
    }

    fn results(&mut self, tx: &Sender<Event>) {
        let include = |i: usize| !self.hide_partial || self.items[i].partial == Some(false);
        let results = if self.mode == SearchMode::Semantic && !self.query.trim().is_empty() {
            self.query_vector.as_ref().map_or_else(Vec::new, |query| {
                index::rank_filtered(&self.vectors, query, 200, include)
            })
        } else if self.mode == SearchMode::Uuid && !self.query.trim().is_empty() {
            let uuid = self.query.trim().to_lowercase().replace('-', "");
            (0..self.items.len())
                .filter(|&i| include(i))
                .filter(|&i| {
                    self.items[i]
                        .texture
                        .id
                        .simple()
                        .encode_lower(&mut [0; 32])
                        .contains(&uuid)
                })
                .collect()
        } else {
            (0..self.items.len()).filter(|&i| include(i)).collect()
        };
        let _ = tx.send(Event::Results(
            self.generation,
            self.query.clone(),
            self.mode,
            !self.hide_partial,
            results,
        ));
        self.results_dirty = false;
        self.last_results = Some(Instant::now());
    }
}
fn run(
    data: PathBuf,
    cpu: bool,
    requested_mode: IndexingMode,
    rx: Receiver<Command>,
    tx: &Sender<Event>,
) -> Result<()> {
    std::fs::create_dir_all(&data)?;
    let index = Index::open(&data.join("index.sqlite3"))?;
    let model_dir = data.join("models").join(model::MODEL_ID);
    let mut gpu_available = !cpu && model::gpu_available();
    let mut indexing_mode = requested_mode.supported(gpu_available);
    let _ = tx.send(Event::IndexingMode(indexing_mode, gpu_available));
    let mut model: Option<Indexers> = None;
    let mut state = State {
        hide_partial: false,
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
        results_dirty: false,
        last_results: None,
    };
    let mut load_model = model::ready(&model_dir);
    let mut load_cancel: Option<Arc<AtomicBool>> = None;
    let mut since_embedding = 0_usize;
    let _ = tx.send(Event::ModelReady(load_model));
    let mut download: Option<(Receiver<Result<bool>>, Arc<AtomicBool>)> = None;
    loop {
        state.flush_results(tx);
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
                    state.results_dirty = false;
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
                                    partial: None,
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
                Command::ShowPartial(show) => {
                    state.hide_partial = !show;
                    state.results(tx);
                }
                Command::IndexingMode(mode) => {
                    indexing_mode = mode.supported(gpu_available);
                    if let Some(model) = &mut model {
                        model.select(indexing_mode);
                        let _ = tx.send(Event::Backend(model.backend()));
                    }
                    let _ = tx.send(Event::IndexingMode(indexing_mode, gpu_available));
                }
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
            if let Some(model) = &model {
                refresh_devices(model, &mut indexing_mode, &mut gpu_available, tx);
            }
            continue;
        }
        // Scan headers and saved embeddings without decoding off-screen previews.
        // Small image decoding happens only when an embedding is actually missing.
        let scanned = state.items.len() - state.scan.len();
        let scan_turn = scanned < 8
            || !load_model
                && (model.is_none()
                    || since_embedding < model.as_ref().map_or(1, Indexers::batch_size)
                    || state.embed.is_empty());
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
                state.items[i].partial = Some(!cache::is_complete(&bytes));
                state.vectors[i] = index.get(&hash)?;
                if state.vectors[i].is_none()
                    && (load_model || model.is_some())
                    && state.prepared.len() < 16
                {
                    let image = cache::decode_preview(&bytes)?;
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
            state.results_dirty = true;
            if state.scan.len().is_multiple_of(16) {
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
            match Indexers::load(&model_dir, indexing_mode, gpu_available) {
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
                    gpu_available = m.gpu_available();
                    indexing_mode = m.mode;
                    let _ = tx.send(Event::IndexingMode(indexing_mode, gpu_available));
                    let _ = tx.send(Event::Backend(m.backend()));
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
            && !state.embed.is_empty()
        {
            // A model downloaded after the catalogue scan has no prepared
            // inputs yet. Prepare one per loop so search/pause/device commands
            // are serviced between decodes, rather than after a whole batch.
            if let Some(i) = state
                .embed
                .iter()
                .take(model.batch_size())
                .copied()
                .find(|i| !state.prepared.contains_key(i))
            {
                let result = (|| -> Result<Option<Vec<f32>>> {
                    let hash = state.hashes[i].as_ref().unwrap();
                    if let Some(existing) = index.get(hash)? {
                        return Ok(Some(existing));
                    }
                    let bytes = state
                        .cache
                        .as_ref()
                        .unwrap()
                        .read(&state.items[i].texture)?;
                    anyhow::ensure!(
                        cache::fingerprint(&bytes) == *hash,
                        "Texture changed; rescan to index its latest version"
                    );
                    state
                        .prepared
                        .insert(i, model::preprocess(&cache::decode_preview(&bytes)?));
                    Ok(None)
                })();
                match result {
                    Ok(None) => {}
                    Ok(Some(vector)) => {
                        state.vectors[i] = Some(vector);
                        state.items[i].indexed = true;
                        state.items[i].error = None;
                        if let Some(position) = state.embed.iter().position(|&queued| queued == i) {
                            state.embed.remove(position);
                        }
                    }
                    Err(error) => {
                        state.items[i].error = Some(format!("Indexing: {error:#}"));
                        if let Some(position) = state.embed.iter().position(|&queued| queued == i) {
                            state.embed.remove(position);
                        }
                    }
                }
                let _ = tx.send(Event::Item(state.generation, i, state.items[i].clone()));
                state.results_dirty = true;
                continue;
            }
            since_embedding = 0;
            let batch: Vec<_> = (0..model.batch_size())
                .filter_map(|_| state.embed.pop_front())
                .collect();
            let mut ready = Vec::new();
            let mut jobs = Vec::new();
            let mut inputs = Vec::new();
            for i in batch {
                let prepared = state.prepared.remove(&i);
                let result = (|| -> Result<(bool, Vec<f32>)> {
                    let hash = state.hashes[i].as_ref().unwrap();
                    // Prepared pixels and their hash came from the same read.
                    // Reopening the file here wastes I/O and cannot improve
                    // that snapshot's consistency.
                    if let Some(pixels) = prepared {
                        return match index.get(hash)? {
                            Some(existing) => Ok((true, existing)),
                            None => Ok((false, pixels)),
                        };
                    }
                    anyhow::bail!("Missing prepared image")
                })();
                match result {
                    Ok((true, vector)) => ready.push((i, Ok(vector))),
                    Ok((false, pixels)) => {
                        jobs.push(i);
                        inputs.push(pixels);
                    }
                    Err(error) => ready.push((i, Err(error))),
                }
            }
            for (i, result) in jobs.into_iter().zip(model.images(inputs)) {
                ready.push((
                    i,
                    result.and_then(|vector| {
                        index.put(state.hashes[i].as_ref().unwrap(), &vector)?;
                        Ok(vector)
                    }),
                ));
            }
            for (i, result) in ready {
                match result {
                    Ok(vector) => {
                        state.vectors[i] = Some(vector);
                        state.items[i].indexed = true;
                        state.items[i].error = None;
                    }
                    Err(error) => state.items[i].error = Some(format!("Indexing: {error:#}")),
                }
                let _ = tx.send(Event::Item(state.generation, i, state.items[i].clone()));
            }
            refresh_devices(model, &mut indexing_mode, &mut gpu_available, tx);
            state.results_dirty = true;
            if state.embed.len().is_multiple_of(8) {
                let _ = tx.send(Event::Backend(model.backend()));
            }
        }
    }
}

fn refresh_devices(model: &Indexers, mode: &mut IndexingMode, gpu: &mut bool, tx: &Sender<Event>) {
    if *mode != model.mode || *gpu != model.gpu_available() {
        *mode = model.mode;
        *gpu = model.gpu_available();
        let _ = tx.send(Event::IndexingMode(*mode, *gpu));
        let _ = tx.send(Event::Backend(model.backend()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn background_results_coalesce_but_search_and_final_updates_are_delivered() {
        let mut state = State::default();
        let (tx, rx) = mpsc::channel();
        state.results(&tx);
        rx.recv().unwrap();
        for _ in 0..100 {
            state.results_dirty = true;
            state.flush_results(&tx);
        }
        assert!(rx.try_recv().is_err());
        state.query = "new search".into();
        state.results(&tx);
        assert!(matches!(rx.recv().unwrap(), Event::Results(_,q,_,_,_) if q=="new search"));
        state.results_dirty = true;
        state.last_results = Some(Instant::now() - Duration::from_secs(1));
        state.flush_results(&tx);
        rx.recv().unwrap();
        state.flush_results(&tx);
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn cpu_only_worker_rejects_unavailable_device_selection() {
        let data = tempfile::tempdir().unwrap();
        let worker = Worker::start(data.path().to_owned(), true, IndexingMode::Gpu, None);
        assert!(matches!(
            worker.events.recv_timeout(Duration::from_secs(5)).unwrap(),
            Event::IndexingMode(IndexingMode::Cpu, false)
        ));
        for requested in IndexingMode::ALL {
            worker
                .commands
                .send(Command::IndexingMode(requested))
                .unwrap();
            loop {
                if let Event::IndexingMode(mode, available) =
                    worker.events.recv_timeout(Duration::from_secs(5)).unwrap()
                {
                    assert_eq!(mode, IndexingMode::Cpu);
                    assert!(!available);
                    break;
                }
            }
        }
    }

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
        let worker = Worker::start(data.path().to_owned(), true, IndexingMode::Cpu, None);
        worker.commands.send(Command::Open(1, cache.root)).unwrap();
        let mut indexed = std::collections::HashSet::new();
        while indexed.len() < 4 {
            match worker.events.recv_timeout(Duration::from_secs(5)).unwrap() {
                Event::Item(1, i, item) => {
                    assert!(item.indexed);
                    assert!(item.dimensions.is_some());
                    assert_eq!(item.partial, Some(false));
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
                partial: None,
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
            matches!(rx.recv().unwrap(), Event::Results(_, _, SearchMode::Semantic, true, hits) if hits.is_empty())
        );
        state.mode = SearchMode::Uuid;
        state.results(&tx);
        assert!(
            matches!(rx.recv().unwrap(), Event::Results(_, _, SearchMode::Uuid, true, hits) if hits.len() == 4)
        );
        state.query = "not-a-uuid".into();
        state.results(&tx);
        assert!(
            matches!(rx.recv().unwrap(), Event::Results(_, _, SearchMode::Uuid, true, hits) if hits.is_empty())
        );
    }

    #[test]
    fn partial_filter_applies_before_semantic_limit_and_to_uuid_and_browsing() {
        let cache = Cache::open(std::path::Path::new("tests/fixtures/texturecache")).unwrap();
        let items: Vec<_> = (0..253)
            .map(|i| {
                let mut texture = cache.textures[0].clone();
                texture.id = uuid::Uuid::from_u128(i as u128 + 1);
                Item {
                    texture,
                    dimensions: Some((128, 128)),
                    error: None,
                    indexed: true,
                    partial: if i == 252 { None } else { Some(i < 250) },
                }
            })
            .collect();
        let mut state = State {
            items,
            hide_partial: true,
            ..Default::default()
        };
        let (tx, rx) = mpsc::channel();
        for mode in [SearchMode::Semantic, SearchMode::Uuid] {
            state.mode = mode;
            state.results(&tx);
            assert!(
                matches!(rx.recv().unwrap(),Event::Results(_,_,_,false,hits) if hits==[250,251])
            );
        }
        state.mode = SearchMode::Semantic;
        state.query = "texture".into();
        state.query_vector = Some(vec![1.]);
        state.vectors = (0..253)
            .map(|i| Some(vec![if i < 250 { 1. } else { 0.5 }]))
            .collect();
        state.results(&tx);
        assert!(matches!(rx.recv().unwrap(),Event::Results(_,_,_,false,hits) if hits==[250,251]));
        state.hide_partial = false;
        state.results(&tx);
        assert!(
            matches!(rx.recv().unwrap(),Event::Results(_,_,_,true,hits) if hits.len()==200 && hits[0]==0)
        );
        state.mode = SearchMode::Uuid;
        state.query = "00000000".into();
        state.results(&tx);
        assert!(matches!(rx.recv().unwrap(),Event::Results(_,_,_,true,hits) if hits.len()==253));
        state.hide_partial = true;
        state.results(&tx);
        assert!(matches!(rx.recv().unwrap(),Event::Results(_,_,_,false,hits) if hits==[250,251]));
    }
}
