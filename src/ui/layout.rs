use super::*;

fn grid_geometry(width: f32, requested: f32) -> (usize, f32) {
    let tile = requested.clamp(TILE_MIN, TILE_MAX).min(width.max(1.));
    let columns = (((width + 12.) / (tile + 12.)).floor() as usize).max(1);
    (columns, tile)
}

fn widths(width: f32, left: f32, right: f32, details: bool) -> (f32, f32) {
    let right = if details {
        right
            .clamp(280., 520.)
            .min((width - 200. - 280. - 12.).max(280.))
    } else {
        0.
    };
    let left = left
        .clamp(200., 400.)
        .min((width - right - 280. - 12.).max(200.));
    (left, right)
}

impl CacheCat {
    fn resize_handle(&self, left: bool, p: Palette, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id(if left {
                "resize-sidebar"
            } else {
                "resize-details"
            })
            .w(px(6.))
            .h_full()
            .flex_shrink_0()
            .bg(rgb(p.line))
            .cursor(CursorStyle::ResizeLeftRight)
            .hover(move |s| s.bg(rgb(p.accent)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.resizing = Some(left);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
    }

    fn details_panel(&self, right: f32, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let item = &self.items[self.selected.unwrap()];
        let id = item.texture.id.to_string();
        let copy_id = id.clone();
        let view = cx.entity().downgrade();
        let format = self.export_format;
        let mut panel = div()
            .w(px(right))
            .min_h_full()
            .p_4()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .w(px(right - 32.))
                    .flex()
                    .justify_between()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Texture details"),
                    )
                    .child(
                        button("close", "Close", p).on_click(cx.listener(|this, _, _, cx| {
                            this.selected = None;
                            cx.notify();
                        })),
                    ),
            )
            .child(
                div()
                    .w(px(right - 32.))
                    .h(px((right - 32.).min(340.)))
                    .flex_shrink_0()
                    .bg(rgb(p.card))
                    .rounded_lg()
                    .overflow_hidden()
                    .child(self.preview_element(
                        self.selected.unwrap(),
                        p,
                        size(px(right - 32.), px((right - 32.).min(340.))),
                    )),
            )
            .child(div().text_xs().text_color(rgb(p.muted)).child("ASSET UUID"))
            .child(
                div()
                    .id("asset-uuid")
                    .w(px(right - 32.))
                    .overflow_x_scroll()
                    .text_size(px(11.))
                    .font_family("monospace")
                    .whitespace_nowrap()
                    .child(id),
            )
            .child(
                sidebar_button(
                    "copy",
                    if self.copied { "Copied" } else { "Copy UUID" },
                    Icon::Copy,
                    p,
                    right - 32.,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy_id.clone()));
                    this.copied = true;
                    cx.notify();
                })),
            )
            .child(
                sidebar_button("view-sculpt", "View as sculpt", Icon::Cube, p, right - 32.)
                    .on_click(cx.listener(|this, _, window, cx| this.show_sculpt(window, cx))),
            )
            .child(
                div()
                    .flex()
                    .w(px(right - 32.))
                    .items_center()
                    .child(
                        Button::new("export-texture")
                            .label(if self.exporting {
                                "Saving…".into()
                            } else {
                                format!("Download {}", format.label())
                            })
                            .icon(Icon::Download)
                            .disabled(self.exporting)
                            .w(px(right - 68.))
                            .rounded_r_none()
                            .on_click(cx.listener(|this, _, _, cx| this.export_selected(cx))),
                    )
                    .child(
                        Button::new("export-format")
                            .icon(IconName::ChevronDown)
                            .tooltip("Download format")
                            .disabled(self.exporting)
                            .rounded_l_none()
                            .w(px(36.))
                            .dropdown_menu(move |mut menu, _, _| {
                                for option in [ExportFormat::Png, ExportFormat::Tga] {
                                    let view = view.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(option.label())
                                            .checked(option == format)
                                            .on_click(move |_, _, cx| {
                                                let _ = view.update(cx, |this, cx| {
                                                    this.export_format = option;
                                                    cx.notify();
                                                });
                                            }),
                                    );
                                }
                                menu
                            }),
                    ),
            )
            .when_some(item.dimensions, |s, (w, h)| {
                s.child(
                    div()
                        .text_sm()
                        .text_color(rgb(p.muted))
                        .child(format!("{w} × {h} pixels")),
                )
            })
            .child(div().text_sm().text_color(rgb(p.muted)).child(format!(
                "Cached data: {:.1} KB",
                (item.texture.body_size + item.texture.image_size.min(600)) as f64 / 1024.
            )))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(p.muted))
                    .child(if item.indexed {
                        "Available in semantic search"
                    } else {
                        "Not yet indexed"
                    }),
            );
        if let Some(message) = &self.export_notice {
            panel = panel.child(div().text_xs().child(message.clone()));
        }
        if let Some(error) = &item.error {
            panel = panel.child(
                div()
                    .text_xs()
                    .text_color(rgb(p.muted))
                    .child(error.clone()),
            );
        }
        div()
            .id("details-scroll")
            .w(px(right))
            .h_full()
            .flex_shrink_0()
            .overflow_y_scroll()
            .bg(rgb(p.panel))
            .child(panel)
            .into_any_element()
    }
}

