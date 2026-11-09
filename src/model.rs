use anyhow::{Context, Result, ensure};
use image::{DynamicImage, imageops::FilterType};
use ort::{session::Session, value::Tensor};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};

pub const MODEL_ID: &str = "siglip-base-patch16-224-q8-4649052-preprocess-v1";
pub const MODEL_SOURCE: &str = "https://huggingface.co/Xenova/siglip-base-patch16-224/tree/4649052661e53c7000355844105f8a1792088239";
const BASE: &str = "https://huggingface.co/Xenova/siglip-base-patch16-224/resolve/4649052661e53c7000355844105f8a1792088239";
const FILES: &[(&str, &str, u64)] = &[
    (
        "onnx/vision_model_quantized.onnx",
        "ef14a954f3d57e1806666432bd9785004c1dc27100aa260eee0cb0f10a5de058",
        99_499_129,
    ),
    (
        "onnx/text_model_quantized.onnx",
        "ad0329b1f35acc66d8953ff2559ce358da8eb0a7011794cf951523d63a4dbce2",
        111_475_220,
    ),
    (
        "tokenizer.json",
        "4a17c975210be5ab4c36b47d8dae4eefb866dbfb1e676e394aad85dc30a3ae08",
        2_398_744,
    ),
];

pub fn ready(dir: &Path) -> bool {
    FILES
        .iter()
        .all(|(name, _, size)| dir.join(name).metadata().is_ok_and(|m| m.len() == *size))
}

/// Downloads only public model assets; no cache data or queries leave the machine.
pub fn download(dir: &Path, mut progress: impl FnMut(String)) -> Result<()> {
    download_cancellable(dir, &AtomicBool::new(false), |done, total| {
        progress(format!(
            "Downloading model: {} / {} MB",
            done / 1_000_000,
            total / 1_000_000
        ));
    })?;
    Ok(())
}
struct PartialFile(PathBuf);
impl Drop for PartialFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
/// Returns false on cancellation; completed verified files are kept.
pub fn download_cancellable(
    dir: &Path,
    cancelled: &AtomicBool,
    progress: impl FnMut(u64, u64),
) -> Result<bool> {
    download_from(dir, cancelled, progress, BASE, FILES)
}
fn download_from(
    dir: &Path,
    cancelled: &AtomicBool,
    mut progress: impl FnMut(u64, u64),
    base: &str,
    files: &[(&str, &str, u64)],
) -> Result<bool> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let transfer = async {
                let total_size: u64 = files.iter().map(|f| f.2).sum();
                let mut completed = 0;
                progress(0, total_size);
                let client = reqwest::Client::builder()
                    .connect_timeout(Duration::from_secs(30))
                    .timeout(Duration::from_secs(900))
                    .build()?;
                for &(name, hash, expected_size) in files {
                    if cancelled.load(Ordering::Relaxed) {
                        return Ok(false);
                    }
                    let dest = dir.join(name);
                    if verify(&dest, hash, expected_size).is_ok() {
                        completed += expected_size;
                        progress(completed, total_size);
                        continue;
                    }
                    fs::create_dir_all(dest.parent().unwrap())?;
                    let partial = PartialFile(dest.with_extension("partial"));
                    let result: Result<()> = async {
                        let mut response = client
                            .get(format!("{base}/{name}"))
                            .send()
                            .await?
                            .error_for_status()?;
                        let mut output = File::create(&partial.0)?;
                        let mut total = 0_u64;
                        let mut last = 0_u64;
                        while let Some(bytes) = response.chunk().await? {
                            ensure!(!cancelled.load(Ordering::Relaxed), "Download cancelled");
                            total += bytes.len() as u64;
                            ensure!(total <= expected_size, "Unexpected model file size");
                            output.write_all(&bytes)?;
                            if total / 100_000 != last {
                                last = total / 100_000;
                                progress(completed + total, total_size);
                            }
                        }
                        output.sync_all()?;
                        drop(output);
                        verify(&partial.0, hash, expected_size)?;
                        ensure!(!cancelled.load(Ordering::Relaxed), "Download cancelled");
                        if dest.exists() {
                            fs::remove_file(&dest)?;
                        }
                        fs::rename(&partial.0, &dest)?;
                        Ok(())
                    }
                    .await;
                    if cancelled.load(Ordering::Relaxed) {
                        return Ok(false);
                    }
                    result?;
                    completed += expected_size;
                    progress(completed, total_size);
                }
                Ok(true)
            };
            tokio::select! {
                biased;
                _ = async {
                    while !cancelled.load(Ordering::Relaxed) {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                } => Ok(false),
                result = transfer => result,
            }
        })
}

