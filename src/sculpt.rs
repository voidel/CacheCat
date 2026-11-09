//! Read-only sculpt-map geometry and a small, bounded matcap preview renderer.
//!
//! Sampling/stitching follows Second Life's documented sculpt format and viewer:
//! https://wiki.secondlife.com/wiki/Sculpted_Prims:_Technical_Explanation
//! https://github.com/secondlife/viewer/blob/master/indra/llmath/llvolume.cpp
//! Row orientation: indra/llimagej2coj/llimagej2coj.cpp, decodeImpl.
use anyhow::{Result, ensure};
use image::{RgbImage, Rgba, RgbaImage};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Topology {
    #[default]
    Sphere,
    Torus,
    Cylinder,
    Plane,
}

impl Topology {
    pub const ALL: [Self; 4] = [Self::Sphere, Self::Torus, Self::Cylinder, Self::Plane];

    pub fn label(self) -> &'static str {
        match self {
            Self::Sphere => "Sphere",
            Self::Torus => "Torus",
            Self::Cylinder => "Cylinder",
            Self::Plane => "Plane",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub topology: Topology,
    pub mirror: bool,
    pub invert: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct V3(f32, f32, f32);

impl V3 {
    fn add(self, b: Self) -> Self {
        Self(self.0 + b.0, self.1 + b.1, self.2 + b.2)
    }
    fn sub(self, b: Self) -> Self {
        self.add(b.scale(-1.))
    }
    fn scale(self, k: f32) -> Self {
        Self(self.0 * k, self.1 * k, self.2 * k)
    }
    fn dot(self, b: Self) -> f32 {
        self.0 * b.0 + self.1 * b.1 + self.2 * b.2
    }
    fn cross(self, b: Self) -> Self {
        Self(
            self.1 * b.2 - self.2 * b.1,
            self.2 * b.0 - self.0 * b.2,
            self.0 * b.1 - self.1 * b.0,
        )
    }
    fn unit(self) -> Self {
        self.scale(1. / self.dot(self).sqrt().max(1e-12))
    }
}

pub struct Mesh {
    positions: Vec<V3>,
    normals: Vec<V3>,
    triangles: Vec<[usize; 3]>,
    center: V3,
    radius: f32,
    two_sided: bool,
}

// Highest SL sculpt LOD: a 1,024-cell budget, adjusted for map aspect ratio.
fn resolution(w: u32, h: u32) -> (usize, usize) {
    let budget = (u64::from(w) * u64::from(h) / 4).clamp(16, 1024) as usize;
    let rows = ((budget as f64 * h as f64 / w as f64).sqrt() as usize).max(4);
    let cols = (budget / rows).max(4);
    (cols.min(256), (budget / cols).clamp(4, 256))
}

impl Mesh {
    pub fn from_map(map: &RgbImage, options: Options) -> Result<Self> {
        let (w, h) = map.dimensions();
        ensure!(
            w >= 2 && h >= 2,
            "This image is too small to form a sculpt."
        );
        let (cols, rows) = resolution(w, h);
        let mut positions = Vec::new();
        let mut samples = HashMap::new();
        let mut grid = Vec::with_capacity((cols + 1) * (rows + 1));
        for row in 0..=rows {
            for col in 0..=cols {
                let col = if options.mirror ^ options.invert {
                    cols - col
                } else {
                    col
                };
                let mut x = col as u32 * w / cols as u32;
                let mut y = row as u32 * h / rows as u32;
                if options.topology == Topology::Sphere && (row == 0 || row == rows) {
                    x = w / 2;
                }
                if x == w {
                    x = if options.topology == Topology::Plane {
                        w - 1
                    } else {
                        0
                    };
                }
                if y == h {
                    y = if options.topology == Topology::Torus {
                        0
                    } else {
                        h - 1
                    };
                }
                // Share seam and pole vertices so their normals are smooth too.
                let index = *samples.entry((x, y)).or_insert_with(|| {
                    // The viewer's JPEG2000 decoder stores rows bottom-up;
                    // image::RgbImage uses top-down rows. Preserve SL sampling.
                    let pixel = map.get_pixel(x, h - 1 - y).0;
                    let mut p = V3(
                        pixel[0] as f32 / 255. - 0.5,
                        pixel[1] as f32 / 255. - 0.5,
                        pixel[2] as f32 / 255. - 0.5,
                    );
                    if options.mirror {
                        p.0 = -p.0;
                    }
                    positions.push(p);
                    positions.len() - 1
                });
                grid.push(index);
            }
        }
        let mut triangles = Vec::with_capacity(cols * rows * 2);
        let mut normals = vec![V3::default(); positions.len()];
        for row in 0..rows {
            for col in 0..cols {
                let a = grid[row * (cols + 1) + col];
                let b = grid[row * (cols + 1) + col + 1];
                let c = grid[(row + 1) * (cols + 1) + col];
                let d = grid[(row + 1) * (cols + 1) + col + 1];
                for triangle in [[a, d, c], [a, b, d]] {
                    let [a, b, c] = triangle;
                    let normal = positions[b]
                        .sub(positions[a])
                        .cross(positions[c].sub(positions[a]));
                    if normal.dot(normal) > 1e-16 {
                        for i in triangle {
                            normals[i] = normals[i].add(normal);
                        }
                        triangles.push(triangle);
                    }
                }
            }
        }
        ensure!(
            !triangles.is_empty(),
            "This texture does not form a visible sculpt surface."
        );
        let mut lo = V3(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let mut hi = V3(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for p in &positions {
            lo = V3(lo.0.min(p.0), lo.1.min(p.1), lo.2.min(p.2));
            hi = V3(hi.0.max(p.0), hi.1.max(p.1), hi.2.max(p.2));
        }
        let center = lo.add(hi).scale(0.5);
        let radius = positions
            .iter()
            .map(|p| p.sub(center).dot(p.sub(center)))
            .fold(0., f32::max)
            .sqrt()
            .max(1e-6);
        Ok(Self {
            positions,
            normals: normals.into_iter().map(V3::unit).collect(),
            triangles,
            center,
            radius,
            two_sided: options.topology == Topology::Plane,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    pub rotating: bool,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            yaw: 0.5,
            pitch: 0.2,
            zoom: 1.,
            rotating: true,
        }
    }
}

impl Camera {
    pub fn tick(&mut self, seconds: f32) {
        if self.rotating {
            self.yaw = (self.yaw + seconds.min(0.1) * 0.45) % std::f32::consts::TAU;
        }
    }
    pub fn interact(&mut self) {
        self.rotating = false;
    }
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.interact();
        self.yaw += dx * 0.009;
        self.pitch = (self.pitch + dy * 0.009).clamp(-1.5, 1.5);
    }
    pub fn zoom_by(&mut self, delta: f32) {
        self.interact();
        self.zoom = (self.zoom * (delta * 0.002).exp()).clamp(0.4, 2.5);
    }
    fn transform(self, p: V3) -> V3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let x = cy * p.0 - sy * p.1;
        let depth = sy * p.0 + cy * p.1;
        V3(x, cp * p.2 - sp * depth, -cp * depth - sp * p.2)
    }
}

/// A procedural neutral ceramic matcap: sampled by view-space surface normals.
/// No network assets, new graphics backend, or persistent image files required.
pub struct Renderer {
    matcap: RgbImage,
}

impl Default for Renderer {
    fn default() -> Self {
        let light = V3(-0.4, 0.65, 0.85).unit();
        let highlight = V3(-0.3, 0.45, 1.).unit();
        let matcap = RgbImage::from_fn(256, 256, |x, y| {
            let nx = x as f32 / 127.5 - 1.;
            let ny = 1. - y as f32 / 127.5;
            let n = V3(nx, ny, (1. - nx * nx - ny * ny).max(0.).sqrt()).unit();
            let diffuse = n.dot(light).max(0.);
            let specular = n.dot(highlight).max(0.).powi(36);
            let rim = (1. - n.2).powi(3);
            let value = (0.25 + 0.56 * diffuse + 0.25 * specular + 0.08 * rim).clamp(0., 1.);
            image::Rgb([
                (value * 239.) as u8,
                (value * 241.) as u8,
                (value * 245.) as u8,
            ])
        });
        Self { matcap }
    }
}

impl Renderer {
    /// Dimensions are capped to bound CPU time and the one in-flight frame.
    pub fn render(
        &self,
        mesh: &Mesh,
        camera: Camera,
        width: u32,
        height: u32,
        background: [u8; 3],
    ) -> RgbaImage {
        let (width, height) = (width.clamp(1, 768), height.clamp(1, 768));
        let mut image = RgbaImage::from_pixel(
            width,
            height,
            Rgba([background[0], background[1], background[2], 255]),
        );
        let mut depth = vec![f32::NEG_INFINITY; (width * height) as usize];
        let scale = width.min(height) as f32 * 0.43 * camera.zoom / mesh.radius;
        let positions: Vec<_> = mesh
            .positions
            .iter()
            .map(|p| {
                let p = camera.transform(p.sub(mesh.center));
                V3(
                    width as f32 * 0.5 + p.0 * scale,
                    height as f32 * 0.5 - p.1 * scale,
                    p.2,
                )
            })
            .collect();
        let normals: Vec<_> = mesh.normals.iter().map(|n| camera.transform(*n)).collect();
        for &[a, b, c] in &mesh.triangles {
            let (pa, pb, pc) = (positions[a], positions[b], positions[c]);
            let area = edge(pa, pb, pc.0, pc.1);
            if area.abs() < 1e-6 || (!mesh.two_sided && area < 0.) {
                continue;
            }
            let min_x = pa.0.min(pb.0).min(pc.0).floor().max(0.) as u32;
            let max_x = pa.0.max(pb.0).max(pc.0).ceil().min(width as f32) as u32;
            let min_y = pa.1.min(pb.1).min(pc.1).floor().max(0.) as u32;
            let max_y = pa.1.max(pb.1).max(pc.1).ceil().min(height as f32) as u32;
            for y in min_y..max_y {
                for x in min_x..max_x {
                    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                    let u = edge(pb, pc, fx, fy) / area;
                    let v = edge(pc, pa, fx, fy) / area;
                    let w = 1. - u - v;
                    if u < -1e-5 || v < -1e-5 || w < -1e-5 {
                        continue;
                    }
                    let z = u * pa.2 + v * pb.2 + w * pc.2;
                    let index = (y * width + x) as usize;
                    if z <= depth[index] {
                        continue;
                    }
                    depth[index] = z;
                    let mut normal = normals[a]
                        .scale(u)
                        .add(normals[b].scale(v))
                        .add(normals[c].scale(w))
                        .unit();
                    if mesh.two_sided && normal.2 < 0. {
                        normal = normal.scale(-1.);
                    }
                    let mx = ((normal.0 * 0.5 + 0.5) * 255.).clamp(0., 255.) as u32;
                    let my = ((0.5 - normal.1 * 0.5) * 255.).clamp(0., 255.) as u32;
                    let color = self.matcap.get_pixel(mx, my).0;
                    image.put_pixel(x, y, Rgba([color[0], color[1], color[2], 255]));
                }
            }
        }
        image
    }
}

fn edge(a: V3, b: V3, x: f32, y: f32) -> f32 {
    (x - a.0) * (b.1 - a.1) - (y - a.1) * (b.0 - a.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> RgbImage {
        RgbImage::from_fn(64, 64, |x, y| {
            image::Rgb([(x * 4) as u8, (y * 4) as u8, ((x ^ y) * 3) as u8])
        })
    }

    #[test]
    fn highest_lod_respects_aspect_ratio_and_bounds() {
        assert_eq!(resolution(64, 64), (32, 32));
        assert_eq!(resolution(128, 32), (64, 16));
        assert_eq!(resolution(32, 128), (16, 64));
        assert_eq!(resolution(8192, 2), (256, 4));
        assert_eq!(resolution(2, 8192), (4, 256));
    }

    #[test]
    fn samples_original_pixels_with_viewer_row_orientation() {
        let mesh = Mesh::from_map(
            &map(),
            Options {
                topology: Topology::Plane,
                ..Default::default()
            },
        )
        .unwrap();
        let position = mesh.positions[0];
        assert_eq!(position.0, -0.5);
        assert_eq!(position.1, 252. / 255. - 0.5);
        assert_eq!(mesh.positions[1].0, 8. / 255. - 0.5);
        assert_eq!(mesh.positions[33].1, 244. / 255. - 0.5);
        let last = mesh.positions.last().unwrap();
        assert_eq!(last.0, 252. / 255. - 0.5);
        assert_eq!(last.1, -0.5);
    }

    #[test]
    fn seams_and_poles_are_welded_without_open_cracks() {
        for (topology, vertices, boundary_edges) in [
            (Topology::Sphere, 994, 0),
            (Topology::Torus, 1024, 0),
            (Topology::Cylinder, 1056, 64),
            (Topology::Plane, 1089, 128),
        ] {
            let mesh = Mesh::from_map(
                &map(),
                Options {
                    topology,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(mesh.positions.len(), vertices, "{topology:?}");
            let mut edges = HashMap::<(usize, usize), usize>::new();
            for &[a, b, c] in &mesh.triangles {
                for (a, b) in [(a, b), (b, c), (c, a)] {
                    *edges.entry((a.min(b), a.max(b))).or_default() += 1;
                }
            }
            assert!(edges.values().all(|&n| n == 1 || n == 2));
            assert_eq!(
                edges.values().filter(|&&n| n == 1).count(),
                boundary_edges,
                "{topology:?}"
            );
            assert!(
                mesh.normals
                    .iter()
                    .all(|n| n.0.is_finite() && n.1.is_finite() && n.2.is_finite())
            );
        }
        let sphere = Mesh::from_map(&map(), Options::default()).unwrap();
        assert_eq!(sphere.positions[0].0, 128. / 255. - 0.5);
        assert_eq!(sphere.positions.last().unwrap().0, 128. / 255. - 0.5);
    }

    #[test]
    fn flags_match_sculpt_mirroring_and_inverted_winding() {
        let map = RgbImage::from_fn(64, 64, |x, y| {
            image::Rgb([(x * 4) as u8, (y * 4) as u8, 128])
        });
        let base = Options {
            topology: Topology::Plane,
            ..Default::default()
        };
        let normal = Mesh::from_map(&map, base).unwrap();
        let inverted = Mesh::from_map(
            &map,
            Options {
                invert: true,
                ..base
            },
        )
        .unwrap();
        let mirrored = Mesh::from_map(
            &map,
            Options {
                mirror: true,
                ..base
            },
        )
        .unwrap();
        assert!(normal.normals[0].2 < -0.99);
        assert!(inverted.normals[0].2 > 0.99);
        assert!(mirrored.normals[0].2 < -0.99);
        assert_eq!(inverted.positions[0].0, normal.positions[32].0);
        assert_eq!(mirrored.positions[0].0, -normal.positions[32].0);
    }

    #[test]
    fn rejects_empty_tiny_and_degenerate_maps() {
        assert!(Mesh::from_map(&RgbImage::new(0, 0), Options::default()).is_err());
        assert!(Mesh::from_map(&RgbImage::new(1, 64), Options::default()).is_err());
        assert!(
            Mesh::from_map(
                &RgbImage::from_pixel(64, 64, image::Rgb([128; 3])),
                Options::default()
            )
            .is_err()
        );
    }

    #[test]
    fn interaction_stops_rotation_and_bounds_zoom_and_pitch() {
        let mut camera = Camera::default();
        let start = camera.yaw;
        camera.tick(0.033);
        assert!(camera.yaw > start);
        camera.interact();
        let stopped = camera.yaw;
        camera.tick(1.);
        assert_eq!(camera.yaw, stopped);
        camera.orbit(10., 10000.);
        assert!(camera.yaw > stopped);
        assert_eq!(camera.pitch, 1.5);
        camera.zoom_by(10000.);
        assert_eq!(camera.zoom, 2.5);
        camera.zoom_by(-10000.);
        assert_eq!(camera.zoom, 0.4);
        assert!(!camera.rotating);
    }

    #[test]
    fn depth_buffer_is_independent_of_triangle_draw_order() {
        let mut mesh = Mesh {
            positions: vec![
                V3(-0.5, -0.2, -0.5),
                V3(0.5, -0.2, -0.5),
                V3(0., -0.2, 0.5),
                V3(-0.5, 0.2, -0.5),
                V3(0.5, 0.2, -0.5),
                V3(0., 0.2, 0.5),
            ],
            normals: vec![
                V3(0., -1., 0.),
                V3(0., -1., 0.),
                V3(0., -1., 0.),
                V3(0., -0.7, 0.7),
                V3(0., -0.7, 0.7),
                V3(0., -0.7, 0.7),
            ],
            triangles: vec![[0, 1, 2], [3, 4, 5]],
            center: V3::default(),
            radius: 1.,
            two_sided: false,
        };
        let camera = Camera {
            yaw: 0.,
            pitch: 0.,
            ..Default::default()
        };
        let renderer = Renderer::default();
        let front_first = renderer.render(&mesh, camera, 128, 128, [10, 10, 10]);
        mesh.triangles.reverse();
        let back_first = renderer.render(&mesh, camera, 128, 128, [10, 10, 10]);
        assert_eq!(front_first, back_first);
        assert_ne!(*front_first.get_pixel(64, 64), Rgba([10, 10, 10, 255]));
        assert_eq!(
            renderer.render(&mesh, camera, 0, 0, [0; 3]).dimensions(),
            (1, 1)
        );
        assert_eq!(
            renderer
                .render(&mesh, camera, 1024, 1024, [0; 3])
                .dimensions(),
            (768, 768)
        );
    }

    #[test]
    fn neutral_matcap_shows_a_sphere_with_smooth_shading() {
        let map = RgbImage::from_fn(64, 64, |x, y| {
            let theta = x as f32 / 64. * std::f32::consts::TAU;
            let phi = y as f32 / 63. * std::f32::consts::PI;
            let p = V3(phi.sin() * theta.cos(), phi.sin() * theta.sin(), phi.cos());
            image::Rgb([
                ((p.0 * 0.45 + 0.5) * 255.) as u8,
                ((p.1 * 0.45 + 0.5) * 255.) as u8,
                ((p.2 * 0.45 + 0.5) * 255.) as u8,
            ])
        });
        let mesh = Mesh::from_map(&map, Options::default()).unwrap();
        let image = Renderer::default().render(&mesh, Camera::default(), 128, 128, [0; 3]);
        let filled: Vec<_> = image.pixels().filter(|p| p.0[0] != 0).collect();
        assert!(
            filled.len() > 8500 && filled.len() < 10000,
            "{}",
            filled.len()
        );
        let brightest = filled.iter().map(|p| p.0[0]).max().unwrap();
        let darkest = filled.iter().map(|p| p.0[0]).min().unwrap();
        assert!(brightest - darkest > 100);
        assert!(
            mesh.normals
                .iter()
                .zip(&mesh.positions)
                .all(|(n, p)| n.dot(p.sub(mesh.center)) > 0.)
        );
    }
}
