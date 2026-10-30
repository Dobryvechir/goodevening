//! Information panel (Ctrl+L, `infolist.cpp`).

use crate::filepanel::FilePanel;
use gm_i18n::{MsgId, msg, plain};
use gm_term::{Buffer, PaletteId, Rect, style};
use gm_ui::draw;

pub fn render(buf: &mut Buffer, r: Rect, other: &FilePanel, active: bool) {
    let text = style(PaletteId::PanelText);
    let info = style(PaletteId::PanelTextInfo);
    let boxs = style(PaletteId::PanelBox);
    draw::fill(buf, r, ' ', text);
    draw::frame(buf, r, true, boxs, None);
    let title = style(if active { PaletteId::PanelTitleSelected } else { PaletteId::PanelTitle });
    draw::title_at(buf, r, r.y, msg(MsgId::MInfoTitle), title);
    let w = (r.width - 2) as usize;
    let mut y = r.y + 1;
    let bottom = r.bottom() - 1;
    let line = |buf: &mut Buffer, y: &mut u16, k: &str, v: &str| {
        if *y >= bottom {
            return;
        }
        let kw = w / 2;
        draw::put_fit(buf, r.x + 2, *y, k, text, kw.saturating_sub(2));
        draw::put(buf, r.x + 1 + kw as u16, *y, v, info, w - kw);
        *y += 1;
    };
    line(buf, &mut y, msg(MsgId::MInfoCompName), &gm_platform::users::host_name());
    line(buf, &mut y, msg(MsgId::MInfoUserName), &gm_platform::users::current_user());
    line(buf, &mut y, "OS", gm_platform::os_name());

    let sep = |buf: &mut Buffer, y: &mut u16, title: &str| {
        if *y >= bottom {
            return;
        }
        draw::separator(buf, r.x, *y, r.width, true, boxs);
        draw::title_at(buf, r, *y, title, text);
        *y += 1;
    };

    if let Some(dir) = other.local_dir() {
        let root = gm_platform::drives::volume_root(&dir);
        let drive =
            gm_platform::drives::list().into_iter().filter(|d| root.starts_with(&d.path) || d.path == root).max_by_key(|d| d.path.as_os_str().len());
        sep(buf, &mut y, &format!("{} {}", msg(MsgId::MInfoDiskTitle), root.display()));
        if let Some(d) = &drive {
            let kind = match d.kind {
                gm_platform::DriveKind::Fixed | gm_platform::DriveKind::Root | gm_platform::DriveKind::Home => msg(MsgId::MInfoFixed),
                gm_platform::DriveKind::Removable => msg(MsgId::MInfoRemovable),
                gm_platform::DriveKind::Network => msg(MsgId::MInfoNetwork),
                gm_platform::DriveKind::Cdrom => msg(MsgId::MInfoCDROM),
                gm_platform::DriveKind::Ram => msg(MsgId::MInfoRAM),
                gm_platform::DriveKind::Unknown => msg(MsgId::MInfoUnknown),
            };
            line(buf, &mut y, &format!("{kind} {}", msg(MsgId::MInfoDisk)), &d.fs);
            if !d.label.is_empty() {
                line(buf, &mut y, msg(MsgId::MInfoDiskLabel), &d.label);
            }
        }
        if let Some((total, free)) = gm_platform::drives::space(&dir) {
            line(buf, &mut y, &format!("{} {}", msg(MsgId::MInfoDiskSpace), msg(MsgId::MInfoMetricTotal)), &draw::group_digits(total));
            line(buf, &mut y, &format!("{} {}", msg(MsgId::MInfoDiskSpace), msg(MsgId::MInfoMetricAvailable)), &draw::group_digits(free));
        }
        sep(buf, &mut y, &plain(MsgId::MInfoDescription));
        match crate::diz::find(&dir).or_else(|| ["readme", "README", "README.md", "readme.txt"].iter().map(|n| dir.join(n)).find(|p| p.is_file())) {
            Some(p) => {
                let content = std::fs::read(&p).map(|b| String::from_utf8_lossy(&b[..b.len().min(8192)]).into_owned()).unwrap_or_default();
                for l in content.lines() {
                    if y >= bottom {
                        break;
                    }
                    draw::put_fit(buf, r.x + 1, y, &l.replace('\t', "    "), text, w);
                    y += 1;
                }
            }
            None => {
                if y < bottom {
                    draw::put(buf, r.x + 2, y, msg(MsgId::MInfoDizAbsent), info, w - 1);
                }
            }
        }
    } else {
        sep(buf, &mut y, &other.provider().title());
        for (k, v) in other.provider().info_lines() {
            line(buf, &mut y, &k, &v);
        }
    }
}
