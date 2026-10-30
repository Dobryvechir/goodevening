//! Help viewer for HLF files (`help.cpp`, `hlfviewer` plugin).
//!
//! HLF syntax: `@Topic` starts a topic, `@Alias=Topic` defines an alias,
//! `$` lines are the non-scrolling header, `#text#` is emphasis,
//! `~text~@Topic@` is a link, `^` centers a line, `\XX` / `\-` are color
//! codes (ignored) and `##`, `~~`, `@@`, `\\` are literal characters.
#![forbid(unsafe_code)]

use gm_i18n::{MsgId, msg};
use gm_term::{Buffer, Event, KeyCode, Mods, MouseButton, MouseEventKind, PaletteId, Rect, style};
use gm_ui::{Flow, Modal, Ui, draw};
use std::collections::HashMap;
use std::sync::OnceLock;

static FILES: &[(&str, &str)] = &[
    ("English", include_str!("../../../assets/help/GmEng.hlf")),
    ("Russian", include_str!("../../../assets/help/GmRus.hlf")),
    ("Ukrainian", include_str!("../../../assets/help/GmUkr.hlf")),
    ("Czech", include_str!("../../../assets/help/GmCze.hlf")),
    ("German", include_str!("../../../assets/help/GmGer.hlf")),
    ("Hungarian", include_str!("../../../assets/help/GmHun.hlf")),
    ("Polish", include_str!("../../../assets/help/GmPol.hlf")),
    ("Slovak", include_str!("../../../assets/help/GmSky.hlf")),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Seg {
    Text(String),
    Emph(String),
    Link(String, String),
}

#[derive(Clone, Debug, Default)]
pub struct Line {
    pub segs: Vec<Seg>,
    pub center: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Topic {
    pub header: Vec<Line>,
    pub body: Vec<Line>,
}

#[derive(Default)]
pub struct HelpFile {
    pub topics: HashMap<String, Topic>,
    pub order: Vec<String>,
    aliases: HashMap<String, String>,
}

impl HelpFile {
    pub fn parse(text: &str) -> HelpFile {
        let mut f = HelpFile::default();
        let mut cur: Option<(String, Topic)> = None;
        for raw in text.lines() {
            if raw.starts_with(".Options") || raw.starts_with(".Language") {
                continue;
            }
            if let Some(rest) = raw.strip_prefix('@')
                && !rest.starts_with('@')
                && !rest.is_empty()
            {
                if let Some((a, t)) = rest.split_once('=') {
                    f.aliases.insert(a.to_string(), t.to_string());
                    continue;
                }
                if let Some((n, t)) = cur.take() {
                    f.order.push(n.clone());
                    f.topics.insert(n, t);
                }
                cur = Some((rest.trim().to_string(), Topic::default()));
                continue;
            }
            let Some((_, t)) = cur.as_mut() else { continue };
            if let Some(h) = raw.strip_prefix('$') {
                t.header.push(parse_line(h));
            } else {
                t.body.push(parse_line(raw));
            }
        }
        if let Some((n, t)) = cur {
            f.order.push(n.clone());
            f.topics.insert(n, t);
        }
        // Trim trailing blank lines.
        for t in f.topics.values_mut() {
            while t.body.last().is_some_and(|l| l.segs.iter().all(|s| matches!(s, Seg::Text(x) if x.trim().is_empty()))) {
                t.body.pop();
            }
        }
        f
    }

    pub fn resolve(&self, name: &str) -> Option<&str> {
        let n = self.aliases.get(name).map(String::as_str).unwrap_or(name);
        self.topics.get_key_value(n).map(|(k, _)| k.as_str())
    }

    /// A generated index topic listing every topic by its title.
    pub fn index(&self) -> Topic {
        let mut items: Vec<(String, String)> = self
            .order
            .iter()
            .map(|n| {
                let title = self.topics[n].header.first().map(line_text).unwrap_or_else(|| n.clone());
                (title.trim().to_string(), n.clone())
            })
            .filter(|(t, _)| !t.is_empty())
            .collect();
        items.sort_by(|a, b| gm_text::sort::cmp_ci(&a.0, &b.0));
        Topic {
            header: vec![Line { segs: vec![Seg::Emph("Index".into())], center: false }],
            body: items.into_iter().map(|(t, n)| Line { segs: vec![Seg::Text(" ".into()), Seg::Link(t, n)], center: false }).collect(),
        }
    }
}

pub fn line_text(l: &Line) -> String {
    l.segs
        .iter()
        .map(|s| match s {
            Seg::Text(t) | Seg::Emph(t) | Seg::Link(t, _) => t.as_str(),
        })
        .collect()
}

fn parse_line(raw: &str) -> Line {
    let mut line = Line::default();
    let mut s = raw;
    if let Some(r) = s.strip_prefix('^') {
        line.center = true;
        s = r;
    }
    let s = s.replace("^<wrap>", "").replace("<wrap>", "");
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut buf = String::new();
    let mut emph = false;
    let flush = |buf: &mut String, emph: bool, segs: &mut Vec<Seg>| {
        if !buf.is_empty() {
            segs.push(if emph { Seg::Emph(std::mem::take(buf)) } else { Seg::Text(std::mem::take(buf)) });
        }
    };
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            '\\' => match next {
                Some('\\') => {
                    buf.push('\\');
                    i += 2;
                }
                Some('-') => i += 2,
                Some(a) if a.is_ascii_hexdigit() && chars.get(i + 2).is_some_and(|b| b.is_ascii_hexdigit()) => i += 3,
                _ => {
                    buf.push('\\');
                    i += 1;
                }
            },
            '#' if next == Some('#') => {
                buf.push('#');
                i += 2;
            }
            '#' => {
                flush(&mut buf, emph, &mut line.segs);
                emph = !emph;
                i += 1;
            }
            '~' if next == Some('~') => {
                buf.push('~');
                i += 2;
            }
            '@' if next == Some('@') => {
                buf.push('@');
                i += 2;
            }
            '~' => {
                // ~text~@Topic@
                let rest: String = chars[i + 1..].iter().collect();
                if let Some(end) = rest.find('~') {
                    let label = &rest[..end];
                    let after = &rest[end + 1..];
                    if let Some(t) = after.strip_prefix('@')
                        && let Some(te) = t.find('@')
                    {
                        flush(&mut buf, emph, &mut line.segs);
                        line.segs.push(Seg::Link(label.replace("##", "#"), t[..te].to_string()));
                        i += 1 + label.chars().count() + 1 + 1 + t[..te].chars().count() + 1;
                        continue;
                    }
                }
                buf.push('~');
                i += 1;
            }
            _ => {
                buf.push(c);
                i += 1;
            }
        }
    }
    flush(&mut buf, emph, &mut line.segs);
    line
}

