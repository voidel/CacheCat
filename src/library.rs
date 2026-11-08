use crate::{
    cache::{self, Cache},
    worker::Item,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SortOrder {
    #[default]
    Relevance,
    Newest,
    Oldest,
    Largest,
    Smallest,
    Alphabetical,
    ReverseAlphabetical,
}
impl SortOrder {
    pub const ALL: [Self; 7] = [
        Self::Relevance,
        Self::Newest,
        Self::Oldest,
        Self::Largest,
        Self::Smallest,
        Self::Alphabetical,
        Self::ReverseAlphabetical,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Relevance => "Relevance / newest",
            Self::Newest => "Most recent first",
            Self::Oldest => "Oldest first",
            Self::Largest => "Dimensions: largest first",
            Self::Smallest => "Dimensions: smallest first",
            Self::Alphabetical => "UUID: A–Z",
            Self::ReverseAlphabetical => "UUID: Z–A",
        }
    }
    pub fn sort(self, items: &[Item], matches: &[usize]) -> Vec<usize> {
        let mut visible = matches.to_vec();
        if self == Self::Relevance {
            return visible;
        }
        visible.sort_by(|&a, &b| {
            let (a, b) = (&items[a], &items[b]);
            let order = match self {
                Self::Newest => b.texture.timestamp.cmp(&a.texture.timestamp),
                Self::Oldest => a.texture.timestamp.cmp(&b.texture.timestamp),
                Self::Alphabetical => a.texture.id.cmp(&b.texture.id),
                Self::ReverseAlphabetical => b.texture.id.cmp(&a.texture.id),
                Self::Largest | Self::Smallest => {
                    let area =
                        |item: &Item| item.dimensions.map(|(w, h)| u64::from(w) * u64::from(h));
                    match (area(a), area(b)) {
                        (Some(a), Some(b)) => {
                            if self == Self::Largest {
                                b.cmp(&a)
                            } else {
                                a.cmp(&b)
                            }
                        }
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        _ => std::cmp::Ordering::Equal,
                    }
                }
                Self::Relevance => unreachable!(),
            };
            order.then(a.texture.id.cmp(&b.texture.id))
        });
        visible
    }
}

#[derive(Default, Clone, Copy, PartialEq)]
pub enum ExportFormat {
    #[default]
    Png,
    Tga,
}
impl ExportFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Tga => "TGA",
        }
    }
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Tga => "tga",
        }
    }
    fn image_format(self) -> image::ImageFormat {
        match self {
            Self::Png => image::ImageFormat::Png,
            Self::Tga => image::ImageFormat::Tga,
        }
    }
}

pub fn export(root: &Path, id: uuid::Uuid, destination: &Path, format: ExportFormat) -> Result<()> {
    let cache = Cache::open(root)?;
    let texture = cache
        .textures
        .iter()
        .find(|t| t.id == id)
        .context("Texture is no longer in the viewer cache; rescan")?;
    let image = cache::decode(&cache.read(texture)?)?;
    write_export(&image, destination, format)
}
fn write_export(
    image: &image::DynamicImage,
    destination: &Path,
    format: ExportFormat,
) -> Result<()> {
    // Export the decoded original, never the 256-pixel preview. Preserve transparency.
    image
        .to_rgba8()
        .save_with_format(destination, format.image_format())
        .context("Could not save texture")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_reads_original_cache_and_rejects_missing_assets() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("web/tests/fixtures/texturecache");
        let temp = tempfile::tempdir().unwrap();
        for format in [ExportFormat::Png, ExportFormat::Tga] {
            let destination = temp.path().join(format.extension());
            export(&root, uuid::Uuid::from_u128(1), &destination, format).unwrap();
            let image = image::load_from_memory_with_format(
                &std::fs::read(destination).unwrap(),
                format.image_format(),
            )
            .unwrap();
            assert_eq!((image.width(), image.height()), (128, 128));
        }
        let missing = temp.path().join("missing.png");
        assert!(export(&root, uuid::Uuid::nil(), &missing, ExportFormat::Png).is_err());
        assert!(!missing.exists());
    }
    #[test]
    fn exports_preserve_dimensions_pixels_and_alpha() {
        let temp = tempfile::tempdir().unwrap();
        let pixels = image::RgbaImage::from_fn(321, 259, |x, y| {
            image::Rgba([x as u8, y as u8, 42, (x + y) as u8])
        });
        for format in [ExportFormat::Png, ExportFormat::Tga] {
            let path = temp.path().join(format.extension());
            write_export(
                &image::DynamicImage::ImageRgba8(pixels.clone()),
                &path,
                format,
            )
            .unwrap();
            let exported = image::load_from_memory_with_format(
                &std::fs::read(path).unwrap(),
                format.image_format(),
            )
            .unwrap()
            .to_rgba8();
            assert_eq!(exported, pixels);
        }
    }
    #[test]
    fn sorts_keep_unknown_dimensions_last_and_relevance_stable() {
        let items: Vec<_> = [
            (3, 20, Some((512, 512))),
            (1, 10, None),
            (2, 30, Some((128, 256))),
        ]
        .into_iter()
        .map(|(id, timestamp, dimensions)| Item {
            texture: cache::Texture {
                id: uuid::Uuid::from_u128(id),
                timestamp,
                slot: 0,
                image_size: 600,
                body_size: 0,
            },
            dimensions,
            thumbnail: None,
            error: None,
            indexed: false,
        })
        .collect();
        let all = [0, 1, 2];
        assert_eq!(SortOrder::Relevance.sort(&items, &[2, 0, 1]), [2, 0, 1]);
        assert_eq!(SortOrder::Newest.sort(&items, &all), [2, 0, 1]);
        assert_eq!(SortOrder::Oldest.sort(&items, &all), [1, 0, 2]);
        assert_eq!(SortOrder::Largest.sort(&items, &all), [0, 2, 1]);
        assert_eq!(SortOrder::Smallest.sort(&items, &all), [2, 0, 1]);
        assert_eq!(SortOrder::Alphabetical.sort(&items, &all), [1, 2, 0]);
        assert_eq!(SortOrder::ReverseAlphabetical.sort(&items, &all), [0, 2, 1]);
    }
}
