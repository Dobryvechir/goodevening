//! Quick view panel (Ctrl+Q, `qview.cpp`): the viewer core in a panel,
//! or folder totals computed in the background.

use crate::filepanel::FilePanel;
use gm_i18n::{MsgId, msg};
use gm_term::{Buffer, PaletteId, Rect, style};
use gm_text::Codepage;
use gm_ui::draw;
use gm_viewer::ViewerCore;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct QuickView {
    shown: Option<PathBuf>,
    viewer: Option<ViewerCore>,
    dir: Option<Arc<Mutex<(gm_fs_size::Size, bool)>>>,
    text: Option<String>,
}

/// Folder totals shared with the background worker.
pub mod gm_fs_size {
    #[derive(Clone, Copy, Debug, Default)]
    pub struct Size {
        pub files: u64,
        pub dirs: u64,
        pub bytes: u64,
    }
}

impl QuickView {
    /// Follows the cursor of the other panel. Returns true if the content changed.
    pub fn update(&mut self, other: &mut FilePanel) -> bool {
        let Some(e) = other.current().cloned() else {
            let changed = self.shown.is_some();
            *self = QuickView::default();
            return changed;
        };
        let key = match other.local_dir() {
            Some(d) => d.join(&e.name),
            None => PathBuf::from(format!("{}/{}", other.display_dir(), e.name)),
        };
        if self.shown.as_ref() == Some(&key) {
            return false;
        }
        self.shown = Some(key.clone());
        self.viewer = None;
        self.dir = None;
        self.text = None;
        if e.is_dotdot() {
            return true;
        }
        if other.is_local() {
            if e.is_dir {
                let shared = Arc::new(Mutex::new((gm_fs_size::Size::default(), false)));
                let s2 = shared.clone();
                std::thread::spawn(move || {
                    let mut acc = gm_fs_size::Size::default();
                    let mut stack = vec![key];
                    while let Some(d) = stack.pop() {
                        // Stop when nobody looks at the result any more.
                        if Arc::strong_count(&s2) == 1 {
                            return;
                        }
                        let Ok(rd) = std::fs::read_dir(&d) else { continue };
                        for e in rd.flatten() {
                            match e.file_type() {
                                Ok(t) if t.is_dir() => {
                                    acc.dirs += 1;
                                    stack.push(e.path());
                                }
                                Ok(_) => {
                                    acc.files += 1;
                                    acc.bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
                                }
                                Err(_) => {}
                            }
                        }
                        if let Ok(mut g) = s2.lock() {
                            g.0 = acc;
                        }
                    }
                    if let Ok(mut g) = s2.lock() {
                        *g = (acc, true);
                    }
                });
                self.dir = Some(shared);
            } else {
                self.viewer = ViewerCore::open(&key, true, Codepage::Utf8).ok();
            }
        } else if let Some(t) = other.provider_mut().describe(&e.name) {
            self.text = Some(t);
        } else if !e.is_dir
            && let Ok(mut r) = other.provider_mut().open_read(&e.name)
        {
            let mut v = Vec::new();
            let _ = std::io::Read::read_to_end(&mut std::io::Read::take(&mut r, 256 * 1024), &mut v);
            self.viewer = Some(ViewerCore::from_bytes(PathBuf::from(&e.name), v, true, Codepage::Utf8));
        }
        true
    }

    /// True while a folder scan is running (needs periodic redraws).
    pub fn busy(&self) -> bool {
        self.dir.as_ref().is_some_and(|d| d.lock().map(|g| !g.1).unwrap_or(false))
    }

    pub fn render(&mut self, buf: &mut Buffer, r: Rect, active: bool) {
        let text = style(PaletteId::PanelText);
        let info = style(PaletteId::PanelTextInfo);
        let boxs = style(PaletteId::PanelBox);
        draw::fill(buf, r, ' ', text);
        draw::frame(buf, r, true, boxs, None);
        let title = style(if active { PaletteId::PanelTitleSelected } else { PaletteId::PanelTitle });
        draw::title_at(buf, r, r.y, msg(MsgId::MQuickViewTitle), title);
        let inner = Rect::new(r.x + 1, r.y + 1, r.width - 2, r.height.saturating_sub(4));
        if let Some(v) = &mut self.viewer {
            v.render(buf, inner, false);
            // The viewer paints its own colors; keep the panel look.
        } else if let Some(d) = &self.dir {
            let (s, done) = d.lock().map(|g| *g).unwrap_or_default();
            let rows = [
                (msg(MsgId::MQuickViewFolders), draw::group_digits(s.dirs)),
                (msg(MsgId::MQuickViewFiles), draw::group_digits(s.files)),
                (msg(MsgId::MQuickViewBytes), draw::group_digits(s.bytes)),
            ];
            for (i, (k, v)) in rows.iter().enumerate() {
                let y = inner.y + 2 + i as u16;
                draw::put(buf, inner.x + 2, y, k, text, 20);
                draw::put(buf, inner.x + 22, y, v, info, (inner.width as usize).saturating_sub(22));
            }
            if !done {
                draw::put(buf, inner.x + 2, inner.y + 6, "…", info, 3);
            }
        } else if let Some(t) = &self.text {
            for (i, l) in t.lines().take(inner.height as usize).enumerate() {
                draw::put_fit(buf, inner.x, inner.y + i as u16, l, text, inner.width as usize);
            }
        }
        draw::separator(buf, r.x, r.bottom() - 3, r.width, true, boxs);
        if let Some(p) = &self.shown {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            draw::put_fit(buf, r.x + 1, r.bottom() - 2, &name, text, (r.width - 2) as usize);
        }
    }
}
