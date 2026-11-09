//! Read-only Firestorm / Second Life texture cache access.
use anyhow::{Context, Result, bail, ensure};
use image::DynamicImage;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
use uuid::Uuid;

const HEADER_BYTES: usize = 600;
const MAX_TEXTURE: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Texture {
    pub id: Uuid,
    pub slot: usize,
    pub image_size: usize,
    pub body_size: usize,
    pub timestamp: u32,
}

#[derive(Clone, Debug)]
pub struct Cache {
    pub root: PathBuf,
    pub textures: Vec<Texture>,
}

impl Cache {
    pub fn open(path: &Path) -> Result<Self> {
        let root = [
            path.to_path_buf(),
            path.join("texturecache"),
            path.join("FirestormCache/texturecache"),
        ]
        .into_iter()
        .find(|p| p.join("texture.entries").is_file())
        .unwrap_or_else(|| path.join("texturecache"));
        let mut bytes = Vec::new();
        File::open(root.join("texture.entries"))
            .context("Select the viewer cache folder or its texturecache subfolder")?
            .take(128 * 1024 * 1024)
            .read_to_end(&mut bytes)?;
        let textures = parse_entries(&bytes)?;
        Ok(Self { root, textures })
    }

    pub fn read(&self, texture: &Texture) -> Result<Vec<u8>> {
        let mut file = File::open(self.root.join("texture.cache"))?;
        file.seek(SeekFrom::Start((texture.slot * HEADER_BYTES) as u64))?;
        let mut bytes = vec![0; texture.image_size.min(HEADER_BYTES)];
        file.read_exact(&mut bytes)
            .context("Cache header changed or is incomplete; rescan")?;
        if texture.image_size > HEADER_BYTES || texture.body_size > 0 {
            let id = texture.id.to_string();
            let path = self.root.join(&id[..1]).join(format!("{id}.texture"));
            // Some older viewers use textures/<hex>/<uuid>.texture.
            let body = match File::open(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => File::open(
                    self.root
                        .join("textures")
                        .join(&id[..1])
                        .join(format!("{id}.texture")),
                ),
                result => result,
            };
            match body {
                Ok(body) => {
                    // The viewer can grow this UUID's body before flushing its
                    // entry table. image_size can also be a provisional estimate
                    // (cached size + 1). Read the bytes actually available.
                    let remaining = MAX_TEXTURE - bytes.len();
                    ensure!(
                        body.metadata()?.len() <= remaining as u64,
                        "Texture is too large"
                    );
                    body.take(remaining as u64 + 1).read_to_end(&mut bytes)?;
                }
                // A header alone can still contain a usable progressive image.
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error).context("Cannot read cached texture body"),
            }
        }
        ensure!(bytes.len() <= MAX_TEXTURE, "Texture is too large");
        Ok(bytes)
    }
}

