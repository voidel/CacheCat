//! Exercise the real helper protocol/recovery with a deterministic fake driver.
use anyhow::{Result, ensure};
use cachecat::{
    indexing::{Indexers, IndexingMode},
    model::Model,
};
use std::{
    io::{Read, Write},
    path::Path,
};

const PIXELS: usize = 3 * 224 * 224;

fn emit(backend: &str, vector: Option<Vec<f32>>, error: Option<&str>) -> Result<()> {
    let mut out = std::io::stdout().lock();
    serde_json::to_writer(
        &mut out,
        &serde_json::json!({"backend":backend,"vector":vector,"error":error}),
    )?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

fn helper(scenario: &str, cpu: bool) -> Result<()> {
    let backend = if cpu { "CPU" } else { "Test GPU" };
    emit(backend, None, None)?;
    let mut input = std::io::stdin().lock();
    loop {
        let mut tag = [0];
        if input.read(&mut tag)? == 0 {
            return Ok(());
        }
        let mut size = [0; 4];
        input.read_exact(&mut size)?;
        let size = u32::from_le_bytes(size) as usize;
        ensure!(size <= 8 * PIXELS * 4);
        let mut bytes = vec![0; size];
        input.read_exact(&mut bytes)?;
        if tag[0] == 2 {
            let mut v = vec![0.; 768];
            v[0] = 1.;
            emit(backend, Some(v), None)?;
            continue;
        }
        ensure!(tag[0] == 1 && size.is_multiple_of(PIXELS * 4));
        let batch = size / (PIXELS * 4);
        if (scenario == "reject-batch" || scenario == "crash-batch") && batch > 2 {
            if scenario == "crash-batch" {
                std::process::exit(42);
            }
            emit(backend, None, Some("Batch exceeds device memory"))?;
            continue;
        }
        let ids: Vec<_> = bytes
            .as_chunks::<{ PIXELS * 4 }>()
            .0
            .iter()
            .map(|image| f32::from_le_bytes(image[..4].try_into().unwrap()) as usize)
            .collect();
        if (scenario == "bad-image" && ids.contains(&2)) || (scenario == "gpu-fails" && !cpu) {
            emit(backend, None, Some("Inference failed"))?;
            continue;
        }
        let mut vectors = vec![0.; batch * 768];
        for (i, id) in ids.into_iter().enumerate() {
            vectors[i * 768 + id] = 1.;
        }
        emit(backend, Some(vectors), None)?;
    }
}

fn inputs(count: usize) -> Vec<Vec<f32>> {
    (0..count).map(|i| vec![i as f32; PIXELS]).collect()
}

fn check_vectors(vectors: &[Vec<f32>], count: usize) {
    assert_eq!(vectors.len(), count);
    for (i, v) in vectors.iter().enumerate() {
        assert_eq!(v.len(), 768);
        assert_eq!(v[i], 1.);
        assert_eq!(v.iter().sum::<f32>(), 1.);
    }
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.iter().any(|a| a == "--inference-worker") {
        let i = args.iter().position(|a| a == "--data-dir").unwrap();
        return helper(&args[i + 1], args.iter().any(|a| a == "--cpu"));
    }
    for scenario in ["normal", "reject-batch", "crash-batch", "gpu-fails"] {
        let mut model = Model::load(Path::new(scenario), false)?;
        assert_eq!(model.batch_size(), 8);
        check_vectors(&model.images_prepared(&inputs(8))?, 8);
        match scenario {
            "reject-batch" | "crash-batch" => {
                assert!(model.accelerated());
                assert_eq!(model.batch_size(), 2);
            }
            "gpu-fails" => assert!(!model.accelerated()),
            _ => {}
        }
        check_vectors(&model.images_prepared(&inputs(1))?, 1);
        println!("{scenario}: ordering, recovery and final partial batch passed");
    }
    let mut pool = Indexers::load(Path::new("bad-image"), IndexingMode::Gpu, true)?;
    let results = pool.images(inputs(8));
    assert_eq!(results.len(), 8);
    for (i, result) in results.into_iter().enumerate() {
        if i == 2 {
            assert!(result.is_err());
        } else {
            assert_eq!(result?[i], 1.);
        }
    }
    assert_eq!(pool.mode, IndexingMode::Cpu);
    println!("bad-image: other images preserved and GPU failure selects CPU");
    let mut cpu = Model::load(Path::new("normal"), true)?;
    assert_eq!(cpu.batch_size(), 4);
    check_vectors(&cpu.images_prepared(&inputs(4))?, 4);
    assert!(cpu.images_prepared(&inputs(5)).is_err());
    Ok(())
}
