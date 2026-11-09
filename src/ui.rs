use cachecat::indexing::IndexingMode;
use cachecat::library::{ExportFormat, SortOrder};
use cachecat::worker::{Command, Event, Item, SearchMode, Worker};
use gpui::{prelude::*, *};
use gpui_component::{
    Disableable, IconName, Root, Sizable, Theme, ThemeMode, WindowExt,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
    select::{Select, SelectEvent, SelectState},
    slider::{Slider, SliderEvent, SliderState},
    spinner::Spinner,
    switch::Switch,
};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

mod download;
mod icons;
mod previews;
mod sculpt;
mod theme;
use icons::Icon;
use theme::Palette;

const TILE_MIN: f32 = 96.;
const TILE_MAX: f32 = 256.;
const TILE_DEFAULT: f32 = 192.;
const SEMANTIC_SEARCH_HINT: &str =
    "Search objects, scenes, colours, shapes, patterns, materials or text…";

#[derive(Default, Clone, Copy, Serialize, Deserialize, PartialEq)]
enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}
impl Appearance {
    fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    cache: Option<PathBuf>,
    appearance: Appearance,
    sort: SortOrder,
    sidebar_width: f32,
    details_width: f32,
    indexing_mode: IndexingMode,
    tile_size: f32,
    show_partial: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            cache: None,
            appearance: Appearance::System,
            sort: SortOrder::default(),
            sidebar_width: 232.,
            details_width: 320.,
            indexing_mode: IndexingMode::default(),
            tile_size: TILE_DEFAULT,
            show_partial: true,
        }
    }
}
struct CacheCat {
    worker: Worker,
    previews: previews::MemoryPreviews,
    preview_range: std::ops::Range<usize>,
    data: PathBuf,
    settings: Settings,
    search: Entity<InputState>,
    mode_select: Entity<SelectState<Vec<SharedString>>>,
    mode: SearchMode,
    download_dialog: bool,
    download_cancel: Arc<AtomicBool>,
    download_state: Rc<RefCell<download::DownloadState>>,
    theme_select: Entity<SelectState<Vec<SharedString>>>,
    sort_select: Entity<SelectState<Vec<SharedString>>>,
    tile_zoom: Entity<SliderState>,
    zoom_anchor: Option<usize>,
    zoom_changed: Option<Instant>,
    items: Vec<Item>,
    matches: Vec<usize>,
    visible: Vec<usize>,
    selected: Option<usize>,
    generation: u64,
    query: String,
    query_changed: Option<Instant>,
    status: String,
    backend: String,
    copied: bool,
    exporting: bool,
    export_notice: Option<String>,
    export_format: ExportFormat,
    resizing: Option<bool>, // true = left, false = right
    error: Option<String>,
    model_ready: bool,
    downloading: bool,
    paused: bool,
    indexing_mode: IndexingMode,
    gpu_available: Option<bool>,
    columns: usize,
    scroll: UniformListScrollHandle,
    _subscriptions: Vec<Subscription>,
}
impl CacheCat {
    fn new(
        path: Option<PathBuf>,
        data: PathBuf,
        cpu: bool,
        legacy: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings_path = data.join("settings.json");
        let mut settings: Settings = std::fs::read(settings_path)
            .or_else(|error| match &legacy {
                Some(path) => std::fs::read(path.join("settings.json")),
                None => Err(error),
            })
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        settings.tile_size = settings.tile_size.clamp(TILE_MIN, TILE_MAX);
        if path.is_some() {
            settings.cache = path;
        }
        if settings.cache.is_none() {
            settings.cache = cachecat::cache::default_path();
        }
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(SEMANTIC_SEARCH_HINT));
        let mode_select = cx.new(|cx| {
            let mut state = SelectState::new(
                vec![SharedString::from("Semantic"), SharedString::from("UUID")],
                None,
                window,
                cx,
            );
            state.set_selected_value(&SharedString::from("Semantic"), window, cx);
            state
        });
        let themes = [Appearance::System, Appearance::Light, Appearance::Dark];
        let theme_select = cx.new(|cx| {
            let mut state = SelectState::new(
                themes.map(|t| SharedString::from(t.label())).to_vec(),
                None,
                window,
                cx,
            );
            state.set_selected_value(&SharedString::from(settings.appearance.label()), window, cx);
            state
        });
        let sort_select = cx.new(|cx| {
            let mut state = SelectState::new(
                SortOrder::ALL
                    .map(|s| SharedString::from(s.label()))
                    .to_vec(),
                None,
                window,
                cx,
            );
            state.set_selected_value(&SharedString::from(settings.sort.label()), window, cx);
            state
        });
        let tile_zoom = cx.new(|_| {
            SliderState::new()
                .min(TILE_MIN)
                .max(TILE_MAX)
                .step(8.)
                .default_value(settings.tile_size)
        });
        let subscriptions = vec![
            cx.subscribe_in(&tile_zoom, window, |this, _, event, _, cx| {
                let SliderEvent::Change(value) = event;
                this.zoom_anchor
                    .get_or_insert(this.preview_range.start * this.columns);
                this.settings.tile_size = value.start().clamp(TILE_MIN, TILE_MAX);
                this.zoom_changed = Some(Instant::now());
                cx.notify();
            }),
            cx.subscribe_in(&mode_select, window, |this, _, event, window, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    this.mode = if value.as_ref() == "UUID" {
                        SearchMode::Uuid
                    } else {
                        SearchMode::Semantic
                    };
                    this.search.update(cx, |input, cx| {
                        input.set_placeholder(
                            if this.mode == SearchMode::Uuid {
                                "Enter an asset UUID…"
                            } else {
                                SEMANTIC_SEARCH_HINT
                            },
                            window,
                            cx,
                        )
                    });
                    this.use_relevance_for_search(window, cx);
                    this.query_changed = Some(Instant::now());
                    this.matches.clear();
                    this.visible.clear();
                    this.selected = None;
                    cx.notify();
                }
            }),
            cx.subscribe_in(&theme_select, window, |this, _, event, window, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    this.settings.appearance =
                        [Appearance::System, Appearance::Light, Appearance::Dark]
                            .into_iter()
                            .find(|t| t.label() == value.as_ref())
                            .unwrap_or_default();
                    this.apply_theme(window, cx);
                    this.save();
                    cx.notify();
                }
            }),
            cx.subscribe_in(&sort_select, window, |this, _, event, _, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    this.settings.sort = SortOrder::ALL
                        .into_iter()
                        .find(|s| s.label() == value.as_ref())
                        .unwrap_or_default();
                    this.refresh_visible();
                    this.scroll.scroll_to_item(0, ScrollStrategy::Top);
                    this.save();
                    cx.notify();
                }
            }),
            cx.subscribe_in(&search, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::Change | InputEvent::PressEnter { .. }) {
                    this.query = this.search.read(cx).value().to_string();
                    this.use_relevance_for_search(window, cx);
                    this.query_changed = Some(Instant::now());
                    this.visible.clear();
                    this.matches.clear();
                    this.selected = None;
                    this.scroll.scroll_to_item(0, ScrollStrategy::Top);
                    cx.notify();
                }
            }),
            cx.observe_window_appearance(window, |this, window, cx| {
                if this.settings.appearance == Appearance::System {
                    this.apply_theme(window, cx);
                }
                cx.notify();
            }),
        ];
        let mut this = Self {
            worker: Worker::start(data.clone(), cpu, settings.indexing_mode, legacy),
            previews: previews::MemoryPreviews::new(),
            preview_range: 0..0,
            data,
            settings,
            search,
            mode_select,
            mode: SearchMode::Semantic,
            download_dialog: false,
            download_cancel: Arc::new(AtomicBool::new(false)),
            download_state: Rc::new(RefCell::new(download::DownloadState::default())),
            theme_select,
            sort_select,
            tile_zoom,
            zoom_anchor: None,
            zoom_changed: None,
            items: vec![],
            matches: vec![],
            visible: vec![],
            selected: None,
            generation: 0,
            query: String::new(),
            query_changed: None,
            status: "Choose a viewer cache folder".into(),
            backend: String::new(),
            copied: false,
            exporting: false,
            export_notice: None,
            export_format: ExportFormat::default(),
            resizing: None,
            error: None,
            model_ready: false,
            downloading: false,
            paused: false,
            indexing_mode: IndexingMode::Cpu,
            gpu_available: None,
            columns: 4,
            scroll: UniformListScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        this.apply_theme(window, cx);
        if let Some(path) = this.settings.cache.clone() {
            this.open(path, cx);
        }
        cx.spawn_in(window, async move |view, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                if view
                    .update_in(cx, |this, window, cx| this.poll(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        this
    }
    fn use_relevance_for_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.query.trim().is_empty() && self.settings.sort != SortOrder::Relevance {
            self.settings.sort = SortOrder::Relevance;
            self.sort_select.update(cx, |select, cx| {
                select.set_selected_value(
                    &SharedString::from(SortOrder::Relevance.label()),
                    window,
                    cx,
                );
            });
            self.save();
        }
    }

    fn save(&mut self) {
        if let Err(e) = (|| -> anyhow::Result<()> {
            std::fs::create_dir_all(&self.data)?;
            std::fs::write(
                self.data.join("settings.json"),
                serde_json::to_vec_pretty(&self.settings)?,
            )?;
            Ok(())
        })() {
            self.error = Some(format!("Could not save settings: {e}"));
        }
    }
    fn apply_theme(&self, window: &mut Window, cx: &mut App) {
        match self.settings.appearance {
            Appearance::System => Theme::sync_system_appearance(Some(window), cx),
            Appearance::Light => Theme::change(ThemeMode::Light, Some(window), cx),
            Appearance::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
        }
        self.palette(window).apply(Theme::global_mut(cx));
        window.refresh();
    }
    fn palette(&self, window: &Window) -> Palette {
        Palette::new(match self.settings.appearance {
            Appearance::Dark => true,
            Appearance::Light => false,
            Appearance::System => matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
        })
    }
    fn open(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.generation += 1;
        self.items.clear();
        self.preview_range = 0..0;
        self.visible.clear();
        self.matches.clear();
        self.selected = None;
        self.error = None;
        self.status = "Reading texture cache…".into();
        let _ = self
            .worker
            .commands
            .send(Command::ShowPartial(self.settings.show_partial));
        self.settings.cache = Some(path.clone());
        self.save();
        let _ = self
            .worker
            .commands
            .send(Command::Open(self.generation, path));
        let _ = self.worker.commands.send(Command::Search(
            self.generation,
            self.query.clone(),
            self.mode,
        ));
        cx.notify();
    }
    fn choose(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open viewer cache".into()),
        });
        cx.spawn(async move |view, cx| match paths.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.into_iter().next() {
                    let _ = view.update(cx, |this, cx| this.open(path, cx));
                }
            }
            Ok(Err(e)) => {
                let _ = view.update(cx, |this, cx| {
                    this.error = Some(format!("Folder picker: {e}"));
                    cx.notify();
                });
            }
            _ => {}
        })
        .detach();
    }
    fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .zoom_changed
            .is_some_and(|time| time.elapsed() >= Duration::from_millis(300))
        {
            self.zoom_changed = None;
            self.save();
        }
        let mut changed = false;
        let mut sort_changed = false;
        if self
            .query_changed
            .is_some_and(|t| t.elapsed() > Duration::from_millis(250))
        {
            self.query_changed = None;
            if self.mode == SearchMode::Semantic
                && !self.query.trim().is_empty()
                && !self.model_ready
                && !self.download_dialog
            {
                self.show_download(window, cx);
            }
            let _ = self.worker.commands.send(Command::Search(
                self.generation,
                self.query.clone(),
                self.mode,
            ));
        }
        for event in self.worker.events.try_iter().take(512) {
            changed = true;
            match event {
                Event::Loaded(g, items) if g == self.generation => self.items = items,
                Event::Item(g, i, item) if g == self.generation => {
                    if let Some(slot) = self.items.get_mut(i) {
                        sort_changed |= slot.partial != item.partial;
                        sort_changed |= slot.dimensions != item.dimensions
                            && matches!(
                                self.settings.sort,
                                SortOrder::Largest | SortOrder::Smallest
                            );
                        *slot = item;
                    }
                }
                Event::Results(g, q, mode, show_partial, visible)
                    if g == self.generation
                        && q == self.query
                        && mode == self.mode
                        && show_partial == self.settings.show_partial =>
                {
                    self.matches = visible;
                    sort_changed = true;
                }
                Event::Status(s) => self.status = s,
                Event::Backend(backend) => self.backend = backend,
                Event::IndexingMode(mode, gpu) => {
                    self.indexing_mode = mode;
                    self.gpu_available = Some(gpu);
                }
                Event::Error(e) => {
                    self.error = Some(e);
                }
                Event::DownloadProgress(done, total) => {
                    let mut state = self.download_state.borrow_mut();
                    state.done = done;
                    state.total = total;
                }
                Event::DownloadFailed(error) => {
                    self.downloading = false;
                    let mut state = self.download_state.borrow_mut();
                    state.busy = false;
                    state.error = Some(error);
                }
                Event::DownloadCancelled => {
                    self.downloading = false;
                    self.download_state.borrow_mut().busy = false;
                }
                Event::ModelReady(ready) => {
                    self.model_ready = ready;
                    if ready && self.download_dialog {
                        self.download_dialog = false;
                        self.paused = false;
                        window.close_dialog(cx);
                    }
                    if ready || self.error.is_some() {
                        self.downloading = false;
                        let mut state = self.download_state.borrow_mut();
                        state.busy = false;
                        if !ready {
                            state.error = self.error.clone();
                        }
                    }
                }
                _ => {}
            }
        }
        if sort_changed {
            self.refresh_visible();
        }
        let start = (self.preview_range.start * self.columns).min(self.visible.len());
        let end = (self.preview_range.end * self.columns).min(self.visible.len());
        let mut wanted = self.visible[start..end].to_vec();
        if let Some(selected) = self.selected {
            wanted.retain(|&i| i != selected);
            wanted.insert(0, selected);
        }
        let preview_items = wanted
            .into_iter()
            .filter_map(|i| self.items.get(i).map(|item| (i, item.texture.clone())))
            .collect();
        changed |= self.previews.update(
            self.generation,
            self.settings.cache.clone(),
            preview_items,
            window,
            cx,
        );
        if changed {
            if self.download_dialog {
                window.refresh();
            }
            cx.notify();
        }
    }
    fn refresh_visible(&mut self) {
        self.visible = self.settings.sort.sort(&self.items, &self.matches);
        if !self.settings.show_partial {
            self.visible
                .retain(|&i| self.items[i].partial == Some(false));
            if self
                .selected
                .is_some_and(|i| self.items[i].partial != Some(false))
            {
                self.selected = None;
            }
        }
    }

    fn export_selected(&mut self, cx: &mut Context<Self>) {
        if self.exporting {
            return;
        }
        let Some(item) = self.selected.and_then(|i| self.items.get(i)) else {
            return;
        };
        let Some(root) = self.settings.cache.clone() else {
            return;
        };
        let id = item.texture.id;
        let format = self.export_format;
        let suggested = format!("{id}.{}", format.extension());
        let directory = directories::UserDirs::new()
            .and_then(|d| d.download_dir().map(PathBuf::from))
            .unwrap_or_else(|| self.data.clone());
        let path = cx.prompt_for_new_path(&directory, Some(&suggested));
        self.exporting = true;
        self.export_notice = None;
        cx.notify();
        cx.spawn(async move |view, cx| {
            let result = match path.await {
                Ok(Ok(Some(path))) => cx
                    .background_executor()
                    .spawn(async move {
                        cachecat::library::export(&root, id, &path, format).map(|_| path)
                    })
                    .await
                    .map(Some),
                Ok(Ok(None)) => Ok(None),
                Ok(Err(error)) => Err(error),
                Err(error) => Err(error.into()),
            };
            let _ = view.update(cx, |this, cx| {
                this.exporting = false;
                match result {
                    Ok(Some(path)) => {
                        this.export_notice = Some(format!("Saved {}", path.display()))
                    }
                    Ok(None) => {}
                    Err(error) => this.error = Some(format!("Export failed: {error:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn preview_element(&self, i: usize, p: Palette, bounds: Size<Pixels>) -> AnyElement {
        if let Some(Ok(image)) = self.previews.get(i) {
            img(image.clone())
                // Explicit dimensions prevent the image's intrinsic aspect ratio
                // from expanding percentage-sized bounds beyond the preview box.
                .w(bounds.width)
                .h(bounds.height)
                .flex_shrink_0()
                .object_fit(ObjectFit::Contain)
                .into_any_element()
        } else {
            div()
                .w(bounds.width)
                .h(bounds.height)
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgb(p.muted))
                .text_sm()
                .child(
                    if self.previews.get(i).is_some_and(|image| image.is_err()) {
                        Icon::Close
                            .element(p.preview_error)
                            .size(px(28.))
                            .into_any_element()
                    } else {
                        Spinner::new()
                            .with_size(px(24.))
                            .color(rgb(p.muted).into())
                            .into_any_element()
                    },
                )
                .into_any_element()
        }
    }
    fn card(&self, i: usize, tile_width: f32, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.selected == Some(i);
        div()
            .id(i)
            .w(px(tile_width))
            .flex_shrink_0()
            .min_w_0()
            .h(px(tile_width))
            .flex()
            .flex_col()
            .rounded_lg()
            .border_1()
            .border_color(rgb(if selected { p.accent } else { p.line }))
            .bg(rgb(p.panel))
            .overflow_hidden()
            .cursor_pointer()
            .hover(move |s| s.border_color(rgb(p.accent)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected = Some(i);
                this.copied = false;
                this.export_notice = None;
                cx.notify();
            }))
            .child(
                div()
                    .h(px(tile_width - 2.))
                    .w(px(tile_width - 2.))
                    .flex_shrink_0()
                    .p_2()
                    .overflow_hidden()
                    .bg(rgb(p.card))
                    .child(self.preview_element(
                        i,
                        p,
                        size(px(tile_width - 18.), px(tile_width - 18.)),
                    )),
            )
            .into_any_element()
    }
}

fn button(id: &'static str, label: impl Into<SharedString>, p: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_2()
        .min_w_0()
        .flex_shrink_0()
        .rounded_md()
        .border_1()
        .border_color(rgb(p.line))
        .text_sm()
        .cursor_pointer()
        .hover(move |s| s.bg(rgb(p.card)))
        .whitespace_nowrap()
        .child(label.into())
}

fn sidebar_button(
    id: &'static str,
    label: &'static str,
    icon: Icon,
    p: Palette,
    width: f32,
) -> Stateful<Div> {
    div()
        .id(id)
        .w(px(width))
        .min_w_0()
        .h(px(40.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .rounded_md()
        .border_1()
        .border_color(rgb(p.line))
        .text_sm()
        .cursor_pointer()
        .hover(move |s| s.bg(rgb(p.card)))
        .child(icon.element(p.muted))
        .child(div().flex_1().min_w_0().truncate().child(label))
}

mod layout;

pub fn run(path: Option<PathBuf>, data: PathBuf, cpu: bool, legacy: Option<PathBuf>) {
    Application::new()
        .with_assets(icons::Assets)
        .run(move |cx| {
            gpui_component::init(cx);
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let bounds = Bounds::centered(None, size(px(1280.), px(840.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(800.), px(580.))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("CacheCat — Second Life Cache Viewer".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                move |window, cx| {
                    let view = cx.new(|cx| CacheCat::new(path, data, cpu, legacy, window, cx));
                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
            .expect("open CacheCat window");
            cx.activate(true);
        });
}
