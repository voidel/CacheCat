use gpui::{AssetSource, SharedString, Styled, Svg, px, rgb, svg};
use std::borrow::Cow;

/// Small monochrome SVGs embedded in the executable, independent of system fonts.
#[derive(Clone, Copy)]
pub enum Icon {
    Cat,
    Copy,
    Folder,
    Refresh,
    Download,
    Pause,
    Play,
    Processor,
    Monitor,
    Search,
    Sort,
    ZoomOut,
    ZoomIn,
}

impl Icon {
    fn name(self) -> &'static str {
        match self {
            Self::Cat => "cat",
            Self::Copy => "copy",
            Self::Folder => "folder",
            Self::Refresh => "refresh",
            Self::Download => "download",
            Self::Pause => "pause",
            Self::Play => "play",
            Self::Processor => "processor",
            Self::Monitor => "monitor",
            Self::Search => "search",
            Self::Sort => "sort",
            Self::ZoomOut => "zoom-out",
            Self::ZoomIn => "zoom-in",
        }
    }

    pub fn element(self, color: u32) -> Svg {
        svg()
            .path(format!("cachecat/icons/{}", self.name()))
            // Raw GPUI SVGs require their own colour to paint; parent text
            // colour alone leaves the icon blank.
            .text_color(rgb(color))
            .size(px(18.))
            .flex_shrink_0()
    }
}

impl gpui_component::IconNamed for Icon {
    fn path(self) -> SharedString {
        format!("cachecat/icons/{}", self.name()).into()
    }
}

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let name = path.strip_prefix("cachecat/icons/").or_else(|| {
            path.strip_prefix("icons/")
                .and_then(|p| p.strip_suffix(".svg"))
        });
        let shape = match name {
            Some("copy") => {
                "<rect x='8' y='8' width='13' height='13' rx='2'/><path d='M16 8V3H3v13h5'/>"
            }
            Some("chevron-down") => "<path d='m6 9 6 6 6-6'/>",
            Some("check") => "<path d='m4 12 5 5L20 6'/>",
            Some("close") => "<path d='m6 6 12 12M6 18 18 6'/>",
            Some("cat") => {
                "<path d='m4 10-1-7 7 4h4l7-4-1 7c3 8-1 11-8 11S1 18 4 10Z'/><path d='M7 13h1m8 0h1m-7 4 2 1 2-1M1 14l4 1m14 0 4-1M2 19h3m14 0h3'/>"
            }
            Some("library") => {
                "<rect x='3' y='3' width='7' height='7' rx='1'/><rect x='14' y='3' width='7' height='7' rx='1'/><rect x='3' y='14' width='7' height='7' rx='1'/><rect x='14' y='14' width='7' height='7' rx='1'/>"
            }
            Some("folder") => "<path d='M3 7V5h7l2 3h9v11H3V7Z'/><path d='M3 10h18'/>",
            Some("refresh") => {
                "<path d='M20 9a8 8 0 0 0-14-3L3 9m0-6v6h6M4 15a8 8 0 0 0 14 3l3-3m0 6v-6h-6'/>"
            }
            Some("search") => "<circle cx='10' cy='10' r='6'/><path d='m15 15 6 6'/>",
            Some("sort") => "<path d='M3 5h11M3 10h8M3 15h5M18 4v16m-4-4 4 4 4-4'/>",
            Some("zoom-out") => "<circle cx='10' cy='10' r='6'/><path d='M7 10h6m2 5 6 6'/>",
            Some("zoom-in") => "<circle cx='10' cy='10' r='6'/><path d='M7 10h6m-3-3v6m5 2 6 6'/>",
            Some("download") => "<path d='M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5'/>",
            Some("pause") => {
                "<rect x='5' y='4' width='4' height='16' rx='1'/><rect x='15' y='4' width='4' height='16' rx='1'/>"
            }
            Some("play") => "<path d='m6 3 15 9-15 9Z'/>",
            Some("processor") => {
                "<rect x='6' y='6' width='12' height='12' rx='2'/><rect x='9' y='9' width='6' height='6' rx='1'/><path d='M9 3v3m6-3v3M9 18v3m6-3v3M3 9h3m-3 6h3m12-6h3m-3 6h3'/>"
            }
            Some("monitor") => {
                "<rect x='2' y='3' width='20' height='14' rx='2'/><path d='M12 17v4m-5 0h10'/>"
            }
            Some("sun") => {
                "<circle cx='12' cy='12' r='4'/><path d='M12 1v2m0 18v2M1 12h2m18 0h2M4 4l2 2m12 12 2 2M4 20l2-2M18 6l2-2'/>"
            }
            Some("moon") => "<path d='M21 13A9 9 0 0 1 11 3a9 9 0 1 0 10 10Z'/>",
            _ => return Ok(None),
        };
        Ok(Some(Cow::Owned(format!(
            "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='black' stroke-width='1.7' stroke-linecap='round' stroke-linejoin='round'>{shape}</svg>"
        ).into_bytes())))
    }

    fn list(&self, _: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}
