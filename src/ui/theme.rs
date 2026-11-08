use gpui::rgb;
use gpui_component::{Theme, ThemeColor};

/// pom.zone's coral and warm-neutral tokens, shared by our custom UI and GPUI controls.
#[derive(Clone, Copy)]
pub(super) struct Palette {
    pub bg: u32,
    pub panel: u32,
    pub card: u32,
    pub line: u32,
    pub text: u32,
    pub muted: u32,
    pub accent: u32,
    pub accent_label: u32,
    pub progress_unavailable: u32,
    accent_text: u32,
    accent_hover: u32,
    accent_active: u32,
    selected: u32,
    selection: u32,
}

impl Palette {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                bg: 0x251c18,
                panel: 0x30241f,
                card: 0x3c2d27,
                line: 0x674b3d,
                text: 0xfaf9f8,
                muted: 0xc6b1a3,
                accent: 0xff9865,
                accent_label: 0xff9865,
                progress_unavailable: 0x8c8c8c,
                accent_text: 0x251c18,
                accent_hover: 0xffb68e,
                accent_active: 0xffdcc9,
                selected: 0x503b32,
                selection: 0x674b3d,
            }
        } else {
            Self {
                bg: 0xfafafa,
                panel: 0xffffff,
                card: 0xf2efed,
                line: 0xe5e0dc,
                text: 0x342b29,
                muted: 0x78675e,
                accent: 0xc74629,
                accent_label: 0xaa3520,
                progress_unavailable: 0x929292,
                accent_text: 0xffffff,
                accent_hover: 0xaa3520,
                accent_active: 0x8d2c1d,
                selected: 0xfff0e8,
                selection: 0xf3c9ba,
            }
        }
    }

    /// Reapply after Theme::change, which restores the library's default colors.
    /// Status colors (errors, warnings and success) keep their semantic meanings.
    pub fn apply(self, theme: &mut Theme) {
        theme.colors = ThemeColor {
            background: rgb(self.panel).into(),
            foreground: rgb(self.text).into(),
            border: rgb(self.line).into(),
            input: rgb(self.line).into(),
            muted: rgb(self.card).into(),
            muted_foreground: rgb(self.muted).into(),
            accent: rgb(self.selected).into(),
            accent_foreground: rgb(self.text).into(),
            primary: rgb(self.accent).into(),
            primary_foreground: rgb(self.accent_text).into(),
            primary_hover: rgb(self.accent_hover).into(),
            primary_active: rgb(self.accent_active).into(),
            secondary: rgb(self.card).into(),
            secondary_foreground: rgb(self.text).into(),
            secondary_hover: rgb(self.selected).into(),
            secondary_active: rgb(self.selection).into(),
            popover: rgb(self.panel).into(),
            popover_foreground: rgb(self.text).into(),
            list: rgb(self.panel).into(),
            list_head: rgb(self.card).into(),
            list_even: rgb(self.panel).into(),
            list_hover: rgb(self.card).into(),
            list_active: rgb(self.selected).into(),
            list_active_border: rgb(self.accent).into(),
            selection: rgb(self.selection).into(),
            ring: rgb(self.accent).into(),
            caret: rgb(self.accent).into(),
            link: rgb(self.accent_label).into(),
            link_hover: rgb(self.accent_hover).into(),
            link_active: rgb(self.accent_active).into(),
            progress_bar: rgb(self.accent).into(),
            scrollbar: rgb(self.panel).into(),
            scrollbar_thumb: rgb(self.line).into(),
            scrollbar_thumb_hover: rgb(self.muted).into(),
            sidebar: rgb(self.panel).into(),
            sidebar_foreground: rgb(self.text).into(),
            sidebar_border: rgb(self.line).into(),
            sidebar_accent: rgb(self.selected).into(),
            sidebar_accent_foreground: rgb(self.text).into(),
            sidebar_primary: rgb(self.accent).into(),
            sidebar_primary_foreground: rgb(self.accent_text).into(),
            title_bar: rgb(self.panel).into(),
            title_bar_border: rgb(self.line).into(),
            window_border: rgb(self.line).into(),
            ..theme.colors
        };
    }
}
