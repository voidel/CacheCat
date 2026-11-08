use crate::{
    cache::{self, Cache, Texture},
    index::{self, Index},
    model::{self, Model},
};
use anyhow::Result;
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct Item {
    pub texture: Texture,
    pub thumbnail: Option<PathBuf>,
    pub dimensions: Option<(u32, u32)>,
    pub error: Option<String>,
    pub indexed: bool,
}
pub enum Command {
    Open(u64, PathBuf),
    Search(u64, String),
    Download,
    Pause(bool),
}
pub enum Event {
    Loaded(u64, Vec<Item>),
    Item(u64, usize, Item),
    Results(u64, String, Vec<usize>),
    Status(String),
    Backend(String),
    Error(String),
    ModelReady(bool),
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
struct State {
    generation: u64,
    cache: Option<Cache>,
    items: Vec<Item>,
    hashes: Vec<Option<String>>,
    vectors: Vec<Option<Vec<f32>>>,
    decode: VecDeque<usize>,
    embed: VecDeque<usize>,
    prepared: HashMap<usize, Vec<f32>>,
    query: String,
    query_vector: Option<Vec<f32>>,
    paused: bool,
}
impl State {
    fn results(&self, tx: &Sender<Event>) {
        let mut results: Vec<usize> = (0..self.items.len()).collect();
        if let Some(uuid) = index::uuid_query(&self.query) {
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
        let _ = tx.send(Event::Results(self.generation, self.query.clone(), results));
    }
}
fn run(data: PathBuf, cpu: bool, rx: Receiver<Command>, tx: &Sender<Event>) -> Result<()> {
    std::fs::create_dir_all(data.join("thumbnails"))?;
    let index = Index::open(&data.join("index.sqlite3"))?;
    let model_dir = data.join("models").join(model::MODEL_ID);
    let mut model: Option<Model> = None;
    let mut state = State {
        generation: 0,
        cache: None,
        items: vec![],
        hashes: vec![],
        vectors: vec![],
        decode: VecDeque::new(),
        embed: VecDeque::new(),
        prepared: HashMap::new(),
        query: String::new(),
        query_vector: None,
        paused: false,
    };
    let mut load_model = model::ready(&model_dir);
    let mut since_embedding = 0_usize;
    let _ = tx.send(Event::ModelReady(load_model));
    loop {
        let pending = !state.paused
            && (!state.decode.is_empty() || model.is_some() && !state.embed.is_empty());
        let command = match rx.recv_timeout(if pending || load_model {
            Duration::ZERO
        } else {
            Duration::from_millis(150)
        }) {
            Ok(c) => Some(c),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            Err(_) => None,
        };
        if let Some(command) = command {
            match command {
                Command::Open(generation, path) => {
                    state.generation = generation;
                    // Drop previous work even when the new folder cannot be read.
                    state.cache = None;
                    state.items.clear();
                    state.decode.clear();
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
                                    thumbnail: None,
                                    dimensions: None,
                                    error: None,
                                    indexed: false,
                                })
                                .collect();
                            let n = state.items.len();
                            state.decode = (0..n).collect();
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
                Command::Search(generation, query) => {
                    if generation != state.generation {
                        continue;
                    }
                    state.query = query;
                    state.query_vector = None;
                    if !state.query.trim().is_empty()
                        && index::uuid_query(&state.query).is_none()
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
                Command::Download => {
                    match model::download(&model_dir, |s| {
                        let _ = tx.send(Event::Status(s));
                    }) {
                        Ok(()) => load_model = true,
                        Err(e) => {
                            let _ = tx.send(Event::Error(format!("Model download failed: {e:#}")));
                            let _ = tx.send(Event::ModelReady(false));
                        }
                    }
                }
            }
            continue;
        }
        // Show an initial preview batch, then keep a bounded preparation queue.
        // This reuses decoded pixels without retaining full-resolution textures.
        let decoded = state.items.len() - state.decode.len();
        let preview_turn = decoded < 8
            || !load_model && (model.is_none() || since_embedding < 1 || state.embed.is_empty());
        if !state.paused
            && preview_turn
            && let Some(i) = state.decode.pop_front()
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
                let thumb = data.join("thumbnails").join(format!("{hash}.png"));
                state.vectors[i] = index.get(&hash)?;
                if !thumb.is_file() {
                    let image = cache::decode(&bytes)?;
                    state.items[i].dimensions = Some((image.width(), image.height()));
                    image.thumbnail(256, 256).save(&thumb)?;
                    if state.vectors[i].is_none()
                        && (load_model || model.is_some())
                        && state.prepared.len() < 16
                    {
                        state.prepared.insert(i, model::preprocess(&image));
                    }
                }
                state.items[i].thumbnail = Some(thumb);
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
            if state.decode.len().is_multiple_of(16) {
                state.results(tx);
                let _ = tx.send(Event::Status("Loading previews…".into()));
            }
            continue;
        }
        if load_model {
            load_model = false;
            let _ = tx.send(Event::Status("Loading SigLIP…".into()));
            match Model::load(&model_dir, cpu) {
                Ok(mut m) => {
                    if !state.query.trim().is_empty() && index::uuid_query(&state.query).is_none() {
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
                // Duplicate content may have been indexed since this preview was queued.
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
