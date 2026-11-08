use super::*;

#[derive(Default)]
pub(super) struct DownloadState {
    pub busy: bool,
    pub done: u64,
    pub total: u64,
    pub error: Option<String>,
}

impl CacheCat {
    fn begin_download(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.downloading {
            return;
        }
        self.download_cancel = Arc::new(AtomicBool::new(false));
        self.downloading = true;
        self.error = None;
        *self.download_state.borrow_mut() = DownloadState {
            busy: true,
            total: 213_373_093,
            ..Default::default()
        };
        let _ = self
            .worker
            .commands
            .send(Command::Download(self.download_cancel.clone()));
        cx.notify();
        window.refresh();
    }
    pub(super) fn show_download(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.download_dialog = true;
        let state = self.download_state.clone();
        let view = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, cx| {
            let p = Palette::new(Theme::global(cx).is_dark());
            let state = state.borrow();
            let busy = state.busy;
            let fraction = state.done as f32 / state.total.max(1) as f32;
            let cancel_view = view.clone();
            let download_view = view.clone();
            let confirm_view = view.clone();
            let mut content = div().flex().flex_col().gap_3()
                .child("Semantic search needs a one-time model download (213 MB). Your textures and searches stay on this device.")
                .child(Button::new("model-source").label("View public model download on Hugging Face ↗")
                    .on_click(|_, _, cx| cx.open_url(cachecat::model::MODEL_SOURCE)));
            if busy {
                content = content
                    .child(div().h(px(6.)).w_full().rounded_full().bg(rgb(p.card))
                        .child(div().h_full().w(relative(fraction)).rounded_full().bg(rgb(p.accent))))
                    .child(if state.total > 0 && state.done == state.total {
                        "Download complete. Loading model…".to_owned()
                    } else {
                        format!("{:.1} / {:.1} MB · {}%", state.done as f64 / 1e6, state.total as f64 / 1e6, (fraction * 100.) as u32)
                    });
            }
            if let Some(error) = &state.error { content = content.child(error.clone()); }
            dialog.title("Download search model").w(px(460.)).overlay_closable(false)
                .child(content)
                .on_ok(move |_, window, cx| {
                    let _ = confirm_view.update(cx, |this, cx| this.begin_download(window, cx));
                    false
                })
                .on_cancel(move |_, _, cx| {
                    let _ = cancel_view.update(cx, |this, cx| {
                        this.download_cancel.store(true, Ordering::Relaxed);
                        this.download_dialog = false;
                        cx.notify();
                    });
                    true
                })
                .footer(move |_, cancel, window, cx| vec![
                    cancel(window, cx),
                    Button::new("download-model").primary().label(if busy { "Downloading…" } else { "Download model" })
                        .disabled(busy)
                        .on_click({
                            let view = download_view.clone();
                            move |_, window, cx| {
                                let _ = view.update(cx, |this, cx| {
                                    this.begin_download(window, cx);
                                });
                                window.refresh();
                            }
                        }).into_any_element(),
                ])
        });
    }
}
