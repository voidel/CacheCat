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
        if texture.body_size > 0 {
            let id = texture.id.to_string();
            let path = self.root.join(&id[..1]).join(format!("{id}.texture"));
            // Some older viewers use textures/<hex>/<uuid>.texture.
            let path = if path.exists() {
                path
            } else {
                self.root
                    .join("textures")
                    .join(&id[..1])
                    .join(format!("{id}.texture"))
            };
            if let Ok(body) = File::open(path) {
                body.take(texture.body_size as u64)
                    .read_to_end(&mut bytes)?;
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
    let decoded =
        jpeg2k::Image::from_bytes(bytes).context("Incomplete or unsupported JPEG 2000 texture")?;
    let image: DynamicImage = (&decoded).try_into()?;
    Ok(image)
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
}
