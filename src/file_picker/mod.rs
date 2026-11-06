//! Original @ token parser/dropdown renderer with a bounded dot host index.
//! Directory enumeration is explicit, nonrecursive, and executes on the worker.
#[allow(dead_code)]
pub mod context;
mod dropdown;
use ratatui::{buffer::Buffer, layout::Rect};
use std::{cell::Cell, path::PathBuf};
pub struct FuzzyMatchResult {
    pub path: String,
    pub indices: Vec<u32>,
}
#[derive(Default)]
pub struct Results {
    pub topk: Vec<FuzzyMatchResult>,
}
#[derive(Default)]
pub struct FileSearchState {
    results: Results,
    selected: usize,
    scroll: usize,
    hover: Option<usize>,
}
impl FileSearchState {
    fn is_visible(&self) -> bool {
        !self.results.topk.is_empty()
    }
    fn results(&self) -> &Results {
        &self.results
    }
    fn selected(&self) -> usize {
        self.selected
    }
    fn scroll_offset(&self) -> usize {
        self.scroll
    }
    fn hovered(&self) -> Option<usize> {
        self.hover
    }
    fn is_dir_mode(&self) -> bool {
        false
    }
    fn result_count(&self) -> usize {
        self.results.topk.len()
    }
    // Original FileSearchState move_selection/ensure_selected_visible clamp.
    pub fn move_selection(&mut self, delta: isize) {
        let len = self.results.topk.len();
        if len == 0 {
            return;
        }
        self.selected =
            (self.selected.min(len - 1) as isize + delta).clamp(0, (len - 1) as isize) as usize;
        let visible = dropdown::MAX_DROPDOWN_ROWS as usize;
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + visible {
            self.scroll = self.selected + 1 - visible;
        }
    }
}
#[derive(Default)]
pub struct Picker {
    pub context: Option<context::AtContext>,
    pub state: FileSearchState,
    pub area: Cell<Rect>,
    query: Option<String>,
    dismissed: Option<String>,
}
impl Picker {
    pub fn update(&mut self, text: &str, cursor: usize) -> Option<String> {
        let detected = context::detect(text, cursor);
        if detected.is_none() {
            self.dismissed = None;
        }
        self.context = detected.filter(|c| self.dismissed.as_deref() != Some(c.query.as_str()));
        let query = self.context.as_ref().map(|c| c.query.clone());
        if query != self.query {
            self.query = query.clone();
            self.state = Default::default();
            return query;
        }
        None
    }
    pub fn accept_results(&mut self, query: &str, files: Vec<String>) {
        if self.query.as_deref() == Some(query) {
            self.state.results.topk = files
                .into_iter()
                .map(|path| FuzzyMatchResult {
                    path,
                    indices: vec![],
                })
                .collect();
        }
    }
    pub fn dismiss(&mut self) {
        self.dismissed = self.query.clone();
        self.context = None;
        self.state = Default::default();
        self.area.set(Rect::default());
    }
    pub fn selected_path(&self) -> Option<PathBuf> {
        self.state
            .results
            .topk
            .get(self.state.selected)
            .map(|x| PathBuf::from(&x.path))
    }
    pub fn draw(&self, buf: &mut Buffer, outer: Rect, prompt: Rect) {
        self.area.set(Rect::default());
        if self.context.is_none() {
            return;
        }
        let h = dropdown::dropdown_height(&self.state, dropdown::MAX_DROPDOWN_ROWS);
        let height = h.saturating_sub(1).min(prompt.y.saturating_sub(outer.y));
        if height == 0 || outer.width < 12 {
            return;
        }
        let area = Rect::new(prompt.x, prompt.y - height, prompt.width, height);
        ratatui::widgets::Widget::render(ratatui::widgets::Clear, area, buf);
        dropdown::render_dropdown(buf, area, &self.state, &crate::theme::Theme::current());
        self.area.set(area);
    }
    pub fn hover(&mut self, x: u16, y: u16) -> bool {
        let a = self.area.get();
        if a.contains(ratatui::layout::Position::new(x, y)) {
            let row = self.state.scroll + usize::from(y - a.y);
            self.state.hover = (row < self.state.results.topk.len()).then_some(row);
            true
        } else {
            self.state.hover = None;
            false
        }
    }
    pub fn pointer(&mut self, x: u16, y: u16) -> bool {
        let area = self.area.get();
        if !area.contains(ratatui::layout::Position::new(x, y)) {
            return false;
        }
        let row = self.state.scroll + usize::from(y - area.y);
        if row >= self.state.results.topk.len() {
            return false;
        }
        self.state.selected = row;
        true
    }
}
pub fn search(query: &str) -> Result<Vec<String>, String> {
    let query = query.strip_prefix('!').unwrap_or(query);
    let supplied = PathBuf::from(query);
    let (dir, prefix) = if query.ends_with('/') {
        (supplied, String::new())
    } else {
        (
            supplied
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(std::path::Path::new("."))
                .to_owned(),
            supplied
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase(),
        )
    };
    let entries = std::fs::read_dir(dir).map_err(|_| "无法读取选择目录")?;
    let mut out = Vec::new();
    for entry in entries.take(1024).flatten() {
        let path = entry.path();
        if !crate::image_ingress::is_image_path(&path)
            || !entry.file_type().is_ok_and(|t| t.is_file())
        {
            continue;
        }
        if path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.to_ascii_lowercase().contains(&prefix))
        {
            out.push(path.to_string_lossy().into_owned());
        }
    }
    out.sort();
    out.truncate(64);
    Ok(out)
}