impl Render for CacheCat {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette(window);
        let width = f32::from(window.viewport_size().width);
        let details = self.selected.is_some_and(|i| i < self.items.len());
        let (left, right) = widths(
            width,
            self.settings.sidebar_width,
            self.settings.details_width,
            details,
        );
        let grid_width = width - left - right - if details { 12. } else { 6. } - 40.;
        let (columns, tile_width) = grid_geometry(grid_width, self.settings.tile_size);
        self.columns = columns;
        if let Some(anchor) = self.zoom_anchor.take() {
            self.scroll.scroll_to_item_strict(
                anchor.min(self.visible.len().saturating_sub(1)) / columns,
                ScrollStrategy::Top,
            );
        }
        let indexed = self.items.iter().filter(|i| i.indexed).count();
        let unavailable = self
            .items
            .iter()
            .filter(|i| i.error.is_some() && !i.indexed)
            .count();
        let pending = self.items.len().saturating_sub(indexed + unavailable);
        let semantic = !self.query.trim().is_empty() && self.mode == SearchMode::Semantic;
        let folder = self
            .settings
            .cache
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "No cache selected".into());
        window.set_window_title(&format!("CacheCat — {folder}"));

        let mut sidebar = div()
            .w(px(left))
            .min_h_full()
            .p_4()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .w_full()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(Icon::Cat.element(p.accent).size(px(32.)))
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::BOLD)
                            .child("CacheCat"),
                    ),
            )
            .child(
                sidebar_button("choose", "Open cache folder", Icon::Folder, p, left - 32.)
                    .on_click(cx.listener(Self::choose)),
            )
            .child(
                sidebar_button("rescan", "Rescan cache", Icon::Refresh, p, left - 32.).on_click(
                    cx.listener(|this, _, _, cx| {
                        if let Some(path) = this.settings.cache.clone() {
                            this.open(path, cx);
                        }
                    }),
                ),
            );
        sidebar = sidebar
            .child(
                sidebar_button(
                    "pause",
                    if self.paused {
                        "Resume indexing"
                    } else {
                        "Pause indexing"
                    },
                    if self.paused { Icon::Play } else { Icon::Pause },
                    p,
                    left - 32.,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.paused = !this.paused;
                    let _ = this.worker.commands.send(Command::Pause(this.paused));
                    cx.notify();
                })),
            )
            .child(
                div()
                    .w_full()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_sm()
                            .text_color(rgb(p.muted))
                            .child(Icon::Processor.element(p.muted))
                            .child("Indexing device"),
                    )
                    .child(
                        Button::new("indexing-device")
                            .w_full()
                            .h(px(32.))
                            .px_3()
                            .child(
                                div()
                                    .w(px(left - 58.))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(self.indexing_mode.label())
                                    .child(
                                        gpui_component::Icon::new(IconName::ChevronDown)
                                            .size(px(14.)),
                                    ),
                            )
                            .tooltip("Choose the devices used for indexing")
                            .dropdown_menu({
                                let view = cx.entity().downgrade();
                                move |mut menu, _, cx| {
                                    let Some(entity) = view.upgrade() else {
                                        return menu;
                                    };
                                    let this = entity.read(cx);
                                    let selected = this.indexing_mode;
                                    let gpu = this.gpu_available == Some(true);
                                    menu = menu.min_w(px(left - 32.));
                                    for mode in IndexingMode::ALL {
                                        let view = view.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new(mode.label())
                                                .checked(mode == selected)
                                                .disabled(!mode.available(gpu))
                                                .on_click(move |_, _, cx| {
                                                    let _ = view.update(cx, |this, cx| {
                                                        if mode.available(
                                                            this.gpu_available == Some(true),
                                                        ) {
                                                            this.indexing_mode = mode;
                                                            this.settings.indexing_mode = mode;
                                                            let _ = this
                                                                .worker
                                                                .commands
                                                                .send(Command::IndexingMode(mode));
                                                            this.save();
                                                            cx.notify();
                                                        }
                                                    });
                                                }),
                                        );
                                    }
                                    menu
                                }
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .flex_shrink_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_sm()
                            .text_color(rgb(p.muted))
                            .child(Icon::Monitor.element(p.muted))
                            .child("Theme"),
                    )
                    .child(
                        div()
                            .w_full()
                            .h(px(38.))
                            .child(Select::new(&self.theme_select)),
                    ),
            )
            .child(div().flex_1().min_h(px(16.)))
            .child(
                Button::new("github-project")
                    .link()
                    .label("GitHub")
                    .icon(Icon::ExternalLink)
                    .text_color(rgb(p.muted))
                    .justify_start()
                    .tooltip("Open CacheCat on GitHub")
                    .on_click(|_, _, cx| cx.open_url("https://github.com/voidel/CacheCat")),
            );

        let mut main = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .p_5()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Browse cache"),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .flex_shrink_0()
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.search)
                                .h(px(44.))
                                .prefix(Icon::Search.element(p.muted)),
                        ),
                    )
                    .child(
                        div()
                            .w(px(135.))
                            .flex_shrink_0()
                            .child(Select::new(&self.mode_select)),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .flex_shrink_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .flex_1()
                            .min_w(px(180.))
                            .max_w(px(310.))
                            .child(Icon::Sort.element(p.muted))
                            .child(div().text_sm().text_color(rgb(p.muted)).child("Sort"))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .h(px(36.))
                                    .child(Select::new(&self.sort_select)),
                            ),
                    )
                    .child(
                        div()
                            .w(px(220.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().text_sm().text_color(rgb(p.muted)).child("Zoom"))
                            .child(Icon::ZoomOut.element(p.muted))
                            .child(
                                div().flex_1().min_w_0().child(
                                    Slider::new(&self.tile_zoom)
                                        .horizontal()
                                        .bg(rgb(p.accent))
                                        .text_color(rgb(p.accent)),
                                ),
                            )
                            .child(Icon::ZoomIn.element(p.muted)),
                    ),
            );
        if let Some(error) = &self.error {
            main = main.child(
                div()
                    .w_full()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(p.card))
                    .text_sm()
                    .child(error.clone())
                    .child(button("dismiss", "Dismiss", p).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.error = None;
                            cx.notify();
                        },
                    ))),
            );
        }
        if semantic && self.model_ready && pending > 0 {
            main = main.child(div().w_full().px_3().py_2().rounded_md().bg(rgb(p.card)).text_sm().text_color(rgb(p.accent_label))
                .child("Indexing is not complete yet — results may be partial. New matches appear as textures are indexed."));
        }
        if self.visible.is_empty() {
            self.preview_range = 0..0;
            let message = if self.items.is_empty() {
                "Open a texture cache"
            } else if semantic && !self.model_ready {
                "Download the model to search descriptions"
            } else if semantic && indexed == 0 {
                "Results will appear as textures are indexed"
            } else {
                "No matching textures"
            };
            main = main.child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(p.muted))
                    .child(message),
            );
        } else {
            main = main.child(
                uniform_list(
                    "texture-grid",
                    self.visible.len().div_ceil(self.columns),
                    cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                        // Use the final virtual-list viewport on the next poll, not
                        // its temporary single-row measurement pass.
                        this.preview_range = range.clone();
                        range
                            .map(|row| {
                                let start = row * this.columns;
                                let end = (start + this.columns).min(this.visible.len());
                                let mut line =
                                    div().w_full().h(px(tile_width + 12.)).flex().gap_3().pb_3();
                                for j in start..end {
                                    line =
                                        line.child(this.card(this.visible[j], tile_width, p, cx));
                                }
                                line
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(self.scroll.clone())
                .flex_1()
                .min_h_0(),
            );
        }
        let status = if self.paused {
            "Indexing paused"
        } else if !self.items.is_empty() && pending == 0 {
            "Indexing complete"
        } else if self.model_ready && !self.items.is_empty() {
            "Indexing textures…"
        } else {
            &self.status
        };
        let mut counts = div()
            .w_full()
            .flex()
            .flex_wrap()
            .gap_x_4()
            .gap_y_1()
            .text_xs()
            .text_color(rgb(p.muted))
            .child(format!("{indexed} / {} indexed", self.items.len()))
            .child(format!("{pending} pending"))
            .child(format!("{unavailable} unavailable"));
        if !self.query.trim().is_empty() {
            counts = counts.child(format!("{} matches", self.visible.len()));
        }
        main = main.child(
            div()
                .w_full()
                .flex_shrink_0()
                .pt_3()
                .border_t_1()
                .border_color(rgb(p.line))
                .flex()
                .flex_col()
                .gap_2()
                .child(counts)
                .child(
                    div()
                        .w_full()
                        .h(px(6.))
                        .rounded_full()
                        .flex()
                        .overflow_hidden()
                        .bg(rgb(p.card))
                        .child(
                            div()
                                .h_full()
                                .flex_shrink_0()
                                .w(relative(indexed as f32 / self.items.len().max(1) as f32))
                                .bg(rgb(p.accent)),
                        )
                        .child(
                            div()
                                .h_full()
                                .flex_shrink_0()
                                .w(relative(
                                    unavailable as f32 / self.items.len().max(1) as f32,
                                ))
                                .bg(rgb(p.progress_unavailable)),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_3()
                        .text_xs()
                        .text_color(rgb(p.muted))
                        .child(status.to_owned())
                        .child(self.backend.clone()),
                ),
        );

        let mut layout = div()
            .id("cachecat-layout")
            .size_full()
            .flex()
            .bg(rgb(p.bg))
            .text_color(rgb(p.text))
            .font_family(if cfg!(target_os = "windows") {
                "Segoe UI"
            } else {
                "sans-serif"
            })
            .on_mouse_move(
                cx.listener(move |this, event: &MouseMoveEvent, window, cx| {
                    if let Some(is_left) = this.resizing {
                        if event.pressed_button != Some(MouseButton::Left) {
                            this.resizing = None;
                            this.save();
                            return;
                        }
                        let width = f32::from(window.viewport_size().width);
                        let x = f32::from(event.position.x);
                        if is_left {
                            this.settings.sidebar_width =
                                x.clamp(200., (width - right - 292.).clamp(200., 400.));
                        } else {
                            this.settings.details_width =
                                (width - x).clamp(280., (width - left - 292.).clamp(280., 520.));
                        }
                        cx.notify();
                    }
                }),
            )
            .capture_any_mouse_up(cx.listener(|this, _, _, cx| {
                if this.resizing.take().is_some() {
                    this.save();
                    cx.notify();
                }
            }))
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.resizing.take().is_some() {
                        this.save();
                        cx.notify();
                    }
                }),
            )
            .child(
                div()
                    .id("sidebar-scroll")
                    .w(px(left))
                    .h_full()
                    .flex_shrink_0()
                    .overflow_y_scroll()
                    .bg(rgb(p.panel))
                    .child(sidebar),
            )
            .child(self.resize_handle(true, p, cx))
            .child(main);
        if details {
            layout = layout
                .child(self.resize_handle(false, p, cx))
                .child(self.details_panel(right, p, cx));
        }
        layout.children(Root::render_dialog_layer(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn zoom_tiles_fit_each_row_without_exceeding_thumbnail_size() {
        for width in [80., 240., 368., 1000., 1920.] {
            for requested in [TILE_MIN, TILE_DEFAULT, TILE_MAX, 500.] {
                let (columns, tile) = grid_geometry(width, requested);
                assert!(tile <= 256. && tile <= width);
                assert!(columns as f32 * tile + (columns - 1) as f32 * 12. <= width);
                assert!((columns + 1) as f32 * tile + columns as f32 * 12. > width);
            }
        }
        assert!(grid_geometry(1000., TILE_MIN).0 > grid_geometry(1000., TILE_MAX).0);
    }

    #[::core::prelude::v1::test]
    fn panels_leave_room_for_the_grid_at_minimum_window_size() {
        for window in [800., 1000., 1280., 1920.] {
            for left in [200., 232., 400.] {
                for right in [280., 320., 520.] {
                    let (left, right) = widths(window, left, right, true);
                    assert!(left >= 200. && right >= 280.);
                    assert!(window - left - right - 12. >= 280.);
                }
            }
        }
    }
    #[::core::prelude::v1::test]
    fn existing_settings_keep_theme_and_gain_panel_defaults() {
        let settings: Settings =
            serde_json::from_str(r#"{"cache":"custom-cache-folder","appearance":"Dark"}"#).unwrap();
        assert_eq!(settings.cache, Some(PathBuf::from("custom-cache-folder")));
        assert!(settings.appearance == Appearance::Dark);
        assert_eq!(settings.sidebar_width, 232.);
        assert_eq!(settings.details_width, 320.);
        assert_eq!(settings.indexing_mode, IndexingMode::GpuCpu);
        assert_eq!(settings.tile_size, TILE_DEFAULT);
    }
}
