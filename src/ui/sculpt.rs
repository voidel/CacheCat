use super::*;
use cachecat::{
    cache,
    sculpt::{Camera, Mesh, Options, Renderer, Topology},
};

impl CacheCat {
    pub(super) fn show_sculpt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.selected.and_then(|i| self.items.get(i)) else {
            return;
        };
        let Some(root) = self.settings.cache.clone() else {
            return;
        };
        let id = item.texture.id;
        let preview = cx.new(|cx| SculptPreview::new(root, id, window, cx));
        window.open_dialog(cx, move |dialog, window, _| {
            let preview = preview.clone();
            let closing = preview.clone();
            dialog
                .title("Sculpt preview")
                .w((window.viewport_size().width - px(48.)).min(px(740.)))
                .margin_top(px(16.))
                .overlay_closable(false)
                .child(preview)
                .on_close(move |_, window, cx| {
                    closing.update(cx, |this, cx| {
                        this.closed = true;
                        this.clear_frame(Some(window), cx);
                    });
                })
        });
    }
}

struct SculptPreview {
    options: Options,
    camera: Camera,
    dragging: Option<Point<Pixels>>,
    frame: Option<Arc<RenderImage>>,
    error: Option<String>,
    loading: bool,
    dirty: bool,
    closed: bool,
    dimensions: (u32, u32),
    background: [u8; 3],
}