fn verify(path: &Path, hash: &str, size: u64) -> Result<()> {
    let mut f = File::open(path)?;
    ensure!(f.metadata()?.len() == size, "Model file size mismatch");
    if !hash.is_empty() {
        let mut digest = Sha256::new();
        let mut buffer = [0; 128 * 1024];
        loop {
            let n = f.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            digest.update(&buffer[..n]);
        }
        ensure!(
            format!("{:x}", digest.finalize()) == hash,
            "Model checksum mismatch"
        );
    }
    Ok(())
}

struct Engine {
    vision: Session,
    text: Session,
    tokenizer: Tokenizer,
    dir: PathBuf,
    pub backend: String,
    accelerated: bool,
}

fn session_builder(gpu: bool) -> Result<ort::session::builder::SessionBuilder> {
    let threads = std::env::var("CACHECAT_CPU_THREADS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&v| (1..=16).contains(&v))
        .unwrap_or_else(|| {
            std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).clamp(1, 4))
        });
    let builder = Session::builder()?
        .with_intra_threads(if gpu { 2 } else { threads })?
        .with_parallel_execution(false)?
        .with_memory_pattern(!gpu)?
        .with_config_entry("session.intra_op.allow_spinning", "0")?;
    #[cfg(target_os = "windows")]
    let builder = if gpu {
        // Extended graph optimizations crash the bundled DirectML runtime while
        // loading the quantized SigLIP models. Basic optimizations retain GPU
        // execution and work for both the vision and text graphs.
        builder
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level1)?
            .with_execution_providers([
                ort::execution_providers::DirectMLExecutionProvider::default()
                    .build()
                    .error_on_failure(),
            ])?
    } else {
        builder
    };
    #[cfg(target_os = "macos")]
    let builder = if gpu {
        builder.with_execution_providers([
            ort::execution_providers::CoreMLExecutionProvider::default()
                .build()
                .error_on_failure(),
        ])?
    } else {
        builder
    };
    #[cfg(all(target_os = "linux", feature = "cuda"))]
    let builder = if gpu {
        builder.with_execution_providers([
            ort::execution_providers::CUDAExecutionProvider::default()
                .build()
                .error_on_failure(),
        ])?
    } else {
        builder
    };
    let _ = gpu;
    Ok(builder)
}

fn session(path: &Path, gpu: bool) -> Result<Session> {
    Ok(session_builder(gpu)?.commit_from_file(path)?)
}

/// Runs only in the isolated hardware-probe process, before models are downloaded.
pub fn probe_gpu() -> Result<()> {
    #[cfg(target_os = "windows")]
    unsafe {
        windows_sys::Win32::System::Diagnostics::Debug::SetErrorMode(0x0001 | 0x0002);
    }
    ensure!(
        cfg!(any(
            target_os = "windows",
            target_os = "macos",
            all(target_os = "linux", feature = "cuda")
        )),
        "No GPU provider in this build"
    );
    session_builder(true)?;
    Ok(())
}

pub fn gpu_available() -> bool {
    let probe = (|| -> Result<bool> {
        use std::process::{Command, Stdio};
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("--probe-gpu")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn()?;
        let start = std::time::Instant::now();
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(status.success());
            }
            if start.elapsed() > Duration::from_secs(10) {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    })();
    probe.unwrap_or(false)
}