fn file_for_language() -> &'static HelpFile {
    static CACHE: OnceLock<std::sync::Mutex<HashMap<&'static str, &'static HelpFile>>> = OnceLock::new();
    let lang = gm_i18n::current_language();
    let (name, text) = FILES.iter().find(|(n, _)| *n == lang).copied().unwrap_or(FILES[0]);
    let cache = CACHE.get_or_init(Default::default);
    let mut g = cache.lock().unwrap_or_else(|e| e.into_inner());
    g.entry(name).or_insert_with(|| Box::leak(Box::new(HelpFile::parse(text))))
}

struct Viewer {
    file: &'static HelpFile,
    name: String,
    topic: Topic,
    top: usize,
    link: Option<usize>,
    history: Vec<(String, usize)>,
    rows: usize,
    /// (row index in wrapped body, x range, topic) of visible links.
    links: Vec<(usize, u16, u16, String)>,
    wrapped: Vec<Line>,
    width: u16,
    rect: Rect,
}

impl Viewer {
    fn open(&mut self, name: &str) -> bool {
        let topic = if name == "Index" {
            Some(("Index".to_string(), self.file.index()))
        } else {
            self.file.resolve(name).map(|n| (n.to_string(), self.file.topics[n].clone()))
        };
        match topic {
            Some((n, t)) => {
                self.name = n;
                self.topic = t;
                self.top = 0;
                self.link = None;
                self.width = 0;
                true
            }
            None => false,
        }
    }