pub fn parse_entries(bytes: &[u8]) -> Result<Vec<Texture>> {
    ensure!(bytes.len() >= 8, "Truncated texture index");
    let version = f32::from_le_bytes(bytes[..4].try_into()?);
    // Legacy SL/Firestorm 1.7: version + count. Firestorm 1.71 adds
    // address size and a 32-byte encoder identifier before the count.
    let header = if version == 1.7_f32 {
        8
    } else if version == 1.71_f32 {
        44
    } else {
        bail!("Unsupported texture cache version {version}")
    };
    ensure!(bytes.len() >= header, "Truncated cache header");
    if header == 44 {
        ensure!(
            matches!(u32::from_le_bytes(bytes[4..8].try_into()?), 32 | 64),
            "Invalid cache address size"
        );
    }
    let count = u32::from_le_bytes(bytes[header - 4..header].try_into()?) as usize;
    ensure!(
        count <= 4_000_000 && bytes.len() >= header + count * 28,
        "Truncated or invalid texture entry table; rescan after the viewer finishes writing"
    );
    let mut result = Vec::with_capacity(count);
    for (slot, entry) in bytes[header..header + count * 28]
        .as_chunks::<28>()
        .0
        .iter()
        .enumerate()
    {
        let id = Uuid::from_slice(&entry[..16])?;
        let image_size = i32::from_le_bytes(entry[16..20].try_into()?);
        let body_size = i32::from_le_bytes(entry[20..24].try_into()?);
        let timestamp = u32::from_le_bytes(entry[24..28].try_into()?);
        if id.is_nil() || image_size <= 0 || body_size < 0 {
            continue;
        }
        if image_size as usize > MAX_TEXTURE || body_size as usize > MAX_TEXTURE - HEADER_BYTES {
            continue;
        }
        if body_size as usize > (image_size as usize).saturating_sub(HEADER_BYTES) {
            continue;
        }
        result.push(Texture {
            id,
            slot,
            image_size: image_size as usize,
            body_size: body_size as usize,
            timestamp,
        });
    }
    result.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then(a.id.cmp(&b.id)));
    let mut seen = std::collections::HashSet::new();
    result.retain(|t| seen.insert(t.id));
    Ok(result)
}

pub fn fingerprint(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Raw JPEG2000 downloads are prefixes of the codestream. A complete stream
/// ends with the EOC marker, unlike the viewer's provisional size estimates.
pub fn is_complete(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xff, 0x4f, 0xff, 0x51]) && bytes.ends_with(&[0xff, 0xd9])
}

pub fn dimensions(bytes: &[u8]) -> Result<(u32, u32)> {
    // Validate JPEG 2000 SIZ dimensions before handing data to the decoder.
    if bytes.starts_with(&[0xff, 0x4f, 0xff, 0x51]) {
        ensure!(bytes.len() >= 42, "Truncated JPEG 2000 SIZ marker");
        let x = u32::from_be_bytes(bytes[8..12].try_into()?);
        let y = u32::from_be_bytes(bytes[12..16].try_into()?);
        let ox = u32::from_be_bytes(bytes[16..20].try_into()?);
        let oy = u32::from_be_bytes(bytes[20..24].try_into()?);
        ensure!(
            x > ox
                && y > oy
                && x <= 8192
                && y <= 8192
                && u64::from(x - ox) * u64::from(y - oy) <= 16_777_216,
            "Invalid or oversized texture dimensions"
        );
        Ok((x - ox, y - oy))
    } else {
        bail!("Not a raw JPEG 2000 texture");
    }
}

pub fn decode(bytes: &[u8]) -> Result<DynamicImage> {
    dimensions(bytes)?;
    // This option is effective only with jpeg2k's `strict-mode` Cargo feature.
    // Viewer caches routinely contain progressive, partially downloaded images.
    let decoded =
        jpeg2k::Image::from_bytes_with(bytes, jpeg2k::DecodeParameters::new().strict(false))
            .with_context(|| {
                format!(
                    "Incomplete or unsupported JPEG 2000 texture ({} cached bytes)",
                    bytes.len()
                )
            })?;
    let components = decoded.components();
    if components.len() == 5 {
        // SL baked textures can carry an auxiliary fifth channel. The viewer
        // displays channels 0..4 and decodes the auxiliary channel separately:
        // secondlife/viewer, indra/llimage/llimagej2c.cpp + llimageworker.cpp.
        let [r, g, b, a] = &components[..4] else {
            unreachable!()
        };
        let (width, height) = (r.width(), r.height());
        ensure!(
            components[..4].iter().all(|c| c.width() == width
                && c.height() == height
                && (1..=16).contains(&c.precision())),
            "Unsupported baked texture channel dimensions or precision"
        );
        if components[..4].iter().all(|c| c.precision() <= 8) {
            let pixels = r
                .data_u8()
                .zip(g.data_u8())
                .zip(b.data_u8())
                .zip(a.data_u8())
                .flat_map(|(((r, g), b), a)| [r, g, b, a])
                .collect();
            let image = image::RgbaImage::from_raw(width, height, pixels)
                .context("Incomplete baked texture channels")?;
            Ok(DynamicImage::ImageRgba8(image))
        } else {
            let pixels = r
                .data_u16()
                .zip(g.data_u16())
                .zip(b.data_u16())
                .zip(a.data_u16())
                .flat_map(|(((r, g), b), a)| [r, g, b, a])
                .collect();
            let image = image::ImageBuffer::from_raw(width, height, pixels)
                .context("Incomplete baked texture channels")?;
            Ok(DynamicImage::ImageRgba16(image))
        }
    } else {
        Ok((&decoded).try_into()?)
    }
}

