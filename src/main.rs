#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod ui;
use anyhow::Result;
use cachecat::{
    cache::{self, Cache},
    model::{self, Model},
    storage,
};
use clap::Parser;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(version, about)]
struct Args {
    #[arg(long, hide = true)]
    inference_worker: bool,
    /// Viewer cache folder (or texturecache subfolder).
    #[arg(long)]
    cache: Option<PathBuf>,
    /// Override portable storage (defaults to data beside the executable).
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// Disable inference acceleration.
    #[arg(long)]
    cpu: bool,
    /// Read and decode N textures without opening a window.
    #[arg(long)]
    inspect: Option<usize>,
    /// Download and verify the pinned model, without opening a window.
    #[arg(long)]
    download_model: bool,
    /// Run a real semantic search over the first N decoded textures (use --inspect N).
    #[arg(long)]
    query: Option<String>,
}
fn main() -> Result<()> {
    let args = Args::parse();
    if args.inference_worker {
        return model::inference_worker(
            args.data_dir
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Missing model directory"))?,
            args.cpu,
        );
    }
    let portable = args.data_dir.is_none();
    let data = match args.data_dir {
        Some(path) => path,
        None => storage::portable_data_dir(&std::env::current_exe()?)?,
    };
    let path = args.cache;
    if args.download_model {
        model::download(&data.join("models").join(model::MODEL_ID), |s| {
            println!("{s}")
        })?;
        return Ok(());
    }
    if let Some(limit) = args.inspect {
        let cache = Cache::open(
            &path
                .or_else(cache::default_path)
                .ok_or_else(|| anyhow::anyhow!("Pass --cache PATH"))?,
        )?;
        println!("{} live textures, newest first", cache.textures.len());
        let loading_started = Instant::now();
        let mut model = if args.query.is_some() {
            Some(Model::load(
                &data.join("models").join(model::MODEL_ID),
                args.cpu,
            )?)
        } else {
            None
        };
        let query = if let Some(m) = model.as_mut() {
            println!("Inference: {}", m.backend);
            Some(m.text(args.query.as_ref().unwrap())?)
        } else {
            None
        };
        let loading_time = loading_started.elapsed();
        let mut decoding_time = Duration::ZERO;
        let mut embedding_time = Duration::ZERO;
        let mut decoded = 0;
        let mut failed = 0;
        let mut results = vec![];
        for texture in cache.textures.iter().take(limit) {
            let started = Instant::now();
            let image = cache.read(texture).and_then(|b| cache::decode(&b));
            decoding_time += started.elapsed();
            match image {
                Ok(image) => {
                    decoded += 1;
                    if let Some(m) = model.as_mut() {
                        let started = Instant::now();
                        let vector = m.image(&image)?;
                        embedding_time += started.elapsed();
                        results.push((
                            cachecat::index::cosine(&vector, query.as_ref().unwrap()),
                            texture.id,
                        ));
                    }
                }
                Err(_) => failed += 1,
            }
        }
        results.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (score, id) in results.iter().take(10) {
            println!("{score:.4} {id}");
        }
        println!(
            "Timing: model/query {:.3}s; read/decode {:.3}s; preprocess/inference {:.3}s",
            loading_time.as_secs_f64(),
            decoding_time.as_secs_f64(),
            embedding_time.as_secs_f64()
        );
        println!("Decoded {decoded}; unavailable {failed}");
        anyhow::ensure!(
            decoded > 0 || limit == 0 || cache.textures.is_empty(),
            "No textures decoded"
        );
        return Ok(());
    }
    let legacy = if portable {
        storage::import_source(&data)
    } else {
        None
    };
    ui::run(path, data, args.cpu, legacy);
    Ok(())
}