    fn wrap(&mut self, width: u16) {
        if self.width == width {
            return;
        }
        self.width = width;
        let w = width as usize;
        let mut out = Vec::new();
        for l in &self.topic.body {
            if gm_text::str_width(&line_text(l)) <= w {
                out.push(l.clone());
                continue;
            }
            // Word wrap keeping segments.
            let mut cur = Line { segs: Vec::new(), center: l.center };
            let mut col = 0;
            for seg in &l.segs {
                let (txt, mk): (&str, Box<dyn Fn(String) -> Seg>) = match seg {
                    Seg::Text(t) => (t, Box::new(Seg::Text)),
                    Seg::Emph(t) => (t, Box::new(Seg::Emph)),
                    Seg::Link(t, n) => {
                        let n = n.clone();
                        (t, Box::new(move |s| Seg::Link(s, n.clone())))
                    }
                };
                for word in txt.split_inclusive(' ') {
                    let ww = gm_text::str_width(word);
                    if col + ww > w && col > 0 {
                        out.push(std::mem::replace(&mut cur, Line { segs: Vec::new(), center: l.center }));
                        col = 0;
                    }
                    cur.segs.push(mk(word.to_string()));
                    col += ww;
                }
            }
            out.push(cur);
        }
        self.wrapped = out;
    }

    fn all_links(&self) -> Vec<(usize, String)> {
        let mut v = Vec::new();
        for (i, l) in self.wrapped.iter().enumerate() {
            for s in &l.segs {
                if let Seg::Link(_, t) = s {
                    v.push((i, t.clone()));
                }
            }
        }
        v
    }

    fn follow(&mut self, target: &str) {
        let back = (self.name.clone(), self.top);
        if self.open(target) {
            self.history.push(back);
        }
    }
}

impl Modal for Viewer {
    fn render(&mut self, buf: &mut Buffer, screen: Rect) -> Option<(u16, u16)> {
        let r = screen;
        self.rect = r;
        let text = style(PaletteId::HelpText);
        let hl = style(PaletteId::HelpTextHighlight);
        let topic_st = style(PaletteId::HelpTopic);
        let topic_sel = style(PaletteId::HelpTopicSelected);
        draw::fill(buf, r, ' ', text);
        let header_title = self.topic.header.first().map(line_text).unwrap_or_else(|| self.name.clone());
        draw::frame(buf, r, true, style(PaletteId::HelpBox), Some((header_title.trim(), style(PaletteId::HelpBoxTitle))));
        let inner_w = r.width.saturating_sub(4);
        self.wrap(inner_w);
        let head_rows = self.topic.header.len().saturating_sub(1) as u16;
        for (i, l) in self.topic.header.iter().skip(1).enumerate() {
            draw::put(buf, r.x + 2, r.y + 1 + i as u16, &line_text(l), hl, inner_w as usize);
        }
        let body_y = r.y + 1 + head_rows + u16::from(head_rows > 0);
        if head_rows > 0 {
            draw::separator(buf, r.x, body_y - 1, r.width, true, style(PaletteId::HelpBox));
        }
        let rows = r.bottom().saturating_sub(body_y + 1) as usize;
        self.rows = rows;
        let all = self.all_links();
        if let Some(li) = self.link
            && let Some((row, _)) = all.get(li)
        {
            if *row < self.top {
                self.top = *row;
            } else if *row >= self.top + rows {
                self.top = row + 1 - rows;
            }
        }
        self.links.clear();
        let mut link_counter = all.iter().take_while(|(row, _)| *row < self.top).count();
        for row in 0..rows {
            let i = self.top + row;
            let Some(l) = self.wrapped.get(i) else { break };
            let y = body_y + row as u16;
            let w = gm_text::str_width(&line_text(l)) as u16;
            let mut x = if l.center { r.x + 2 + inner_w.saturating_sub(w) / 2 } else { r.x + 2 };
            for s in &l.segs {
                let (t, st, link) = match s {
                    Seg::Text(t) => (t, text, None),
                    Seg::Emph(t) => (t, hl, None),
                    Seg::Link(t, n) => {
                        let selected = self.link == Some(link_counter);
                        link_counter += 1;
                        (t, if selected { topic_sel } else { topic_st }, Some(n.clone()))
                    }
                };
                let max = (r.right().saturating_sub(2).saturating_sub(x)) as usize;
                let n = draw::put(buf, x, y, t, st, max) as u16;
                if let Some(target) = link {
                    self.links.push((row, x, x + n, target));
                }
                x += n;
            }
        }
        if self.wrapped.len() > rows {
            draw::scrollbar(buf, r.right() - 1, body_y, rows as u16, self.top, self.wrapped.len(), rows, style(PaletteId::HelpScrollbar));
        }
        None
    }