pub fn default_path() -> Option<PathBuf> {
    let mut paths = vec![];
    if let Some(base) = directories::BaseDirs::new() {
        paths.extend([
            base.cache_dir().join("firestorm"),
            base.cache_dir().join("secondlife"),
            base.home_dir().join(".firestorm_x64/cache"),
            base.home_dir().join(".firestorm/cache"),
            base.home_dir().join(".secondlife/cache"),
            base.home_dir().join("Library/Caches/Firestorm"),
            base.home_dir().join("Library/Caches/SecondLife"),
            base.data_local_dir().join("Firestorm_x64"),
            base.data_local_dir().join("SecondLife"),
        ]);
    }
    paths.into_iter().find(|p| {
        p.join("texturecache/texture.entries").is_file() || p.join("texture.entries").is_file()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(version: f32, entries: &[(u128, i32, i32, u32)]) -> Vec<u8> {
        let mut b = version.to_le_bytes().to_vec();
        if version == 1.71 {
            b.extend(64_u32.to_le_bytes());
            b.extend([0; 32]);
        }
        b.extend((entries.len() as u32).to_le_bytes());
        for &(id, image, body, time) in entries {
            b.extend(id.to_be_bytes());
            b.extend(image.to_le_bytes());
            b.extend(body.to_le_bytes());
            b.extend(time.to_le_bytes());
        }
        b
    }
    #[test]
    fn formats_and_recency() {
        for v in [1.7, 1.71] {
            let items = parse_entries(&fixture(
                v,
                &[
                    (1, 700, 100, 5),
                    (2, 600, 0, 9),
                    (0, 600, 0, 10),
                    (3, -1, 0, 11),
                ],
            ))
            .unwrap();
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].id, Uuid::from_u128(2));
            assert_eq!(items[1].slot, 0);
        }
    }
    #[test]
    fn rejects_corrupt_headers() {
        assert!(parse_entries(&[0; 7]).is_err());
        assert!(parse_entries(&fixture(9.0, &[])).is_err());
        let mut b = fixture(1.7, &[(1, 700, 100, 0)]);
        b.pop();
        assert!(parse_entries(&b).is_err());
    }
    #[test]
    fn joins_header_and_body_without_modifying_cache() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("texture.entries"),
            fixture(1.7, &[(1, 700, 100, 1)]),
        )
        .unwrap();
        std::fs::write(dir.path().join("texture.cache"), vec![42; 600]).unwrap();
        std::fs::create_dir(dir.path().join("0")).unwrap();
        std::fs::write(
            dir.path().join(format!("0/{}.texture", Uuid::from_u128(1))),
            vec![7; 100],
        )
        .unwrap();
        let c = Cache::open(dir.path()).unwrap();
        let bytes = c.read(&c.textures[0]).unwrap();
        assert_eq!(&bytes[..600], &vec![42; 600]);
        assert_eq!(&bytes[600..], &vec![7; 100]);
    }

    #[test]
    fn reads_a_grown_body_even_when_entry_sizes_are_stale() {
        for declared_body in [0, 20] {
            let dir = tempfile::tempdir().unwrap();
            let entries = fixture(1.71, &[(1, 601 + declared_body, declared_body, 1)]);
            std::fs::write(dir.path().join("texture.entries"), &entries).unwrap();
            std::fs::write(dir.path().join("texture.cache"), vec![42; 600]).unwrap();
            std::fs::create_dir(dir.path().join("0")).unwrap();
            let path = dir.path().join(format!("0/{}.texture", Uuid::from_u128(1)));
            std::fs::write(&path, vec![7; 100]).unwrap();
            let cache = Cache::open(dir.path()).unwrap();
            let bytes = cache.read(&cache.textures[0]).unwrap();
            assert_eq!(bytes.len(), 700);
            assert_eq!(&bytes[600..], &[7; 100]);
            assert_eq!(std::fs::read(path).unwrap(), vec![7; 100]);
            assert_eq!(
                std::fs::read(dir.path().join("texture.entries")).unwrap(),
                entries
            );
        }
    }

    #[test]
    fn partial_and_legacy_bodies_stay_readable_with_a_hard_size_limit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("texture.entries"),
            fixture(1.7, &[(1, 800, 200, 1)]),
        )
        .unwrap();
        std::fs::write(dir.path().join("texture.cache"), vec![42; 600]).unwrap();
        let cache = Cache::open(dir.path()).unwrap();
        assert_eq!(cache.read(&cache.textures[0]).unwrap().len(), 600);
        let folder = dir.path().join("textures/0");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(format!("{}.texture", Uuid::from_u128(1)));
        std::fs::write(&path, vec![7; 50]).unwrap();
        assert_eq!(cache.read(&cache.textures[0]).unwrap().len(), 650);
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_len(MAX_TEXTURE as u64)
            .unwrap();
        assert!(
            cache
                .read(&cache.textures[0])
                .unwrap_err()
                .to_string()
                .contains("too large")
        );
    }

    #[test]
    fn decodes_progressive_cache_data_without_changing_complete_images() {
        let cache = Cache::open(Path::new("tests/fixtures/texturecache")).unwrap();
        let bytes = cache.read(&cache.textures[0]).unwrap();
        assert!(is_complete(&bytes));
        let strict =
            jpeg2k::Image::from_bytes_with(&bytes, jpeg2k::DecodeParameters::new().strict(true))
                .unwrap();
        let expected: DynamicImage = (&strict).try_into().unwrap();
        assert_eq!(decode(&bytes).unwrap().into_rgba8(), expected.into_rgba8());
        for length in [bytes.len() - 2, bytes.len() / 2] {
            let partial = &bytes[..length];
            assert!(!is_complete(partial));
            assert!(
                jpeg2k::Image::from_bytes_with(
                    partial,
                    jpeg2k::DecodeParameters::new().strict(true)
                )
                .is_err()
            );
            let image = decode(partial).unwrap();
            assert_eq!((image.width(), image.height()), (128, 128));
        }
        assert!(decode(&bytes[..42]).is_err());
    }

    #[test]
    fn baked_texture_auxiliary_channel_is_not_used_as_colour_or_alpha() {
        // Synthetic lossless 32x32 OpenJPEG fixture, not a viewer-cache asset.
        // Channels are R=x*8, G=y*8, B=(x+y)*4, A=x*8, auxiliary=255-y*8.
        let bytes = include_bytes!("../tests/fixtures/baked-five-channel.j2c");
        let raw = jpeg2k::Image::from_bytes(bytes).unwrap();
        assert_eq!(raw.num_components(), 5);
        let image = decode(bytes).unwrap().into_rgba8();
        assert_eq!(image.dimensions(), (32, 32));
        for (x, y, pixel) in image.enumerate_pixels() {
            assert_eq!(
                pixel.0,
                [
                    (x * 8) as u8,
                    (y * 8) as u8,
                    ((x + y) * 4) as u8,
                    (x * 8) as u8
                ]
            );
        }
    }
}