impl Engine {
    pub fn load(dir: &Path, force_cpu: bool) -> Result<Self> {
        for &(file, hash, size) in FILES {
            verify(&dir.join(file), hash, size)
                .context("Model missing or damaged; download it again")?;
        }
        let gpu = !force_cpu
            && cfg!(any(
                target_os = "windows",
                target_os = "macos",
                all(target_os = "linux", feature = "cuda")
            ));
        let make = |gpu| -> Result<(Session, Session)> {
            Ok((
                session(&dir.join(FILES[0].0), gpu)?,
                session(&dir.join(FILES[1].0), gpu)?,
            ))
        };
        let (vision, text, accelerated) = match make(gpu) {
            Ok((v, t)) => (v, t, gpu),
            Err(_) if gpu => {
                let (v, t) = make(false)?;
                (v, t, false)
            }
            Err(e) => return Err(e),
        };
        let mut tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: 64,
                ..Default::default()
            }))
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::Fixed(64),
            pad_id: 1,
            pad_token: "</s>".into(),
            ..Default::default()
        }));
        let backend = if accelerated {
            if cfg!(target_os = "windows") {
                "DirectML + CPU"
            } else if cfg!(target_os = "macos") {
                "Core ML + CPU"
            } else {
                "CUDA + CPU"
            }
        } else {
            "CPU"
        }
        .into();
        Ok(Self {
            vision,
            text,
            tokenizer,
            dir: dir.to_path_buf(),
            backend,
            accelerated,
        })
    }

    fn fallback(&mut self) -> Result<()> {
        self.vision = session(&self.dir.join(FILES[0].0), false)?;
        self.text = session(&self.dir.join(FILES[1].0), false)?;
        self.accelerated = false;
        self.backend = "CPU (GPU fallback)".into();
        Ok(())
    }

    fn image_inner(&mut self, pixels: Vec<f32>) -> Result<Vec<f32>> {
        let input = Tensor::from_array(([1, 3, 224, 224], pixels))?;
        let outputs = self.vision.run(ort::inputs!["pixel_values"=>input])?;
        normalize(
            outputs["pooler_output"]
                .try_extract_tensor::<f32>()?
                .1
                .to_vec(),
        )
    }
    pub fn text(&mut self, query: &str) -> Result<Vec<f32>> {
        let text = query
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        let encoding = self
            .tokenizer
            .encode(format!("This is a photo of {text}."), true)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let ids: Vec<i64> = encoding.get_ids().iter().map(|&i| i as i64).collect();
        match self.text_inner(ids.clone()) {
            Ok(v) => Ok(v),
            Err(_) if self.accelerated => {
                self.fallback()?;
                self.text_inner(ids)
            }
            Err(e) => Err(e),
        }
    }
    fn text_inner(&mut self, ids: Vec<i64>) -> Result<Vec<f32>> {
        let outputs = self
            .text
            .run(ort::inputs!["input_ids"=>Tensor::from_array(([1,64],ids))?])?;
        normalize(
            outputs["pooler_output"]
                .try_extract_tensor::<f32>()?
                .1
                .to_vec(),
        )
    }
}

/// Native drivers run out-of-process: an access violation must not take down the UI.
pub struct Model {
    child: std::process::Child,
    output: std::io::BufReader<std::process::ChildStdout>,
    dir: PathBuf,
    cpu: bool,
    pub backend: String,
}
enum Request {
    Image(Vec<f32>),
    Text(String),
}
const IMAGE_FLOATS: usize = 3 * 224 * 224;

// One bounded binary frame per request. Writing JSON directly to ChildStdin
// previously issued a pipe write for practically every pixel value and comma.
fn write_request(mut writer: impl Write, request: &Request) -> Result<()> {
    let mut frame = Vec::new();
    match request {
        Request::Image(pixels) => {
            ensure!(pixels.len() == IMAGE_FLOATS, "Invalid image tensor size");
            frame.reserve(5 + IMAGE_FLOATS * 4);
            frame.push(1);
            frame.extend((IMAGE_FLOATS as u32 * 4).to_le_bytes());
            frame.extend(pixels.iter().flat_map(|v| v.to_le_bytes()));
        }
        Request::Text(text) => {
            ensure!(text.len() <= 1024 * 1024, "Search text is too long");
            frame.push(2);
            frame.extend((text.len() as u32).to_le_bytes());
            frame.extend(text.as_bytes());
        }
    }
    writer.write_all(&frame)?;
    writer.flush()?;
    Ok(())
}

fn read_request(mut reader: impl Read) -> Result<Option<Request>> {
    let mut tag = [0];
    if reader.read(&mut tag)? == 0 {
        return Ok(None);
    }
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    ensure!(
        (tag[0] == 1 && length == IMAGE_FLOATS * 4) || (tag[0] == 2 && length <= 1024 * 1024),
        "Invalid inference request frame"
    );
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    Ok(Some(if tag[0] == 1 {
        Request::Image(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|v| f32::from_le_bytes(*v))
                .collect(),
        )
    } else {
        Request::Text(String::from_utf8(bytes)?)
    }))
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Response {
    backend: String,
    vector: Option<Vec<f32>>,
    error: Option<String>,
}
impl Model {
    pub fn accelerated(&self) -> bool {
        !self.backend.starts_with("CPU")
    }