    fn handle(&mut self, ev: &Event, _ui: &mut Ui) -> Flow {
        let all = self.all_links();
        match ev {
            Event::Key(k) => match (k.code, k.mods) {
                (KeyCode::Esc, _) | (KeyCode::F(10), _) => return Flow::Done,
                (KeyCode::Down, _) => {
                    if self.top + self.rows < self.wrapped.len() {
                        self.top += 1;
                    }
                    self.link = None;
                }
                (KeyCode::Up, _) => {
                    self.top = self.top.saturating_sub(1);
                    self.link = None;
                }
                (KeyCode::PageDown, _) => {
                    self.top = (self.top + self.rows).min(self.wrapped.len().saturating_sub(self.rows));
                    self.link = None;
                }
                (KeyCode::PageUp, _) => {
                    self.top = self.top.saturating_sub(self.rows);
                    self.link = None;
                }
                (KeyCode::Home, _) => self.top = 0,
                (KeyCode::End, _) => self.top = self.wrapped.len().saturating_sub(self.rows),
                (KeyCode::Tab, Mods::NONE) | (KeyCode::Right, _) if !all.is_empty() => {
                    let start = self.link.map(|l| l + 1).unwrap_or_else(|| all.iter().position(|(r, _)| *r >= self.top).unwrap_or(0));
                    self.link = Some(start % all.len());
                }
                (KeyCode::Tab, Mods::SHIFT) | (KeyCode::Left, _) if !all.is_empty() => {
                    let n = all.len();
                    self.link = Some(self.link.map(|l| (l + n - 1) % n).unwrap_or(n - 1));
                }
                (KeyCode::Enter, _) => {
                    if let Some((_, t)) = self.link.and_then(|l| all.get(l)).cloned() {
                        self.follow(&t);
                    }
                }
                (KeyCode::Backspace, _) | (KeyCode::F(1), Mods::ALT) => {
                    if let Some((n, top)) = self.history.pop() {
                        self.open(&n);
                        self.top = top;
                    }
                }
                (KeyCode::F(1), Mods::SHIFT) => self.follow("Index"),
                (KeyCode::F(1), Mods::NONE) => self.follow("Help"),
                (KeyCode::F(2), Mods::SHIFT) | (KeyCode::Char('c'), Mods::NONE) => self.follow("Contents"),
                _ => {}
            },
            Event::Mouse(m) => match m.kind {
                MouseEventKind::ScrollDown => {
                    if self.top + self.rows < self.wrapped.len() {
                        self.top += 1;
                    }
                }
                MouseEventKind::ScrollUp => self.top = self.top.saturating_sub(1),
                MouseEventKind::Down(MouseButton::Left) => {
                    let hit = self.links.iter().find(|(row, x1, x2, _)| {
                        let y = self.rect.y
                            + 1
                            + (self.topic.header.len().saturating_sub(1) as u16)
                            + u16::from(self.topic.header.len() > 1)
                            + *row as u16;
                        m.row == y && m.column >= *x1 && m.column < *x2
                    });
                    if let Some((_, _, _, t)) = hit.cloned() {
                        self.follow(&t);
                    }
                }
                _ => {}
            },
            _ => {}
        }
        Flow::Continue
    }
}

/// Opens the help viewer at `topic` (falls back to "Contents").
pub fn show(ui: &mut Ui, topic: &str) {
    let file = file_for_language();
    let mut v = Viewer {
        file,
        name: String::new(),
        topic: Topic::default(),
        top: 0,
        link: None,
        history: Vec::new(),
        rows: 1,
        links: Vec::new(),
        wrapped: Vec::new(),
        width: 0,
        rect: Rect::default(),
    };
    if !v.open(topic) && !v.open("Contents") {
        gm_ui::message::error(ui, msg(MsgId::MHelpTitle), &format!("{}\n{topic}", msg(MsgId::MHelpTopicNotFound)));
        return;
    }
    let _ = ui.run_modal(&mut v);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_markup() {
        let l = parse_line("Press #F1# or see ~the index~@Index@, 50## and \\1Fcolor\\- \\\\");
        assert_eq!(
            l.segs,
            vec![
                Seg::Text("Press ".into()),
                Seg::Emph("F1".into()),
                Seg::Text(" or see ".into()),
                Seg::Link("the index".into(), "Index".into()),
                Seg::Text(", 50# and color \\".into()),
            ]
        );
    }

    #[test]
    fn english_file() {
        let f = HelpFile::parse(FILES[0].1);
        assert!(f.topics.len() > 150);
        assert!(f.resolve("Contents").is_some());
        assert!(f.resolve("Viewer").is_some() || f.resolve("ViewerCmd").is_some());
        assert!(!f.topics.keys().any(|k| k.starts_with("Plugin")));
        assert!(f.index().body.len() > 100);
    }
}