impl SculptPreview {
    fn new(root: PathBuf, id: uuid::Uuid, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.on_release(|this, cx| this.clear_frame(None, cx))
            .detach();
        cx.spawn_in(window, async move |view, cx| {
            let decoded = cx
                .background_spawn(async move {
                    let cache = cache::Cache::open(&root)?;
                    let texture = cache.textures.iter().find(|t| t.id == id).ok_or_else(|| {
                        anyhow::anyhow!(
                            "This texture is no longer in the cache. Rescan and try again."
                        )
                    })?;
                    cache::decode(&cache.read(texture)?).map(|image| Arc::new(image.into_rgb8()))
                })
                .await;
            let map = match decoded {
                Ok(map) => map,
                Err(error) => {
                    let _ = view.update_in(cx, |this, _, cx| {
                        this.loading = false;
                        this.error = Some(format!("Could not load this sculpt: {error:#}"));
                        cx.notify();
                    });
                    return;
                }
            };
            let renderer = Arc::new(Renderer::default());
            let mut geometry: Option<(Options, Result<Arc<Mesh>, String>)> = None;
            let mut last = Instant::now();
            loop {
                // One frame at a time, at most 30 fps. Paused views do no raster work.
                let started = Instant::now();
                let request = view.update_in(cx, |this, _, _| {
                    if this.closed {
                        return None;
                    }
                    this.camera.tick(last.elapsed().as_secs_f32());
                    let draw = this.dirty || this.camera.rotating;
                    this.dirty = false;
                    Some((
                        this.options,
                        this.camera,
                        this.dimensions,
                        this.background,
                        draw,
                    ))
                });
                last = Instant::now();
                let Ok(Some((options, camera, (width, height), background, draw))) = request else {
                    break;
                };
                if draw {
                    if geometry
                        .as_ref()
                        .is_none_or(|(previous, _)| *previous != options)
                    {
                        let map = map.clone();
                        let mesh = cx
                            .background_spawn(async move {
                                Mesh::from_map(&map, options)
                                    .map(Arc::new)
                                    .map_err(|e| e.to_string())
                            })
                            .await;
                        geometry = Some((options, mesh));
                    }
                    let result = match &geometry.as_ref().unwrap().1 {
                        Ok(mesh) => {
                            let mesh = mesh.clone();
                            let renderer = renderer.clone();
                            Ok(cx
                                .background_spawn(async move {
                                    let mut image =
                                        renderer.render(&mesh, camera, width, height, background);
                                    // GPUI uploads BGRA, while the renderer produces RGBA.
                                    for pixel in image.pixels_mut() {
                                        pixel.0.swap(0, 2);
                                    }
                                    Arc::new(RenderImage::new(vec![image::Frame::new(image)]))
                                })
                                .await)
                        }
                        Err(error) => Err(error.clone()),
                    };
                    if view
                        .update_in(cx, |this, window, cx| {
                            if this.closed || this.options != options {
                                return;
                            }
                            this.clear_frame(Some(window), cx);
                            this.loading = false;
                            match result {
                                Ok(frame) => {
                                    this.frame = Some(frame);
                                    this.error = None;
                                }
                                Err(error) => {
                                    this.error = Some(error);
                                    this.camera.rotating = false;
                                }
                            }
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                let delay = Duration::from_millis(33).saturating_sub(started.elapsed());
                cx.background_executor().timer(delay).await;
            }
        })
        .detach();
        Self {
            options: Options::default(),
            camera: Camera::default(),
            dragging: None,
            frame: None,
            error: None,
            loading: true,
            dirty: true,
            closed: false,
            dimensions: (640, 400),
            background: [37, 28, 24],
        }
    }

    fn clear_frame(&mut self, window: Option<&mut Window>, cx: &mut App) {
        if let Some(frame) = self.frame.take() {
            cx.drop_image(frame, window);
        }
    }

    fn changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.camera.interact();
        self.clear_frame(Some(window), cx);
        self.error = None;
        self.loading = true;
        self.dirty = true;
        cx.notify();
    }
}

impl Render for SculptPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::new(Theme::global(cx).is_dark());
        let width = (f32::from(window.viewport_size().width) - 96.).clamp(200., 692.);
        let height = (f32::from(window.viewport_size().height) - 260.).clamp(180., 480.);
        let factor = window.scale_factor().min(768. / width.max(height));
        let dimensions = ((width * factor) as u32, (height * factor) as u32);
        let background = [(p.bg >> 16) as u8, (p.bg >> 8) as u8, p.bg as u8];
        if self.dimensions != dimensions || self.background != background {
            self.dimensions = dimensions;
            self.background = background;
            self.dirty = true;
        }
        let mut viewport = div()
            .id("sculpt-viewport")
            .w(px(width))
            .h(px(height))
            .flex_shrink_0()
            .overflow_hidden()
            .rounded_md()
            .bg(rgb(p.bg))
            .flex()
            .items_center()
            .justify_center()
            .cursor(CursorStyle::OpenHand)
            .on_any_mouse_down(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                this.camera.interact();
                this.dragging = (event.button == MouseButton::Left).then_some(event.position);
                cx.stop_propagation();
                cx.notify();
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if !event.dragging() {
                    this.dragging = None;
                    return;
                }
                if let Some(previous) = this.dragging {
                    this.dragging = Some(event.position);
                    this.camera.orbit(
                        f32::from(event.position.x - previous.x),
                        f32::from(event.position.y - previous.y),
                    );
                    this.dirty = true;
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.dragging = None),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.dragging = None),
            )
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                this.camera
                    .zoom_by(f32::from(event.delta.pixel_delta(px(20.)).y));
                this.dirty = true;
                cx.stop_propagation();
                cx.notify();
            }));
        if let Some(frame) = &self.frame {
            viewport = viewport.child(
                img(frame.clone())
                    .w(px(width))
                    .h(px(height))
                    .flex_shrink_0(),
            );
        } else {
            viewport = viewport.child(div().p_6().text_sm().text_color(rgb(p.muted)).child(
                self.error.clone().unwrap_or_else(|| {
                    if self.loading {
                        "Loading sculpt…"
                    } else {
                        "No sculpt preview"
                    }
                    .to_owned()
                }),
            ));
        }
        let topology = self.options.topology;
        let view = cx.weak_entity();
        let toolbar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(
                Button::new("sculpt-topology")
                    .icon(Icon::Cube)
                    .label(topology.label())
                    .dropdown_caret(true)
                    .tooltip("Sculpt stitching type")
                    .w(px(128.))
                    .dropdown_menu(move |mut menu, _, _| {
                        for option in Topology::ALL {
                            let view = view.clone();
                            menu = menu.item(
                                PopupMenuItem::new(option.label())
                                    .checked(option == topology)
                                    .on_click(move |_, window, cx| {
                                        let _ = view.update(cx, |this, cx| {
                                            this.options.topology = option;
                                            this.changed(window, cx);
                                        });
                                    }),
                            );
                        }
                        menu
                    }),
            )
            .child(
                Button::new("sculpt-mirror")
                    .label(if self.options.mirror {
                        "Mirror: on"
                    } else {
                        "Mirror: off"
                    })
                    .tooltip("Mirror the sculpt along its X axis")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.options.mirror = !this.options.mirror;
                        this.changed(window, cx);
                    })),
            )
            .child(
                Button::new("sculpt-invert")
                    .label(if self.options.invert {
                        "Invert: on"
                    } else {
                        "Invert: off"
                    })
                    .tooltip("Reverse the sculpt’s surface direction")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.options.invert = !this.options.invert;
                        this.changed(window, cx);
                    })),
            )
            .child(
                Button::new("sculpt-rotation")
                    .icon(if self.camera.rotating {
                        Icon::Pause
                    } else {
                        Icon::Play
                    })
                    .label(if self.camera.rotating {
                        "Pause"
                    } else {
                        "Rotate"
                    })
                    .disabled(self.error.is_some())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.camera.rotating = !this.camera.rotating;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("sculpt-reset")
                    .icon(Icon::Refresh)
                    .label("Reset view")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.camera = Camera {
                            rotating: false,
                            ..Default::default()
                        };
                        this.dirty = true;
                        cx.notify();
                    })),
            );
        div().w(px(width)).flex().flex_col().gap_3()
            .child(toolbar)
            .child(viewport)
            .child(div().text_xs().text_color(rgb(p.muted)).child("Drag to rotate · Scroll to zoom · Click to stop automatic rotation"))
            .child(div().text_xs().text_color(rgb(p.muted)).child("Choose the sculpt type and flags to match the original object; they aren’t stored in the texture. Object scale is not included."))
    }
}