    pub fn load(dir: &Path, force_cpu: bool) -> Result<Self> {
        match Self::spawn(dir, force_cpu) {
            Ok(m) => Ok(m),
            Err(_) if !force_cpu => {
                let mut m = Self::spawn(dir, true)?;
                m.backend = "CPU (GPU startup fallback)".into();
                Ok(m)
            }
            Err(e) => Err(e),
        }
    }
    fn spawn(dir: &Path, cpu: bool) -> Result<Self> {
        use std::process::{Command, Stdio};
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("--inference-worker")
            .arg("--data-dir")
            .arg(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if cpu {
            command.arg("--cpu");
        }
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn()?;
        let output = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut model = Self {
            child,
            output,
            dir: dir.to_path_buf(),
            cpu,
            backend: String::new(),
        };
        let response = model.read()?;
        if let Some(e) = response.error {
            anyhow::bail!(e);
        }
        model.backend = response.backend;
        Ok(model)
    }
    fn read(&mut self) -> Result<Response> {
        use std::io::BufRead;
        let mut line = String::new();
        ensure!(
            self.output.read_line(&mut line)? > 0,
            "Inference helper exited unexpectedly"
        );
        Ok(serde_json::from_str(&line)?)
    }
    fn request(&mut self, request: &Request) -> Result<Vec<f32>> {
        let result = (|| -> Result<Vec<f32>> {
            let input = self.child.stdin.as_mut().context("Inference pipe closed")?;
            write_request(input, request)?;
            let response = self.read()?;
            if let Some(error) = response.error {
                anyhow::bail!(error);
            }
            self.backend = response.backend;
            response.vector.context("No embedding returned")
        })();
        match result {
            Err(_) if !self.cpu => {
                let dir = self.dir.clone();
                *self = Self::spawn(&dir, true)?;
                let result = self.request(request);
                self.backend = "CPU (GPU fallback)".into();
                result
            }
            other => other,
        }
    }
    pub fn image(&mut self, image: &DynamicImage) -> Result<Vec<f32>> {
        self.image_prepared(preprocess(image))
    }
    pub fn image_prepared(&mut self, pixels: Vec<f32>) -> Result<Vec<f32>> {
        self.request(&Request::Image(pixels))
    }
    pub fn text(&mut self, query: &str) -> Result<Vec<f32>> {
        self.request(&Request::Text(query.to_owned()))
    }
}
impl Drop for Model {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn inference_worker(dir: &Path, cpu: bool) -> Result<()> {
    #[cfg(target_os = "windows")]
    // Only this isolated helper suppresses OS crash dialogs. The parent observes EOF.
    unsafe {
        windows_sys::Win32::System::Diagnostics::Debug::SetErrorMode(0x0001 | 0x0002);
    }
    let mut stdout = std::io::BufWriter::new(std::io::stdout().lock());
    let mut emit = |response: Response| -> Result<()> {
        serde_json::to_writer(&mut stdout, &response)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
        Ok(())
    };
    let mut engine = match Engine::load(dir, cpu) {
        Ok(e) => e,
        Err(e) => {
            emit(Response {
                backend: "CPU".into(),
                vector: None,
                error: Some(format!("{e:#}")),
            })?;
            return Ok(());
        }
    };
    emit(Response {
        backend: engine.backend.clone(),
        vector: None,
        error: None,
    })?;
    let mut stdin = std::io::BufReader::new(std::io::stdin().lock());
    while let Some(request) = read_request(&mut stdin)? {
        let result = match request {
            Request::Image(pixels) => engine.image_inner(pixels),
            Request::Text(text) => engine.text(&text),
        };
        let (vector, error) = match result {
            Ok(v) => (Some(v), None),
            Err(e) => (None, Some(format!("{e:#}"))),
        };
        emit(Response {
            backend: engine.backend.clone(),
            vector,
            error,
        })?;
    }
    Ok(())
}

pub fn preprocess(image: &DynamicImage) -> Vec<f32> {
    // Composite alpha onto neutral grey, then bicubic resize exactly to 224².
    let composited;
    let rgb = if let Some(rgb) = image.as_rgb8() {
        rgb
    } else {
        let rgba = image.to_rgba8();
        composited = image::RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
            let p = rgba.get_pixel(x, y).0;
            if p[3] == 255 {
                return image::Rgb([p[0], p[1], p[2]]);
            }
            let a = p[3] as f32 / 255.;
            image::Rgb([0, 1, 2].map(|c| (p[c] as f32 * a + 127.5 * (1. - a)).round() as u8))
        });
        &composited
    };
    let resized = image::imageops::resize(rgb, 224, 224, FilterType::CatmullRom);
    let mut data = vec![0.; 3 * 224 * 224];
    for (i, p) in resized.pixels().enumerate() {
        for c in 0..3 {
            data[c * 224 * 224 + i] = p[c] as f32 / 127.5 - 1.;
        }
    }
    data
}
pub fn normalize(mut values: Vec<f32>) -> Result<Vec<f32>> {
    ensure!(
        values.len() == 768 && values.iter().all(|x| x.is_finite()),
        "Unexpected SigLIP embedding"
    );
    let norm = values.iter().map(|x| x * x).sum::<f32>().sqrt();
    ensure!(norm > 1e-12, "Empty embedding");
    for v in &mut values {
        *v /= norm;
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preprocessing_preserves_rgb_and_alpha_compositing() {
        let rgb = image::RgbImage::from_fn(317, 259, |x, y| {
            image::Rgb([x as u8, y as u8, (x + y) as u8])
        });
        let image = DynamicImage::ImageRgb8(rgb.clone());
        assert_eq!(
            preprocess(&image),
            preprocess(&DynamicImage::ImageRgba8(image.to_rgba8()))
        );
        let rgba = image::RgbaImage::from_fn(317, 259, |x, y| {
            image::Rgba([x as u8, y as u8, (x + y) as u8, (x * y) as u8])
        });
        let composited = image::RgbImage::from_fn(317, 259, |x, y| {
            let p = rgba.get_pixel(x, y).0;
            let a = p[3] as f32 / 255.;
            image::Rgb([0, 1, 2].map(|c| (p[c] as f32 * a + 127.5 * (1. - a)).round() as u8))
        });
        assert_eq!(
            preprocess(&DynamicImage::ImageRgba8(rgba)),
            preprocess(&DynamicImage::ImageRgb8(composited))
        );
    }
    #[test]
    fn cancelled_download_removes_partial_and_retry_keeps_completed_files() {
        use std::{net::TcpListener, sync::Arc, time::Instant};
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("complete"), b"saved").unwrap();
        let saved_hash = format!("{:x}", Sha256::digest(b"saved"));
        let bytes = vec![42_u8; 200_000];
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let files = [
            ("complete", saved_hash.as_str(), 5),
            ("model", hash.as_str(), bytes.len() as u64),
        ];
        for cancel in [true, false] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let body = bytes.clone();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                assert!(String::from_utf8_lossy(&request).starts_with("GET /model "));
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream
                    .write_all(if cancel { &body[..120_000] } else { &body })
                    .unwrap();
                if cancel {
                    let _ = stream.read(&mut byte);
                }
            });
            let cancelled = Arc::new(AtomicBool::new(false));
            let started = Instant::now();
            let mut progress = Vec::new();
            let finished = download_from(
                dir.path(),
                &cancelled,
                |done, total| {
                    progress.push((done, total));
                    if cancel && done > 100_000 {
                        cancelled.store(true, Ordering::Relaxed);
                    }
                },
                &base,
                &files,
            )
            .unwrap();
            assert_eq!(finished, !cancel);
            assert!(!dir.path().join("model.partial").exists());
            assert_eq!(fs::read(dir.path().join("complete")).unwrap(), b"saved");
            if cancel {
                assert!(started.elapsed() < Duration::from_secs(2));
                assert!(!dir.path().join("model").exists());
            } else {
                assert_eq!(fs::read(dir.path().join("model")).unwrap(), bytes);
                assert_eq!(progress.last(), Some(&(200_005, 200_005)));
            }
            server.join().unwrap();
        }
    }
    #[test]
    fn inference_frames_preserve_pixels_and_reject_invalid_lengths() {
        let pixels: Vec<f32> = (0..IMAGE_FLOATS)
            .map(|i| (i % 256) as f32 / 127.5 - 1.)
            .collect();
        let mut bytes = vec![];
        write_request(&mut bytes, &Request::Image(pixels.clone())).unwrap();
        write_request(&mut bytes, &Request::Text("red silk 🐈".into())).unwrap();
        let mut reader = std::io::Cursor::new(bytes);
        assert!(
            matches!(read_request(&mut reader).unwrap(), Some(Request::Image(v)) if v == pixels)
        );
        assert!(
            matches!(read_request(&mut reader).unwrap(), Some(Request::Text(v)) if v == "red silk 🐈")
        );
        assert!(read_request(&mut reader).unwrap().is_none());
        for malformed in [
            vec![1, 0, 0, 0, 0],
            vec![2, 255, 255, 255, 255],
            vec![3, 0, 0, 0, 0],
            vec![2, 2, 0, 0, 0, b'a'],
        ] {
            assert!(read_request(malformed.as_slice()).is_err());
        }
    }
    #[test]
    fn image_layout_and_normalization() {
        let im = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            2,
            3,
            image::Rgba([255, 0, 0, 255]),
        ));
        let v = preprocess(&im);
        assert_eq!(v.len(), 150528);
        assert_eq!(v[0], 1.);
        assert_eq!(v[50176], -1.);
        let n = normalize(vec![2.; 768]).unwrap();
        assert!((n.iter().map(|x| x * x).sum::<f32>() - 1.).abs() < 0.0001);
        assert!(normalize(vec![f32::NAN; 768]).is_err());
    }
}
