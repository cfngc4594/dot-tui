#[allow(dead_code)]
mod attachments;
#[cfg(test)]
mod audit_tests;
mod backend;
mod clipboard_bridge;
#[cfg(target_os = "macos")]
mod clipboard_native;
#[allow(dead_code)]
mod clipboard_paste;
#[cfg(not(target_os = "macos"))]
mod clipboard_portable;
mod copy_feedback;
#[cfg(test)]
mod copy_feedback_tests;
mod diagnostics;
mod dropdown_chrome;
#[cfg(test)]
mod enter_history_tests;
mod file_picker;
mod grok_markdown;
#[allow(clippy::collapsible_if)] // Preserve the extracted upstream scroll module unchanged.
mod grok_scroll;
#[cfg(test)]
mod grok_ui_parity_tests;
mod history_selection;
#[cfg(test)]
mod history_selection_tests;
mod i18n;
#[cfg(test)]
mod image_app_tests;
#[cfg(target_os = "macos")]
mod image_clipboard;
#[allow(dead_code)]
mod image_composer;
#[cfg(feature = "dev-demo")]
mod image_demo;
mod image_download;
#[cfg(test)]
mod image_focus_tests;
mod image_ingress;
#[allow(dead_code)]
mod image_transfer;
mod inline_history;
mod keyboard_protocol;
#[cfg(test)]
mod keyboard_safety_tests;
mod native_media;
mod platform_fs;
#[allow(dead_code)]
mod prompt_images;
#[allow(dead_code)]
mod render;
mod sanitize;
mod scrollback;
#[allow(dead_code)]
mod scrollbar;
#[cfg(any(test, feature = "dev-demo"))]
mod showcase;
mod slash;
#[allow(dead_code)]
mod slash_dropdown;
#[allow(dead_code)]
mod terminal;
#[allow(dead_code)]
mod theme;
mod ui;
mod upstream_cursor;
mod upstream_enter;
mod upstream_follow;
mod upstream_input;
mod upstream_media;
#[allow(dead_code)]
mod upstream_text_selection;
mod upstream_user_fold;
#[allow(dead_code)]
mod util;
use backend::{Backend, Dot, Message, SendError};
use clap::Parser;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
        EnableFocusChange, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;
use sanitize::strip_control_chars as clean;
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(
    version,
    about = "Native single-conversation dot client. Enter sends to your connected dot; Shift+Enter inserts a newline when reported by the terminal."
)]
struct Args {
    /// Connect directly, ignoring proxy environment variables.
    #[arg(long)]
    no_proxy: bool,
    /// Existing official Codex login file, not an inline token.
    #[arg(long)]
    auth: Option<PathBuf>,
    /// Development only: fake local backend; never reads credentials.
    #[cfg(feature = "dev-demo")]
    #[arg(long, conflicts_with = "auth")]
    demo: bool,
    /// Offline README scene for a 128x40 terminal; never reads credentials.
    #[cfg(feature = "dev-demo")]
    #[arg(long, conflicts_with_all = ["auth", "demo_delay_ms", "demo_history", "demo_incoming", "demo_image", "demo_clipboard_image", "demo_inline_fixtures", "demo_inline_large_fixtures"])]
    showcase: bool,
    /// Development-only synthetic connection delay, never accesses an account.
    #[cfg(feature = "dev-demo")]
    #[arg(long, requires = "demo", default_value_t = 0)]
    demo_delay_ms: u64,
    /// Development-only rich history fixture.
    #[cfg(feature = "dev-demo")]
    #[arg(long, requires = "demo", default_value_t = 0)]
    demo_history: usize,
    /// Offline incoming-message fixture, injected on the next latest-page read.
    #[cfg(feature = "dev-demo")]
    #[arg(long, requires = "demo")]
    demo_incoming: bool,
    /// Isolated original image widget harness; synthetic data only.
    #[cfg(feature = "dev-demo")]
    #[arg(long, requires = "demo")]
    demo_image: bool,
    /// Synthetic image paste fixture; never reads the system clipboard.
    #[cfg(feature = "dev-demo")]
    #[arg(long, requires = "demo")]
    demo_clipboard_image: bool,
    #[cfg(feature = "dev-demo")]
    #[arg(long, requires = "demo")]
    demo_inline_fixtures: bool,
    #[cfg(feature = "dev-demo")]
    #[arg(long, requires = "demo")]
    demo_inline_large_fixtures: bool,
}
impl Args {
    fn is_showcase(&self) -> bool {
        #[cfg(feature = "dev-demo")]
        {
            self.showcase
        }
        #[cfg(not(feature = "dev-demo"))]
        {
            false
        }
    }
    fn is_demo(&self) -> bool {
        #[cfg(feature = "dev-demo")]
        {
            self.demo || self.showcase
        }
        #[cfg(not(feature = "dev-demo"))]
        {
            false
        }
    }
}
fn require_terminal(stdin_tty: bool, stdout_tty: bool) -> Result<(), String> {
    if stdin_tty && stdout_tty {
        Ok(())
    } else {
        Err("请在交互终端中运行 dot-tui；尚未读取登录信息。".into())
    }
}
#[derive(Clone)]
struct SendPlan {
    dot: Dot,
    text: String,
    key: String,
    images: Vec<prompt_images::PastedImage>,
}
#[derive(Clone, Debug)]
struct ReadingAnchor {
    id: String,
    row: usize,
    media: Option<(usize, bool, usize)>,
}
struct App {
    dots: Vec<Dot>,
    selected: usize,
    messages: Vec<Message>,
    scroll: usize,
    status: String,
    busy: bool,
    generation: u64,
    live: bool,
    older_exhausted: bool,
    older_cursor: Option<String>,
    older_seen: std::collections::HashSet<String>,
    older_failed: bool,
    older_error: Option<String>,
    reading_anchor: Option<ReadingAnchor>,
    fold_anchor: Option<ui::FoldAnchor>,
    timeline_area: std::cell::Cell<Rect>,
    draft: image_composer::ImageComposer,
    image_loading: usize,
    image_stage_tx: Option<mpsc::SyncSender<Job>>,
    paste_feedback: Option<(String, Instant)>,
    received_image: Option<prompt_images::PastedImage>,
    image_view_loading: bool,
    image_view_epoch: u64,
    image_view_number: usize,
    image_view_file: Option<String>,
    native_media: native_media::Store,
    history_frame: std::cell::RefCell<Vec<inline_history::Placement>>,
    history_layout_pending: std::cell::Cell<bool>,
    history_pointer: std::cell::Cell<Option<Position>>,
    history_image_hover: std::cell::RefCell<Option<(String, usize)>>,
    visible_media: std::cell::RefCell<std::collections::HashSet<String>>,
    history_loading: std::collections::HashSet<String>,
    history_failed: std::collections::HashSet<String>,
    #[cfg(any(test, feature = "dev-demo"))]
    image_clipboard_fixture: Option<Vec<u8>>,
    picker: file_picker::Picker,
    language_menu: slash::Menu,
    language_path: Option<PathBuf>,
    draft_epoch: u64,
    previewed_images: std::cell::RefCell<std::collections::HashSet<u64>>,
    image_painted: std::cell::Cell<Option<u64>>,
    image_escapes: std::cell::RefCell<Option<terminal::overlay::Escapes>>,
    draft_state: std::cell::RefCell<xai_ratatui_textarea::TextAreaState>,
    composer_area: std::cell::Cell<Rect>,
    timeline_cache: ui::TimelineCache,
    scrollbar_drag: Option<Rect>,
    expanded_messages: std::collections::HashMap<String, bool>,
    history_press: Option<(String, u16, u16)>,
    last_history_click: Option<(Instant, String, u8)>,
    text_selection: upstream_text_selection::TextSelection,
    history_selection: std::cell::RefCell<history_selection::HistorySelection>,
    keyboard_confirmed: bool,
    window_focused: bool,
    reassert_mouse_capture: bool,
    clipboard: clipboard_bridge::Clipboard,
    copy_feedback: std::cell::RefCell<copy_feedback::CopyToast>,
    prompt_focused: bool,
    prompt_mouse_down: bool,
    focus_in_batch: bool,
    bound_target: Option<(String, String)>,
    paste_in_batch: bool,
    last_paste: Option<Instant>,
    pending: Option<SendPlan>,
    uncertain: bool,
    send_error: Option<String>,
    submitted_history: Vec<image_composer::ImageComposer>,
    quit_review: bool,
    follow_latest: bool,
    bottom_hit: std::cell::Cell<Option<Rect>>,
    bottom_hover: std::cell::Cell<bool>,
    poll_paused: bool,
    read_retry_at: Option<Instant>,
    read_failures: u32,
    ui_visible: bool,
    operation: Option<(String, Instant)>,
    #[cfg(any(test, feature = "dev-demo"))]
    showcase_elapsed_seconds: Option<u64>,
    last_result: String,
    status_observation: std::cell::RefCell<(String, Instant)>,
    worker_failed: bool,
    last_send_accepted: bool,
    scroller: grok_scroll::Scroller,
}
impl App {
    fn new() -> Self {
        let clipboard = clipboard_bridge::Clipboard::default();
        let mut draft = image_composer::ImageComposer::default();
        draft.set_clipboard_provider(clipboard.provider());
        Self {
            dots: Vec::new(),
            selected: 0,
            messages: Vec::new(),
            scroll: 0,
            status: "尚未连接".into(),
            busy: false,
            generation: 0,
            live: true,
            older_exhausted: false,
            older_cursor: None,
            older_seen: Default::default(),
            older_failed: false,
            older_error: None,
            reading_anchor: None,
            fold_anchor: None,
            timeline_area: std::cell::Cell::new(Rect::default()),
            draft,
            image_loading: 0,
            image_stage_tx: None,
            paste_feedback: None,
            received_image: None,
            image_view_loading: false,
            image_view_epoch: 0,
            image_view_number: 1,
            image_view_file: None,
            native_media: Default::default(),
            history_frame: Default::default(),
            history_layout_pending: Default::default(),
            history_pointer: Default::default(),
            history_image_hover: Default::default(),
            visible_media: Default::default(),
            history_loading: Default::default(),
            history_failed: Default::default(),
            #[cfg(any(test, feature = "dev-demo"))]
            image_clipboard_fixture: None,
            picker: Default::default(),
            language_menu: Default::default(),
            language_path: None,
            draft_epoch: 0,
            previewed_images: Default::default(),
            image_painted: Default::default(),
            image_escapes: Default::default(),
            draft_state: std::cell::RefCell::default(),
            composer_area: std::cell::Cell::new(Rect::default()),
            timeline_cache: ui::TimelineCache::new(),
            scrollbar_drag: None,
            expanded_messages: Default::default(),
            history_press: None,
            last_history_click: None,
            // Use upstream WordSelect, matching the installed Grok 1.0.77 docs.
            // The public source's Flash default can be overridden at startup.
            text_selection: upstream_text_selection::TextSelection::WordSelect,
            history_selection: Default::default(),
            keyboard_confirmed: false,
            window_focused: true,
            reassert_mouse_capture: false,
            clipboard,
            copy_feedback: Default::default(),
            prompt_focused: true,
            prompt_mouse_down: false,
            focus_in_batch: false,
            bound_target: None,
            paste_in_batch: false,
            last_paste: None,
            pending: None,
            uncertain: false,
            send_error: None,
            submitted_history: Vec::new(),
            quit_review: false,
            follow_latest: true,
            bottom_hit: Default::default(),
            bottom_hover: Default::default(),
            poll_paused: false,
            read_retry_at: None,
            read_failures: 0,
            ui_visible: true,
            operation: None,
            #[cfg(any(test, feature = "dev-demo"))]
            showcase_elapsed_seconds: None,
            last_result: "尚未完成消息读取".into(),
            status_observation: std::cell::RefCell::new((String::new(), Instant::now())),
            worker_failed: false,
            last_send_accepted: false,
            scroller: grok_scroll::Scroller::new(),
        }
    }
    #[cfg(any(test, feature = "dev-demo"))]
    fn demo() -> Self {
        let mut app = Self::new();
        app.live = false;
        app.keyboard_confirmed = true; // Unit fixture; run() replaces with negotiated capability before input.
        app.dots = vec![demo_dot()];
        app.messages = demo_messages();
        app.bound_target = Some(("demo".into(), "demo-room".into()));
        app.status = "开发模拟 · 不访问账户".into();
        app.last_result = "开发模拟 · 非真实消息".into();
        app
    }
    fn begin(&mut self, phase: &str) {
        #[cfg(any(test, feature = "dev-demo"))]
        {
            self.showcase_elapsed_seconds = None;
        }
        self.busy = true;
        self.operation = Some((phase.into(), Instant::now()));
        self.status = phase.into();
    }
    fn status_line(&self) -> String {
        if self.uncertain {
            return format!(
                "{} · Check the official app before resending. Up from an empty input recalls the previous submission.",
                self.send_error
                    .as_deref()
                    .unwrap_or("Previous send not confirmed")
            );
        }
        if let Some(error) = &self.send_error {
            return error.clone();
        }
        match &self.operation {
            Some((phase, started)) => {
                let seconds = started.elapsed().as_secs();
                #[cfg(any(test, feature = "dev-demo"))]
                let seconds = self.showcase_elapsed_seconds.unwrap_or(seconds);
                format!("{phase} · {seconds}秒")
            }
            None => self.status.clone(),
        }
    }
    fn header_status(&self, layout_pending: bool, image_error: Option<String>) -> Option<String> {
        let now = Instant::now();
        let mut observed = self.status_observation.borrow_mut();
        if observed.0 != self.status {
            *observed = (self.status.clone(), now);
        }
        if self.worker_failed || self.uncertain || self.send_error.is_some() {
            return Some(self.status_line());
        }
        if self.pending.is_none() {
            if self.image_loading > 0 {
                return Some("正在处理图片…".into());
            }
            if let Some((message, at)) = &self.paste_feedback {
                if at.elapsed() < Duration::from_secs(6) {
                    return Some(message.clone());
                }
            }
        }
        if self.pending.is_some() || self.busy || self.poll_paused {
            return Some(self.status_line());
        }
        if self.image_view_loading {
            return Some("正在准备并打开图片…".into());
        }
        if self.image_loading > 0 {
            return Some("正在处理图片…".into());
        }
        if let Some((file, _)) = self.history_image_hover.borrow().as_ref() {
            let room = self
                .dots
                .get(self.selected)
                .and_then(|d| d.room.as_deref())
                .unwrap_or("");
            if self.history_loading.contains(&format!("{room}/{file}")) {
                return Some("正在准备图片预览…".into());
            }
        }
        if !self.history_loading.is_empty() {
            return Some("正在加载历史图片…".into());
        }
        if layout_pending {
            return Some("正在排版历史…".into());
        }
        if let Some(error) = image_error {
            return Some(format!("图片：{error}"));
        }
        if self.older_failed {
            return self
                .older_error
                .clone()
                .or_else(|| Some("历史读取已暂停；点击重试历史".into()));
        }
        if self.status.starts_with("已读取")
            || self.status == "尚未完成消息读取"
            || self.status.starts_with("开发模拟")
            || self.status.is_empty()
        {
            return None;
        }
        // Completed operations are brief, rather than permanent duplicate idle text.
        (now.duration_since(observed.1) < Duration::from_secs(4)).then(|| self.status_line())
    }
    fn worker_failure(&mut self, reason: &str) {
        self.worker_failed = true;
        self.poll_paused = true;
        self.read_retry_at = None;
        self.busy = false;
        self.operation = None;
        if self.pending.is_some() {
            self.uncertain = true;
            self.status = format!("{reason}；发送结果不确定，请核对官方应用，勿重发。");
        } else {
            self.status = format!("{reason}；未能确认本次读取结果，请退出后重启。");
        }
    }
    fn read_succeeded(&mut self) {
        self.poll_paused = false;
        self.read_retry_at = None;
        self.read_failures = 0;
    }
    fn retry_read_later(&mut self, reason: &str) {
        // Transport does not retry a single request; this scheduler separately
        // retries read-only operations. Avoid displaying contradictory guidance.
        let reason = reason
            .replace(" No automatic retry.", "")
            .replace(" Pause refresh and try later.", "");
        self.busy = false;
        self.operation = None;
        self.poll_paused = true;
        if reason.contains("Session expired.") {
            // Repeating requests with the same expired credentials cannot repair login.
            self.read_retry_at = None;
            self.status = "登录已失效；请在官方应用重新登录后重启 dot-tui".into();
        } else {
            let delay = if reason.contains("Rate limited.") {
                60
            } else {
                (10u64 << self.read_failures.min(3)).min(60)
            };
            self.read_failures = self.read_failures.saturating_add(1);
            self.read_retry_at = Some(Instant::now() + Duration::from_secs(delay));
            self.status = format!("{reason}；{delay}秒后自动重试读取");
        }
        self.last_result = self.status.clone();
    }
    fn drain(&mut self, rx: &mpsc::Receiver<Reply>, tx: &mpsc::Sender<Job>) {
        if self.worker_failed {
            return;
        }
        loop {
            match rx.try_recv() {
                Ok(reply) => self.apply(reply, tx),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.worker_failure("后台连接已断开");
                    break;
                }
            }
        }
    }
    fn commit_image_preview(&self) {
        if self.received_image.is_some() || self.history_image_hover.borrow().is_some() {
            return;
        }
        if let Some(id) = self.image_painted.get() {
            self.previewed_images.borrow_mut().insert(id);
        }
    }
    #[cfg(test)]
    fn draft(&self) -> &str {
        self.draft.text()
    }
    #[cfg(test)]
    fn draft_cursor(&self) -> usize {
        self.draft.cursor()
    }
    fn restore_submitted(&mut self) {
        if let Some(draft) = self.submitted_history.pop() {
            self.draft = draft;
            self.draft_epoch = self.draft_epoch.wrapping_add(1);
            self.previewed_images.borrow_mut().clear();
        }
    }
    fn reset_draft(&mut self) {
        self.draft_epoch = self.draft_epoch.wrapping_add(1);
        self.previewed_images.borrow_mut().clear();
        self.picker = Default::default();
        self.language_menu = Default::default();
        self.draft = image_composer::ImageComposer::default();
        self.draft.set_clipboard_provider(self.clipboard.provider());
        *self.draft_state.borrow_mut() = xai_ratatui_textarea::TextAreaState::default();
    }
    // Original should_restore_prompt_on_focus_gained with dot's single text pane,
    // no vim/overlay/agent task state. Only the dot local operation is mapped idle.
    fn should_restore_prompt_on_focus_gained(&self) -> bool {
        if self.prompt_focused || self.quit_review {
            return false;
        }
        !self.worker_failed
    }
    fn goto_bottom(&mut self) {
        self.history_selection.borrow_mut().clear();
        self.fold_anchor = None;
        self.scroller.cancel();
        self.reading_anchor = None;
        self.follow_latest = true;
        self.scroll = usize::MAX;
        self.scrollbar_drag = None;
        self.history_press = None;
        self.last_history_click = None;
    }
    fn history_changed(&self) {
        self.history_selection.borrow_mut().clear();
        self.timeline_cache.invalidate();
    }
    fn can_edit(&self) -> bool {
        self.window_focused
            && self.prompt_focused
            && !self.quit_review
            && self.received_image.is_none()
            && !self.image_view_loading
    }
    fn open_received_image(&mut self, file: String, tx: &mpsc::Sender<Job>) {
        if !self.window_focused
            || !self.ui_visible
            || self.quit_review
            || self.pending.is_some()
            || self.image_view_loading
            || self.dots.len() != 1
            || self.selected != 0
        {
            return;
        }
        let dot = self.dots[0].clone();
        if self.bound_target != dot.room.as_ref().map(|r| (dot.id.clone(), r.clone())) {
            return;
        }
        let key = format!("{}/{}", dot.room.as_deref().unwrap_or(""), file);
        self.history_pointer.set(None);
        self.image_view_number = 1;
        self.prompt_focused = false;
        self.focus_in_batch = true;
        if let Some(mut image) = self.native_media.view(&key) {
            image.source_path = None; // Remote filename is not a pasted local filepath.
            self.received_image = Some(image);
            self.status.clear();
            return;
        }
        let already_loading = self.history_loading.contains(&key);
        self.image_view_file = Some(key);
        self.image_view_epoch = self.image_view_epoch.wrapping_add(1);
        self.image_view_loading = true;
        self.prompt_focused = false;
        self.focus_in_batch = true;
        self.status = "读取图片元数据与匿名下载；不会发送消息。".into();
        if already_loading {
            return;
        }
        if tx
            .send(Job::ViewImage(self.image_view_epoch, dot, file))
            .is_err()
        {
            self.image_view_loading = false;
            self.status = "图片查看后台不可用".into();
        }
    }
    fn sync_language_menu(&mut self) {
        self.language_menu.sync(
            self.draft.text(),
            self.draft.cursor(),
            self.can_edit()
                && self.ui_visible
                && self.draft.images.is_empty()
                && self.image_loading == 0,
        );
    }
    fn complete_language(&mut self) {
        if self.image_loading > 0 || !self.draft.images.is_empty() {
            return;
        }
        if let Some(text) = self.language_menu.completion() {
            self.draft.clear_selection();
            let end = self.draft.text().len();
            self.draft.replace_range(0..end, &text);
            self.sync_language_menu();
            self.focus_in_batch = true;
        }
    }
    fn choose_language(&mut self) {
        if !self.draft.images.is_empty() || self.image_loading > 0 {
            self.language_menu.dismiss();
            self.status = "Finish or clear image attachments before changing language".into();
            return;
        }
        match self.language_menu.choice() {
            Some(slash::Choice::Arguments) => {
                self.reset_draft();
                self.append("/language ");
                self.sync_language_menu();
            }
            Some(slash::Choice::Language(language)) => {
                let result = self
                    .language_path
                    .as_ref()
                    .ok_or_else(|| "Cannot locate language preferences".to_owned())
                    .and_then(|path| i18n::save(path, language));
                match result {
                    Ok(()) => {
                        self.fold_anchor = None;
                        if !self.follow_latest {
                            self.reading_anchor = ui::reading_anchor(self);
                        }
                        i18n::set(language);
                        self.history_changed();
                        self.reset_draft();
                        self.status = "Interface language saved".into();
                    }
                    Err(e) => self.status = e,
                }
            }
            None => self.status = "Choose English or Chinese with /language".into(),
        }
        self.focus_in_batch = true;
    }
    fn choose_image(&mut self, tx: &mpsc::Sender<Job>) {
        if !self.can_edit() || !self.ui_visible {
            return;
        }
        if let (Some(path), Some(context)) =
            (self.picker.selected_path(), self.picker.context.clone())
        {
            self.draft.replace_range(context.range, "");
            self.draft.sync_images_with_textarea();
            self.picker.dismiss();
            self.stage_image(image_ingress::Input::Paths(vec![path]), tx);
        }
    }
    fn stage_image(&mut self, input: image_ingress::Input, tx: &mpsc::Sender<Job>) {
        if !self.can_edit() {
            return;
        }
        if self.image_loading >= 2 {
            self.paste_feedback = Some((
                "Still preparing earlier pasted images; try again shortly".into(),
                Instant::now(),
            ));
            return;
        }
        self.paste_feedback = None;
        self.image_loading += 1;
        self.paste_in_batch = true;
        self.last_paste = Some(Instant::now());
        let job = Job::StageImage(self.draft_epoch, input);
        let failed = if let Some(stage_tx) = &self.image_stage_tx {
            stage_tx.try_send(job).is_err()
        } else {
            tx.send(job).is_err()
        };
        if failed {
            self.image_loading -= 1;
            self.paste_feedback = Some(("图片后台不可用，未上传。".into(), Instant::now()));
        }
    }
    // Grok's Scrollback -> FocusPrompt -> forward paste, without bypassing a
    // modal, uncertain send, or application/window focus gate.
    fn focus_for_paste(&mut self) -> bool {
        if self.window_focused
            && self.ui_visible
            && !self.quit_review
            && self.received_image.is_none()
            && !self.image_view_loading
        {
            self.prompt_focused = true;
            true
        } else {
            self.paste_feedback = Some((
                "Paste is unavailable while input is locked".into(),
                Instant::now(),
            ));
            false
        }
    }
    fn append(&mut self, text: &str) {
        if !self.can_edit() {
            return;
        }
        let text = clean(text, true);
        if !text.is_empty() {
            self.last_send_accepted = false;
            self.draft.begin_interaction();
            self.draft.insert_str_replacing_selection(&text);
            self.draft.sync_images_with_textarea();
        }
    }
    fn input(&mut self, mut event: event::KeyEvent) {
        if !self.can_edit() {
            return;
        }
        let area = self.composer_area.get();
        if area.width > 0 {
            self.draft
                .cursor_pos_with_state(area, *self.draft_state.borrow());
        }
        let old = self.draft.text().to_owned();
        if let Some(xai_ratatui_textarea::EditCommand::Insert(c)) =
            xai_ratatui_textarea::classify_key_event(&event)
        {
            event.code = KeyCode::Char(clean(&c.to_string(), true).chars().next().unwrap_or(' '));
        }
        self.draft.handle_key(event); // Original Grok PromptWidget one-shot/key dispatcher.
        self.draft.sync_images_with_textarea();
        if self.draft.text() != old {
            self.last_send_accepted = false;
        }
    }
    #[cfg(test)]
    fn edit(&mut self, code: KeyCode) {
        self.input(event::KeyEvent::new(code, KeyModifiers::NONE));
    }
    fn submit(&mut self, tx: &mpsc::Sender<Job>) {
        if slash::local(self.draft.text()) {
            self.status = "Choose English or Chinese with /language".into();
            return;
        }
        if !self.window_focused
            || !self.ui_visible
            || self.worker_failed
            || self.image_loading > 0
            || self.image_view_loading
            || self.received_image.is_some()
            || self.pending.is_some()
            || self.quit_review
            || !self.prompt_focused
        {
            return;
        }
        if self.dots.len() != 1 || self.selected != 0 {
            return;
        }
        let dot = self.dots[0].clone();
        let Some(room) = dot.room.as_ref().filter(|s| !s.is_empty()) else {
            return;
        };
        if self.bound_target.as_ref() != Some(&(dot.id.clone(), room.clone())) {
            return;
        }
        self.draft.sync_images_with_textarea();
        let images = self.draft.images.clone();
        if images.iter().any(|i| {
            !self
                .previewed_images
                .borrow()
                .contains(&i.preview.identity())
        }) {
            self.status = "请先预览每张图片；窗口过窄时请放大。".into();
            return;
        }
        let text = self.draft.submission_text();
        if text.trim().is_empty() && images.is_empty() {
            return;
        }
        let plan = SendPlan {
            dot,
            text,
            key: uuid::Uuid::new_v4().to_string(),
            images,
        };
        self.pending = Some(plan.clone());
        self.begin("提交消息");
        if tx.send(Job::Send(self.generation, plan.clone())).is_err() {
            self.busy = false;
            self.operation = None;
            self.pending = None;
            self.uncertain = false;
            self.status = "后台不可用，消息未发出。".into();
        } else {
            // Grok dispatch/prompt: stash image/chip state before clearing the
            // consumed prompt. The history retains the original image bytes; no queued message is replayed.
            self.submitted_history.push(std::mem::take(&mut self.draft));
            self.reset_draft();
            self.send_error = None;
            self.uncertain = false;
            self.status = "正在提交；退出或超时不能撤回请求，请勿重复发送。".into();
        }
    }
    fn warm_visible_media(&mut self, tx: &mpsc::Sender<Job>) {
        // An asynchronous reflow gap is not leaving the viewport. Keep the
        // last admission/visibility interval until real geometry is available.
        if self.history_layout_pending.get() {
            return;
        }
        let room = self
            .dots
            .get(self.selected)
            .and_then(|d| d.room.as_deref())
            .unwrap_or("");
        self.native_media.set_visible(
            self.visible_media
                .borrow()
                .iter()
                .map(|f| format!("{room}/{f}"))
                .collect(),
        );
        if (!terminal::image::scrollback_inline_overlay_active()
            && self.history_image_hover.borrow().is_none())
            || !self.window_focused
            || self.quit_review
            || !self.ui_visible
            || self.dots.len() != 1
            || self.pending.is_some()
            || self.history_loading.len() >= 2
        {
            return;
        }
        let dot = self.dots[0].clone();
        let Some(room) = dot.room.as_ref() else {
            return;
        };
        if self.bound_target.as_ref() != Some(&(dot.id.clone(), room.clone())) {
            return;
        }
        for file in self.visible_media.borrow().iter() {
            let key = format!("{room}/{file}");
            if !self.native_media.can_load_auto(&key) && self.native_media.cached(&key).is_none() {
                if !self.follow_latest && self.reading_anchor.is_none() {
                    self.reading_anchor = ui::reading_anchor(self);
                }
                self.timeline_cache.set_media(
                    file.clone(),
                    upstream_media::Info {
                        error: Some("当前图片缓存容量不足；点击打开可优先加载".into()),
                        name: "Image".into(),
                        dimensions: (0, 0),
                    },
                );
            }
            if !self.native_media.can_load_auto(&key)
                || self.history_loading.contains(&key)
                || (self.image_view_loading && self.image_view_file.as_ref() == Some(&key))
                || self.history_failed.contains(&key)
            {
                continue;
            }
            if self.history_loading.len() >= 2 {
                break;
            }
            if tx
                .send(Job::LoadHistoryImage(dot.clone(), file.clone()))
                .is_ok()
            {
                self.history_loading.insert(key);
            }
        }
    }
    fn apply(&mut self, reply: Reply, tx: &mpsc::Sender<Job>) {
        if self.worker_failed {
            return;
        }
        match reply {
            Reply::HistoryImage(room, file, result) => {
                let key = format!("{room}/{file}");
                self.history_loading.remove(&key);
                if self.dots.len() != 1 || self.dots[0].room.as_deref() != Some(&room) {
                    return;
                }
                match result {
                    Ok(prepared) => {
                        self.history_failed.remove(&key);
                        if !self.follow_latest && self.reading_anchor.is_none() {
                            self.reading_anchor = ui::reading_anchor(self);
                        }
                        if let Some(dimensions) = prepared.image.preview_dimensions() {
                            let name = prepared
                                .image
                                .source_path
                                .as_ref()
                                .and_then(|p| p.file_name())
                                .map(|p| p.to_string_lossy().into_owned())
                                .unwrap_or_else(|| "Image".into());
                            self.timeline_cache.set_media(
                                file.clone(),
                                upstream_media::Info {
                                    error: None,
                                    name,
                                    dimensions,
                                },
                            );
                        }
                        if self.image_view_loading && self.image_view_file.as_ref() == Some(&key) {
                            self.apply(Reply::ViewedImage(self.image_view_epoch, Ok(prepared)), tx);
                        } else {
                            if !self.native_media.retain_auto(key, prepared) {
                                self.timeline_cache.set_media(
                                    file,
                                    upstream_media::Info {
                                        error: Some(
                                            "当前图片缓存容量不足；点击打开可优先加载".into(),
                                        ),
                                        name: "Image".into(),
                                        dimensions: (0, 0),
                                    },
                                );
                                self.status =
                                    "可见图片达到会话缓存预算；点击 Open Image 可优先打开".into();
                            }
                        }
                    }
                    Err(e) => {
                        let reason = clean(&e, false);
                        if self.image_view_file.as_ref() == Some(&key) {
                            self.image_view_loading = false;
                            self.image_view_file = None;
                        }
                        self.timeline_cache.set_media(
                            file,
                            upstream_media::Info {
                                error: Some(reason.clone()),
                                name: "Image".into(),
                                dimensions: (0, 0),
                            },
                        );
                        self.history_failed.insert(key);
                        self.status = format!("图片加载失败：{reason}；点击 Open Image 重试");
                    }
                }
            }
            Reply::ViewedImage(epoch, result) => {
                if epoch != self.image_view_epoch {
                    return;
                }
                self.image_view_loading = false;
                match result {
                    Ok(prepared) => {
                        let fallback = prepared.image.clone();
                        if let (Some(key), Some(dimensions)) =
                            (&self.image_view_file, prepared.image.preview_dimensions())
                            && let Some((_, file)) = key.split_once('/')
                        {
                            if !self.follow_latest && self.reading_anchor.is_none() {
                                self.reading_anchor = ui::reading_anchor(self);
                            }
                            let name = prepared
                                .image
                                .source_path
                                .as_ref()
                                .and_then(|p| p.file_name())
                                .map(|p| p.to_string_lossy().into_owned())
                                .unwrap_or_else(|| "Image".into());
                            self.timeline_cache.set_media(
                                file.to_owned(),
                                upstream_media::Info {
                                    error: None,
                                    name,
                                    dimensions,
                                },
                            );
                        }
                        let key = self.image_view_file.take().unwrap_or_default();
                        self.history_failed.remove(&key);
                        if !self.native_media.retain(key.clone(), prepared) {
                            self.received_image = None;
                            self.status = "图片超过会话缓存预算，未打开".into();
                            return;
                        }
                        if !self.window_focused {
                            self.received_image = None;
                            self.status = "图片已准备；返回窗口后点击 Open Image 打开。".into();
                            return;
                        }
                        self.received_image =
                            self.native_media
                                .view(&key)
                                .or(Some(fallback))
                                .map(|mut image| {
                                    image.source_path = None;
                                    image.display_number = self.image_view_number;
                                    image
                                });
                        self.focus_in_batch = true;
                        self.status.clear();
                    }
                    Err(e) => {
                        let reason = clean(&e, false);
                        if let Some(key) = self.image_view_file.take() {
                            if let Some((_, file)) = key.split_once('/') {
                                if !self.follow_latest && self.reading_anchor.is_none() {
                                    self.reading_anchor = ui::reading_anchor(self);
                                }
                                self.timeline_cache.set_media(
                                    file.to_owned(),
                                    upstream_media::Info {
                                        name: "Image".into(),
                                        dimensions: (0, 0),
                                        error: Some(reason.clone()),
                                    },
                                );
                            }
                            self.history_failed.insert(key);
                        }
                        self.status = format!("图片加载失败：{reason}；点击 Open Image 重试");
                    }
                }
            }
            Reply::Files(query, result) => match result {
                Ok(files) => self.picker.accept_results(&query, files),
                Err(e) => self.status = e,
            },
            Reply::StagedImage(epoch, result) => {
                self.image_loading = self.image_loading.saturating_sub(1);
                if epoch != self.draft_epoch {
                    return;
                }
                match result {
                    Ok(staged) => {
                        let empty = staged.images.is_empty() && staged.text.is_none();
                        if empty {
                            self.paste_feedback = Some((
                                "Clipboard has no image or text to paste".into(),
                                Instant::now(),
                            ));
                        }
                        if self.draft.images.len() + staged.images.len()
                            > image_composer::ImageComposer::IMAGE_CAP
                        {
                            self.paste_feedback =
                                Some(("图片数量超过十张，本次选择未加入。".into(), Instant::now()));
                            return;
                        }
                        for image in staged.images {
                            match self.draft.insert_image(image) {
                                Ok(()) => {}
                                Err(e) => {
                                    self.paste_feedback = Some((e, Instant::now()));
                                }
                            }
                        }
                        if let Some(text) = staged.text {
                            let focused = self.prompt_focused;
                            let window = self.window_focused;
                            self.prompt_focused = true;
                            self.window_focused = true;
                            self.append(&text);
                            self.prompt_focused = focused;
                            self.window_focused = window;
                        }
                        self.paste_in_batch = true;
                        self.last_paste = Some(Instant::now());
                    }
                    Err(e) => {
                        self.paste_feedback = Some((e, Instant::now()));
                    }
                }
            }
            Reply::Dots(result) => {
                self.busy = false;
                self.operation = None;
                match result {
                    Ok(d) => {
                        self.read_succeeded();
                        // Single-dot client: never silently select a recipient from several.
                        if d.len() > 1 {
                            self.bound_target = None;
                            self.dots.clear();
                            self.poll_paused = true;
                            self.status = "发现多个 dot，无法确定当前会话；请使用官方应用。".into();
                            self.last_result = self.status.clone();
                            return;
                        }
                        self.bound_target = None;
                        self.dots = d;
                        self.status = if self.dots.is_empty() {
                            "没有找到 dot".into()
                        } else {
                            "已读取 dot 列表".into()
                        };
                        self.last_result = self.status.clone();
                        request(self, tx, false)
                    }
                    Err(e) => {
                        self.retry_read_later(&e);
                    }
                }
            }
            Reply::PageLoaded(g, older, requested, result) => {
                if g != self.generation {
                    return;
                }
                match result {
                    Ok(page) => {
                        let initial = self.messages.is_empty();
                        let stalled = page.raw_count == 0
                            || page.next_before.is_none()
                            || (older
                                && (page.next_before == requested
                                    || page
                                        .next_before
                                        .as_ref()
                                        .is_some_and(|id| self.older_seen.contains(id))));
                        if (older || initial) && !stalled {
                            if let Some(id) = &page.next_before {
                                self.older_seen.insert(id.clone());
                            }
                            self.older_cursor = page.next_before;
                        }
                        self.apply(Reply::Page(g, older, Ok(page.messages)), tx);
                        self.older_exhausted = false;
                        if stalled && (older || initial) {
                            self.older_failed = true;
                            self.status = if page.raw_count == 0 {
                                "本页未返回消息；未确认历史已结束，点击底部“重试历史”重试".into()
                            } else {
                                "分页游标未推进；已暂停自动翻页，点击底部“重试历史”重试；不代表没有更早消息".into()
                            };
                            self.older_error = Some(self.status.clone());
                            self.last_result = self.status.clone();
                        }
                    }
                    Err(e) => self.apply(Reply::Page(g, older, Err(e)), tx),
                }
            }
            Reply::Page(g, older, result) => {
                if g != self.generation {
                    return;
                }
                self.busy = false;
                self.operation = None;
                match result {
                    Ok(messages) => {
                        self.read_succeeded();
                        self.bound_target = self
                            .dots
                            .get(self.selected)
                            .and_then(|d| d.room.as_ref().map(|r| (d.id.clone(), r.clone())));
                        if !self.follow_latest && self.reading_anchor.is_none() {
                            self.reading_anchor = ui::reading_anchor(self);
                        }
                        // Background polls often repeat the same messages. Keep
                        // WordSelect and its exact layout snapshot until visible
                        // data really changes; older pages ignore existing IDs.
                        let changed = messages.iter().any(|message| {
                            match self.messages.iter().find(|old| old.id == message.id) {
                                Some(old) => !older && old != message,
                                None => true,
                            }
                        });
                        if changed {
                            self.history_changed();
                        }
                        let count = messages.len();
                        self.last_result = format!(
                            "{}本页已读取 {count} 条消息",
                            if self.last_send_accepted {
                                "提交已获服务接受；"
                            } else {
                                ""
                            }
                        );
                        if !older {
                            let initial = self.messages.is_empty();
                            let added = messages
                                .iter()
                                .filter(|m| !self.messages.iter().any(|old| old.id == m.id))
                                .count();
                            merge_messages(&mut self.messages, messages);
                            if self.follow_latest {
                                self.scroll = usize::MAX;
                            }
                            if initial {
                                self.older_exhausted = false;
                            }
                            if added > 0 && !self.follow_latest {
                                self.last_result = format!("新增 {added} 条消息；保留当前阅读位置");
                            }
                        } else {
                            let additions: Vec<_> = messages
                                .into_iter()
                                .filter(|m| !self.messages.iter().any(|old| old.id == m.id))
                                .collect();
                            prepend_messages(&mut self.messages, additions);
                            self.older_exhausted = false;
                            self.older_failed = false;
                            self.older_error = None;
                        }
                        if !self.uncertain {
                            self.status = format!(
                                "已读取 {count} 条；只代表当前分页。向上滚动读取更早消息。"
                            );
                        }
                    }
                    Err(e) => {
                        self.older_failed |= older;
                        if older {
                            self.status =
                                format!("加载更早失败：{e}；点击底部“重试历史”重试；后台更新继续");
                            self.older_error = Some(self.status.clone());
                            self.last_result = self.status.clone();
                        } else {
                            self.retry_read_later(&e);
                        }
                    }
                }
            }
            Reply::Sent(g, key, result) => {
                if g != self.generation
                    || self.pending.as_ref().map(|p| p.key.as_str()) != Some(key.as_str())
                {
                    return;
                }
                self.busy = false;
                self.operation = None;
                self.quit_review = false;
                self.pending.take().unwrap();
                match result {
                    Ok(_message_id) => {
                        self.last_send_accepted = true;
                        self.last_result = "服务已接受消息（不代表 dot 已完成）".into();
                        self.uncertain = false;
                        self.send_error = None;
                        self.status =
                            "服务已接受消息（不代表 dot 已回复）。正在读取最新消息…".into();
                        self.follow_latest = true;
                        request(self, tx, false);
                    }
                    Err(SendError::Rejected(e) | SendError::NotSent(e)) => {
                        self.uncertain = false;
                        self.status = format!(
                            "{e} No automatic retry. Up from an empty input recalls the previous submission."
                        );
                        self.send_error = Some(self.status.clone());
                    }
                    Err(SendError::Uncertain(e)) => {
                        self.send_error = Some(e.clone());
                        self.uncertain = true;
                        self.status = e;
                        self.last_result = self.status.clone();
                        self.poll_paused = false;
                    }
                }
            }
        }
    }
}
#[cfg(any(test, feature = "dev-demo"))]
fn demo_dot() -> Dot {
    Dot {
        id: "demo".into(),
        name: "dot · 演示".into(),
        room: Some("demo-room".into()),
        speakers: vec![],
    }
}
#[cfg(any(test, feature = "dev-demo"))]
fn demo_messages() -> Vec<Message> {
    vec![Message { id:"1".into(), role:"you".into(), text:"请展示加粗、代码块和消息时间，便于对照界面。".into(), files:vec![],created_at:"2026-11-04T06:28:00Z".into() }, Message {
        id:"2".into(), role:"dot".into(),
        text:"这是一段**开发测试消息**，用于对照原版 Markdown 渲染，不来自真实账户。\n\n保留 **加粗**、*斜体*、`行内代码` 和 [源码链接](https://github.com/xai-org/grok-build)。\n\n```rust\nfn main() {\n    println!(\"hello, dot\");\n}\n```\n\n- 消息保持原始顺序\n- 输入框随内容增高\n- 鼠标和触控板滚动\n\n> 协议确认后 Enter 发送；Shift+Enter 换行。".into(),
        files:vec![],created_at:"2026-11-04T06:29:00Z".into(),
    }]
}

enum Job {
    StageImage(u64, image_ingress::Input),
    FindFiles(String),
    ViewImage(u64, Dot, String),
    LoadHistoryImage(Dot, String),
    Dots,
    Page(u64, Dot, Option<String>),
    Send(u64, SendPlan),
}
enum Reply {
    StagedImage(u64, Result<image_ingress::Staged, String>),
    Files(String, Result<Vec<String>, String>),
    ViewedImage(u64, Result<native_media::Prepared, String>),
    HistoryImage(String, String, Result<native_media::Prepared, String>),
    Dots(Result<Vec<Dot>, String>),
    Page(u64, bool, Result<Vec<Message>, String>),
    PageLoaded(
        u64,
        bool,
        Option<String>,
        Result<backend::MessagePage, String>,
    ),
    Sent(u64, String, Result<String, SendError>),
}
#[cfg(any(test, feature = "dev-demo"))]
struct FakeBackend {
    messages: Vec<Message>,
    fail_uncertain: bool,
    sends: usize,
    delay_ms: u64,
    incoming: bool,
    image_uploads: usize,
}
#[cfg(any(test, feature = "dev-demo"))]
impl FakeBackend {
    fn new() -> Self {
        Self {
            messages: demo_messages(),
            fail_uncertain: false,
            sends: 0,
            delay_ms: 0,
            incoming: false,
            image_uploads: 0,
        }
    }
    fn send_images(&mut self, plan: &SendPlan) -> Result<String, SendError> {
        let room = image_transfer::BoundRoom::from_unique(
            std::slice::from_ref(&plan.dot),
            plan.dot.room.as_deref().unwrap_or(""),
        )
        .map_err(|e| SendError::Rejected(e.into()))?;
        let mut images = Vec::new();
        for image in &plan.images {
            let mut i = image_transfer::LocalImage::stage(
                image
                    .encoded_bytes
                    .as_ref()
                    .ok_or_else(|| SendError::Rejected("missing local image".into()))?
                    .to_vec(),
            )
            .map_err(|e| SendError::Rejected(e.into()))?;
            i.record_preview(true);
            images.push(i);
        }
        image_transfer::transmit(
            &mut DemoImageTransport(self),
            &room,
            &plan.text,
            &images,
            &plan.key,
        )
        .map_err(|e| e.into_send_error())
    }
    fn send(&mut self, text: &str) -> Result<String, SendError> {
        self.sends += 1;
        if self.fail_uncertain {
            return Err(SendError::Uncertain(
                "模拟结果不确定；请核对后再自行决定是否发送。".into(),
            ));
        }
        let n = self.messages.len() + 1;
        let id = n.to_string();
        self.messages.push(Message {
            id: id.clone(),
            role: "you".into(),
            text: text.into(),
            files: vec![],
            created_at: chrono::Utc::now().to_rfc3339(),
        });
        self.messages.push(Message {
            id: (n + 1).to_string(),
            role: "dot".into(),
            text: "离线模拟回复：已收到文字，没有访问真实账户。".into(),
            files: vec![],
            created_at: chrono::Utc::now().to_rfc3339(),
        });
        Ok(id)
    }
}
#[cfg(any(test, feature = "dev-demo"))]
struct DemoImageTransport<'a>(&'a mut FakeBackend);
#[cfg(any(test, feature = "dev-demo"))]
impl image_transfer::Transport for DemoImageTransport<'_> {
    fn upload(
        &mut self,
        url: reqwest::Url,
        field: &str,
        mime: &str,
        bytes: &[u8],
    ) -> Result<serde_json::Value, String> {
        if url.host_str() != Some("chatgpt.com")
            || !url.path().ends_with("/files")
            || field != "file"
            || mime != attachments::mime_from_bytes(bytes)
        {
            return Err("synthetic transport failure".into());
        }
        attachments::decode_bounded(bytes).map_err(|_| "synthetic transport failure".to_owned())?;
        self.0.image_uploads += 1;
        if self.0.fail_uncertain {
            return Err("synthetic transport failure".into());
        }
        Ok(serde_json::json!({"id":format!("fake-file-{}",self.0.image_uploads)}))
    }
    fn message(
        &mut self,
        url: reqwest::Url,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if !url.path().ends_with("/messages") || body["request_id"] != body["idempotency_token"] {
            return Err("synthetic transport failure".into());
        }
        let files = body["content"]["attachments"]
            .as_array()
            .ok_or("synthetic missing files")?;
        if files.is_empty()
            || files
                .iter()
                .any(|v| v["type"] != "file" || v["file_id"].as_str().is_none())
        {
            return Err("synthetic transport failure".into());
        }
        let text = format!(
            "{}\n[离线图片附件 × {}]",
            body["content"]["text"].as_str().unwrap_or(""),
            files.len()
        );
        let id = self
            .0
            .send(&text)
            .map_err(|_| "synthetic transport failure".to_owned())?;
        if let Some(m) = self.0.messages.iter_mut().find(|m| m.id == id) {
            m.files = files
                .iter()
                .filter_map(|f| f["file_id"].as_str().map(str::to_owned))
                .collect();
        }
        Ok(serde_json::json!({"id":id}))
    }
}

#[cfg(any(test, feature = "dev-demo"))]
fn demo_image_view(dot: &Dot, file: &str) -> Result<prompt_images::PastedImage, String> {
    struct FakeView {
        file: String,
    }
    impl image_download::Transport for FakeView {
        fn metadata(&mut self, url: reqwest::Url) -> Result<serde_json::Value, String> {
            if !url.path().contains("/files/") {
                return Err("invalid fake metadata endpoint".into());
            }
            let name = if self.file == showcase::IMAGE_ID {
                "moonlit-night.png"
            } else {
                "offline-received.png"
            };
            Ok(
                serde_json::json!({"mime_type":"image/png","name":name,"download_url":"https://images.example.test/synthetic"}),
            )
        }
        fn anonymous(&mut self, _url: reqwest::Url) -> Result<Vec<u8>, String> {
            Ok(if self.file == showcase::IMAGE_ID {
                showcase::image_bytes()
            } else {
                image_ingress::synthetic_media(&self.file)
            })
        }
    }
    let room = image_transfer::BoundRoom::from_unique(
        std::slice::from_ref(dot),
        dot.room.as_deref().unwrap_or(""),
    )
    .map_err(str::to_owned)?;
    image_download::fetch(
        &mut FakeView {
            file: file.to_owned(),
        },
        &room,
        file,
    )
}

#[derive(Clone)]
struct LiveConfig {
    auth_path: PathBuf,
    no_proxy: bool,
}

enum Source {
    PendingLive(LiveConfig),
    Unavailable(LiveConfig, String),
    #[cfg(any(test, feature = "dev-demo"))]
    Demo(FakeBackend),
    Live(Backend),
}
impl Source {
    fn perform(&mut self, job: Job) -> Reply {
        if let Job::FindFiles(query) = job {
            return Reply::Files(query.clone(), file_picker::search(&query));
        }
        if let Job::StageImage(epoch, input) = job {
            #[cfg(any(test, feature = "dev-demo"))]
            let fake = matches!(self, Self::Demo(_));
            #[cfg(not(any(test, feature = "dev-demo")))]
            let fake = false;
            return Reply::StagedImage(epoch, image_ingress::stage(input, fake));
        }
        if let Self::PendingLive(config) | Self::Unavailable(config, _) = self {
            let config = config.clone();
            *self = match Backend::new(config.auth_path.clone(), config.no_proxy) {
                Ok(b) => Self::Live(b),
                Err(e) => Self::Unavailable(config, e),
            };
        }
        if let Job::Page(g, ref dot, ref before) = job
            && let Self::Live(backend) = self
        {
            return Reply::PageLoaded(
                g,
                before.is_some(),
                before.clone(),
                backend.message_page(dot, before.as_deref()),
            );
        }
        match job {
            Job::StageImage(..) | Job::FindFiles(..) | Job::LoadHistoryImage(..) => unreachable!(),
            Job::ViewImage(epoch, dot, file) => Reply::ViewedImage(
                epoch,
                match self {
                    Self::Live(b) => b.view_image(&dot, &file),
                    #[cfg(any(test, feature = "dev-demo"))]
                    Self::Demo(_) => demo_image_view(&dot, &file),
                    Self::Unavailable(_, e) => Err(e.clone()),
                    Self::PendingLive(_) => unreachable!(),
                }
                .and_then(|image| {
                    native_media::prepare(image).map_err(|e| format!("本地图片准备：{e}"))
                }),
            ),
            Job::Dots => Reply::Dots(match self {
                #[cfg(any(test, feature = "dev-demo"))]
                Self::Demo(b) => {
                    thread::sleep(Duration::from_millis(b.delay_ms.min(5000)));
                    Ok(vec![demo_dot()])
                }
                Self::Live(b) => b.list_dots(),
                Self::Unavailable(_, e) => Err(e.clone()),
                Self::PendingLive(_) => unreachable!(),
            }),
            Job::Page(g, d, before) => Reply::Page(
                g,
                before.is_some(),
                match self {
                    #[cfg(any(test, feature = "dev-demo"))]
                    Self::Demo(b) => {
                        if before.is_none() && b.incoming {
                            b.incoming = false;
                            b.messages.push(Message {
                                id: "999999".into(),
                                role: "dot".into(),
                                text: "ASYNC-INCOMING-OFFLINE".into(),
                                files: vec![],
                                created_at: "2026-11-04T07:30:00Z".into(),
                            });
                        }
                        Ok(b.messages
                            .iter()
                            .filter(|m| before.as_ref().is_none_or(|id| m.id < *id))
                            .rev()
                            .take(32)
                            .cloned()
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect())
                    }
                    Self::Live(b) => b.messages(&d, before.as_deref()),
                    Self::Unavailable(_, e) => Err(e.clone()),
                    Self::PendingLive(_) => unreachable!(),
                },
            ),
            Job::Send(g, p) => Reply::Sent(
                g,
                p.key.clone(),
                match self {
                    #[cfg(any(test, feature = "dev-demo"))]
                    Self::Demo(b) => {
                        if p.images.is_empty() {
                            b.send(&p.text)
                        } else {
                            b.send_images(&p)
                        }
                    }
                    Self::Live(b) => {
                        if p.images.is_empty() {
                            b.send(&p.dot, &p.text, &p.key)
                        } else {
                            b.send_images(&p.dot, &p.text, &p.images, &p.key)
                        }
                    }
                    Self::Unavailable(_, e) => Err(SendError::Rejected(e.clone())),
                    Self::PendingLive(_) => unreachable!(),
                },
            ),
        }
    }
}
fn merge_messages(old: &mut Vec<Message>, new: Vec<Message>) {
    for m in new {
        if let Some(i) = old.iter().position(|x| x.id == m.id) {
            old[i] = m
        } else {
            old.push(m)
        }
    }
    old.sort_by(|a, b| a.created_at.cmp(&b.created_at));
}
fn prepend_messages(old: &mut Vec<Message>, new: Vec<Message>) {
    let existing = std::mem::take(old);
    let mut combined = new;
    merge_messages(&mut combined, existing);
    *old = combined;
}
fn request(app: &mut App, tx: &mpsc::Sender<Job>, older: bool) {
    if app.worker_failed || app.busy || (older && app.reading_anchor.is_some()) {
        return;
    }
    if older && app.older_exhausted {
        app.status = "没有更多更早消息。".into();
        return;
    }
    if older {
        app.older_failed = false;
        app.older_error = None;
    }
    if let Some(d) = app.dots.get(app.selected).cloned() {
        let before = if older {
            app.older_cursor
                .clone()
                .or_else(|| app.messages.first().map(|m| m.id.clone()))
        } else {
            None
        };
        if tx.send(Job::Page(app.generation, d, before)).is_ok() {
            app.begin("读取消息");
            if older {
                app.follow_latest = false;
            }
            if !app.uncertain {
                app.status = "正在读取…".into()
            }
        } else {
            app.worker_failure("后台连接已断开");
        }
    }
}
fn poll_latest(app: &mut App, tx: &mpsc::Sender<Job>) {
    if app.busy || app.quit_review || app.worker_failed {
        return;
    }
    if let Some(deadline) = app.read_retry_at {
        if Instant::now() < deadline {
            return;
        }
        app.read_retry_at = None;
        app.poll_paused = false;
    }
    if app.poll_paused {
        return;
    }
    if app.dots.is_empty() {
        app.begin("自动重连并读取 dot 列表");
        if tx.send(Job::Dots).is_err() {
            app.worker_failure("后台连接已断开");
        }
    } else {
        request(app, tx, false);
    }
}
fn sync_history_position(app: &mut App, area: Rect) {
    if let Some(anchor) = app.fold_anchor.as_ref() {
        let timeline = ui::timeline(app, area);
        if let Some(scroll) = ui::resolve_fold_anchor(&timeline, anchor) {
            app.scroll = scroll;
            app.fold_anchor = None;
            app.reading_anchor = None;
        } else if !timeline.pending {
            app.fold_anchor = None;
        }
    }
    if let Some(anchor) = &app.reading_anchor
        && let Some(row) = ui::resolve_anchor(app, area, anchor)
    {
        app.scroll = row;
        app.reading_anchor = None;
    }
    if app.reading_anchor.is_some() && !ui::timeline(app, area).pending {
        app.reading_anchor = None;
        app.status = "原阅读消息已不在当前页，保留可用滚动位置。".into();
    }
    let timeline = ui::timeline(app, area);
    if !timeline.pending && app.follow_latest {
        app.scroll = timeline.limit();
    }
}
#[allow(clippy::collapsible_if)] // Keep original prompt image overlay routing visually grouped.
fn draw(f: &mut Frame, app: &App) {
    app.history_layout_pending.set(false);
    app.history_frame.borrow_mut().clear();
    app.visible_media.borrow_mut().clear();
    ui::draw(f, app);
    let area = f.area();
    let prompt = ui::prompt_hit_area(app, area);
    app.picker.draw(f.buffer_mut(), area, prompt);
    if app.prompt_focused && app.window_focused && !app.quit_review {
        app.language_menu.draw(f.buffer_mut(), area, prompt);
        app.language_menu.draw_prompt_hint(
            f.buffer_mut(),
            &app.draft.textarea,
            *app.draft_state.borrow(),
            app.composer_area.get(),
        );
    }
    app.image_painted.set(None);
    *app.image_escapes.borrow_mut() = None;
    if area.width >= ui::MIN_WIDTH
        && area.height >= ui::MIN_HEIGHT
        && (app.prompt_focused
            || app.received_image.is_some()
            || app.image_view_loading
            || app.history_image_hover.borrow().is_some())
        && app.window_focused
        && !app.quit_review
        && app.pending.is_none()
        && app.picker.context.is_none()
        && !app.language_menu.open()
    {
        if app.received_image.is_some() || app.image_view_loading {
            if let Some(image) = app.received_image.as_ref() {
                let theme = theme::Theme::current();
                let (_, pixels) = render::image_overlay::render_image_overlay_checked(
                    f.buffer_mut(),
                    ui::history_rect(app, area),
                    image,
                    theme.paste_bg,
                    theme.paste_fg,
                    theme.paste_dim,
                );
                *app.image_escapes.borrow_mut() = pixels;
            } else {
                *app.image_escapes.borrow_mut() = terminal::overlay::clear();
            }
            // Explicit image loading/viewing owns input, but uses the same
            // content-fitted original preview as prompt and hover images.
            app.image_painted.set(Some(
                app.received_image
                    .as_ref()
                    .map_or(0, |image| image.preview.identity()),
            ));
            ui::image_preview_footer(f, app, area);
        } else if let Some((file, number)) = app.history_image_hover.borrow().as_ref() {
            let room = app
                .dots
                .get(app.selected)
                .and_then(|d| d.room.as_deref())
                .unwrap_or("");
            if let Some(mut image) = app.native_media.image(&format!("{room}/{file}")).cloned() {
                image.display_number = *number;
                image.source_path = None; // Remote metadata name is not a user filepath.
                let theme = theme::Theme::current();
                let (painted, pixels) = render::image_overlay::render_image_overlay_checked(
                    f.buffer_mut(),
                    ui::history_rect(app, area),
                    &image,
                    theme.paste_bg,
                    theme.paste_fg,
                    theme.paste_dim,
                );
                if painted {
                    app.image_painted.set(Some(image.preview.identity()));
                }
                *app.image_escapes.borrow_mut() = pixels;
            }
        } else if app.prompt_focused
            && let Some(image) = app.draft.image_for_preview()
        {
            let theme = theme::Theme::current();
            let (painted, pixels) = render::image_overlay::render_image_overlay_checked(
                f.buffer_mut(),
                ui::history_rect(app, area),
                image,
                theme.paste_bg,
                theme.paste_fg,
                theme.paste_dim,
            );
            if painted {
                app.image_painted.set(Some(image.preview.identity()));
            }
            *app.image_escapes.borrow_mut() = pixels;
        }
    }
    if app.image_painted.get().is_none() {
        *app.image_escapes.borrow_mut() = terminal::overlay::clear();
    }
}
fn apply_scroll(app: &mut App, delta: i32, area: Rect) {
    if delta == 0 || app.quit_review {
        return;
    }
    app.history_selection.borrow_mut().clear();
    sync_history_position(app, area);
    if app.fold_anchor.take().is_some() {
        app.reading_anchor = ui::reading_anchor(app);
    }
    let timeline = ui::timeline(app, area);
    if timeline.pending && timeline.total == 0 {
        return;
    }
    if let Some(anchor) = &app.reading_anchor
        && let Some(row) = ui::resolve_anchor_from(&timeline, anchor)
    {
        app.scroll = row;
        app.reading_anchor = None;
    }
    let limit = timeline.limit();
    app.scroll = app
        .scroll
        .min(limit)
        .saturating_add_signed(delta as isize)
        .min(limit);
    app.follow_latest = app.scroll == limit;
    if app.reading_anchor.is_some() {
        app.reading_anchor = ui::reading_anchor_in(&timeline, app.scroll);
    }
}
fn mouse(app: &mut App, m: MouseEvent, area: Rect, tx: &mpsc::Sender<Job>) -> bool {
    if !app.window_focused {
        return false;
    }
    if m.kind == MouseEventKind::Down(MouseButton::Left) {
        app.history_selection.borrow_mut().clear_for_press();
    }
    if app.can_edit() && app.language_menu.open() {
        let inside = app.language_menu.hover(m.column, m.row);
        if inside {
            match m.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if app.language_menu.pointer(m.column, m.row) =>
                {
                    app.complete_language()
                }
                MouseEventKind::ScrollUp => app.language_menu.scroll_selection(-1),
                MouseEventKind::ScrollDown => app.language_menu.scroll_selection(1),
                _ => {}
            }
            return false;
        }
        if m.kind == MouseEventKind::Down(MouseButton::Left) {
            app.language_menu.dismiss();
        }
    }
    if !app.quit_review
        && app.can_edit()
        && app.picker.context.is_some()
        && app.picker.hover(m.column, m.row)
    {
        match m.kind {
            MouseEventKind::Moved => return false,
            MouseEventKind::ScrollUp => {
                app.picker.state.move_selection(-1);
                return false;
            }
            MouseEventKind::ScrollDown => {
                app.picker.state.move_selection(1);
                return false;
            }
            _ => {}
        }
    }
    if !app.quit_review
        && app.can_edit()
        && m.kind == MouseEventKind::Down(MouseButton::Left)
        && app.picker.pointer(m.column, m.row)
    {
        app.choose_image(tx);
        return false;
    }
    if app.received_image.is_some() || app.image_view_loading {
        let position = Position::new(m.column, m.row);
        let inside_preview = app
            .received_image
            .as_ref()
            .and_then(|image| {
                render::image_overlay::image_overlay_rect(ui::history_rect(app, area), image)
            })
            .is_some_and(|popup| popup.contains(position));
        if m.kind == MouseEventKind::Down(MouseButton::Left) && !inside_preview {
            app.received_image = None;
            app.image_view_loading = false;
            app.image_view_epoch = app.image_view_epoch.wrapping_add(1);
            app.focus_in_batch = true;
        }
        return false;
    }
    if !app.quit_review && app.ui_visible && history_selection::motion_or_release(app, m, area) {
        return false;
    }
    if matches!(
        m.kind,
        MouseEventKind::Moved | MouseEventKind::Down(MouseButton::Left)
    ) {
        app.history_pointer
            .set(Some(Position::new(m.column, m.row)));
    } else if matches!(
        m.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown | MouseEventKind::Drag(_)
    ) {
        app.history_pointer.set(None);
    }
    // Grok app/mouse.rs routes Moved to pane/button/link hover only. It does
    // not call PromptWidget::handle_mouse: that handler clears the one-shot
    // post-insert preview on actual prompt clicks/releases/drags/scrolls.
    let bottom_hover = app
        .bottom_hit
        .get()
        .is_some_and(|r| r.contains(Position::new(m.column, m.row)));
    app.bottom_hover.set(bottom_hover);
    if app.ui_visible
        && !app.quit_review
        && bottom_hover
        && app.bottom_hit.get() == upstream_follow::hit(ui::status_rect(app, area))
        && m.kind == MouseEventKind::Down(MouseButton::Left)
        && !app.follow_latest
    {
        app.goto_bottom();
        app.focus_in_batch = true;
        return false;
    }
    if app.ui_visible
        && !app.quit_review
        && m.kind == MouseEventKind::Down(MouseButton::Left)
        && let Some((file, number)) = ui::attachment_hit_with_number(app, area, m.column, m.row)
    {
        app.open_received_image(file, tx);
        app.image_view_number = number.unwrap_or(1);
        if let Some(image) = app.received_image.as_mut() {
            image.display_number = app.image_view_number;
        }
        return false;
    }
    if matches!(m.kind, MouseEventKind::Drag(MouseButton::Left))
        && app.history_press.as_ref().is_some_and(|(_, x, y)| {
            upstream_user_fold::drag_threshold_exceeded((*x, *y), m.column, m.row)
        })
    {
        app.history_press = None;
        app.last_history_click = None;
    }
    if m.kind == MouseEventKind::Up(MouseButton::Left) {
        if app.prompt_mouse_down && app.can_edit() {
            app.draft
                .handle_mouse(m, app.composer_area.get(), *app.draft_state.borrow());
        }
        app.prompt_mouse_down = false;
        if app.ui_visible
            && !app.quit_review
            && app.scrollbar_drag.is_none()
            && let Some((id, x, y)) = app.history_press.take()
            && !upstream_user_fold::drag_threshold_exceeded((x, y), m.column, m.row)
            && ui::message_hit(app, area, m.column, m.row).as_deref() == Some(id.as_str())
        {
            if history_selection::text_click(app, x, y, area) {
                app.scrollbar_drag = None;
                return false;
            }
            let now = Instant::now();
            let count = upstream_user_fold::click_count(
                app.last_history_click
                    .as_ref()
                    .map(|(t, id, c)| (*t, id.as_str(), *c)),
                now,
                &id,
            );
            app.last_history_click = if count >= 3 {
                None
            } else {
                Some((now, id.clone(), count))
            };
            // Exact WordSelect body hits returned above; non-text chrome keeps
            // the original block-level fold action in every selection mode.
            if count == 2
                && let Some(message) = app
                    .messages
                    .iter()
                    .find(|msg| msg.id == id && msg.is_human_message())
                && upstream_user_fold::is_foldable(&message.text)
            {
                let current = upstream_user_fold::expanded(
                    &message.text,
                    app.expanded_messages.get(&id).copied(),
                );
                let next = upstream_user_fold::toggle(&message.text, current);
                // Grok's fold anchor preserves entry virtual-Y plus scroll;
                // omit the dispatcher's separate scroll_to_entry_top navigation.
                app.fold_anchor = ui::capture_fold_anchor(app, area, &id);
                app.reading_anchor = None;
                app.follow_latest = false;
                app.expanded_messages.insert(id, next);
                app.history_changed();
            }
        }
        app.history_press = None;
        app.scrollbar_drag = None;
        return false;
    }
    if !app.ui_visible || app.quit_review || app.scrollbar_drag.is_some_and(|old| old != area) {
        app.scrollbar_drag = None;
    }
    if !app.ui_visible || area.width < ui::MIN_WIDTH || area.height < ui::MIN_HEIGHT {
        app.scrollbar_drag = None;
        app.scroller.cancel();
        return false;
    }
    // Adapted app/mouse.rs pane hit routing: prompt click focuses Prompt,
    // history/other chrome switches to Scrollback. Modal ownership stays above.
    if !app.quit_review && m.kind == MouseEventKind::Down(MouseButton::Left) {
        let prompt = ui::prompt_hit_area(app, area).contains(Position::new(m.column, m.row));
        if app.prompt_focused != prompt {
            app.focus_in_batch = true;
        }
        app.prompt_focused = prompt;
        app.prompt_mouse_down = prompt;
        app.history_press = if prompt {
            None
        } else {
            ui::message_hit(app, area, m.column, m.row).map(|id| (id, m.column, m.row))
        };
        if prompt {
            if app.can_edit() {
                app.draft
                    .handle_mouse(m, app.composer_area.get(), *app.draft_state.borrow());
            }
            return false;
        }
    }
    if app.prompt_mouse_down && matches!(m.kind, MouseEventKind::Drag(MouseButton::Left)) {
        if app.can_edit() {
            app.draft
                .handle_mouse(m, app.composer_area.get(), *app.draft_state.borrow());
        }
        return false;
    }
    if app.ui_visible && !app.quit_review {
        let timeline = ui::timeline(app, area);
        let grab = timeline.track.is_some_and(|track| {
            scrollbar::scrollbar_grab_zone(track).contains(Position::new(m.column, m.row))
        });
        if (m.kind == MouseEventKind::Down(MouseButton::Left) && grab)
            || (m.kind == MouseEventKind::Drag(MouseButton::Left)
                && app.scrollbar_drag == Some(area))
        {
            if let Some(track) = timeline.track {
                app.scroller.cancel();
                app.scrollbar_drag = Some(area);
                app.history_press = None;
                app.last_history_click = None;
                let row = m
                    .row
                    .saturating_sub(track.y)
                    .min(track.height.saturating_sub(1));
                let offset = match scrollbar::scrollbar_click_to_offset(
                    row,
                    track.height,
                    timeline.total,
                    track.height,
                ) {
                    scrollbar::ScrollbarClickResult::Top => 0,
                    scrollbar::ScrollbarClickResult::Bottom => timeline.limit(),
                    scrollbar::ScrollbarClickResult::Offset(n) => n.min(timeline.limit()),
                };
                app.scroll = offset;
                app.follow_latest = offset == timeline.limit();
            } else {
                app.scrollbar_drag = None;
            }
            return false;
        }
    }
    if !app.quit_review && m.kind == MouseEventKind::Down(MouseButton::Left) {
        history_selection::begin(app, m, area);
    }
    match m.kind {
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            app.history_selection.borrow_mut().clear();
            app.scrollbar_drag = None;
            app.history_press = None;
            app.last_history_click = None;
            if app.quit_review {
                return false;
            }
            // Grok app/agent_view/panes.rs ActivePane::Prompt routes wheel
            // events to PromptWidget first; only a non-scrolling textarea
            // falls through to scrollback. Keep the vendored textarea's own
            // viewport, acceleration and cursor-independent scroll override.
            if ui::prompt_hit_area(app, area).contains(Position::new(m.column, m.row))
                && app.can_edit()
                && matches!(
                    app.draft
                        .handle_mouse(m, app.composer_area.get(), *app.draft_state.borrow(),),
                    xai_ratatui_textarea::MouseAction::Scrolled
                )
            {
                app.scroller.cancel();
                return false;
            }
            app.scroller
                .set_viewport_height(ui::viewport_height(app, area));
            let up = m.kind == MouseEventKind::ScrollUp;
            if up && app.scroll == 0 && !app.older_failed && !ui::timeline(app, area).pending {
                request(app, tx, true);
            }
            if up {
                app.follow_latest = false;
            }
            let delta = app.scroller.push(up, Instant::now());
            apply_scroll(app, delta, area);
        }
        MouseEventKind::Down(MouseButton::Left) => {
            app.scroller.cancel();
            app.scrollbar_drag = None;
            if !app.ui_visible {
                return false;
            }
            let action = if ui::history_retry_hit(app, area, m.column, m.row) {
                Some(ui::Action::RetryHistory)
            } else {
                ui::hit_test(area, m.column, m.row)
            };
            if let Some(action) = action {
                use ui::Action;
                if app.quit_review {
                    match action {
                        Action::Confirm => return true,
                        Action::Cancel => app.quit_review = false,
                        _ => {}
                    }
                } else {
                    match action {
                        Action::RetryHistory => request(app, tx, true),
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
    false
}
struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        image_clipboard::cleanup_owned();
        {
            use std::io::Write;
            let _ = io::stdout().write_all(inline_history::clear().as_bytes());
        }
        if let Some(escapes) = terminal::overlay::clear() {
            let _ = terminal::overlay::PostFlush::from(escapes).write_to(&mut io::stdout());
        }
        let _ = disable_raw_mode();
        if std::env::var_os("NO_COLOR").is_none() {
            use std::io::Write;
            let _ = io::stdout().write_all(b"\x1b]112\x07");
        }
        let _ = execute!(
            io::stdout(),
            crossterm::terminal::EndSynchronizedUpdate,
            DisableBracketedPaste,
            DisableMouseCapture,
            DisableFocusChange,
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
    }
}
/// Returns true only for an explicit exit. A confirmation modal owns all keystrokes.
fn key(app: &mut App, k: event::KeyEvent, tx: &mpsc::Sender<Job>) -> bool {
    if !app.window_focused {
        return false;
    }
    app.scroller.cancel();
    app.scrollbar_drag = None;
    if k.kind == KeyEventKind::Release {
        return false;
    }
    if app.quit_review {
        if app.focus_in_batch {
            return false;
        }
        match k.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => return true,
            KeyCode::Esc => app.quit_review = false,
            _ => {}
        }
        return false;
    }
    if app.can_edit() && app.picker.context.is_some() && k.kind == KeyEventKind::Press {
        match k.code {
            KeyCode::Esc => {
                app.picker.dismiss();
                return false;
            }
            KeyCode::Up => {
                app.picker.state.move_selection(-1);
                return false;
            }
            KeyCode::Down => {
                app.picker.state.move_selection(1);
                return false;
            }
            KeyCode::Tab | KeyCode::Enter if k.modifiers.is_empty() => {
                app.choose_image(tx);
                return false;
            }
            _ => {}
        }
    }
    if (app.received_image.is_some() || app.image_view_loading)
        && !(k.modifiers == KeyModifiers::CONTROL && k.code == KeyCode::Char('q'))
    {
        if matches!(k.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q'))
            && k.kind == KeyEventKind::Press
        {
            app.received_image = None;
            app.image_view_loading = false;
            app.image_view_epoch = app.image_view_epoch.wrapping_add(1);
            app.focus_in_batch = true;
        }
        return false;
    }
    if app.can_edit() && app.language_menu.open() && k.kind == KeyEventKind::Press {
        match (k.code, k.modifiers) {
            (KeyCode::Esc, _) => {
                app.language_menu.dismiss();
                app.focus_in_batch = true;
                return false;
            }
            (KeyCode::Up, _) | (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                app.language_menu.move_selection(-1);
                return false;
            }
            (KeyCode::Down, _) | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                app.language_menu.move_selection(1);
                return false;
            }
            (KeyCode::Tab, KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                if app.paste_in_batch
                    || app.focus_in_batch
                    || app
                        .last_paste
                        .is_some_and(|at| at.elapsed() < Duration::from_millis(150))
                {
                    return false;
                }
                if k.code == KeyCode::Tab {
                    app.complete_language();
                } else {
                    app.choose_language();
                }
                return false;
            }
            (KeyCode::PageUp, _) => {
                app.language_menu
                    .scroll_selection(-(slash::MAX_VISIBLE_SUGGESTIONS as isize));
                return false;
            }
            (KeyCode::PageDown, _) => {
                app.language_menu
                    .scroll_selection(slash::MAX_VISIBLE_SUGGESTIONS as isize);
                return false;
            }
            _ => {}
        }
    }
    // Upstream copy ownership: history selection wins before prompt Ctrl+C.
    if k.kind == KeyEventKind::Press
        && k.code == KeyCode::Char('c')
        && matches!(k.modifiers, KeyModifiers::CONTROL | KeyModifiers::SUPER)
        && history_selection::copy_key(app)
    {
        return false;
    }
    app.history_selection.borrow_mut().clear();
    app.history_pointer.set(None);
    // Cropped Grok up-arrow input-history behavior. Never replays a request.
    if k.kind == KeyEventKind::Press
        && k.code == KeyCode::Up
        && k.modifiers.is_empty()
        && app.can_edit()
        && app.pending.is_none()
        && app.draft.text().is_empty()
    {
        app.restore_submitted();
        return false;
    }
    // Grok PromptWidget clears the draft on Ctrl+C; Cmd+C keeps selection copy.
    if k.code == KeyCode::Char('c') && k.modifiers == KeyModifiers::CONTROL {
        if k.kind == KeyEventKind::Press && app.ui_visible && app.can_edit() {
            app.reset_draft();
            app.focus_in_batch = true;
        }
        return false;
    }
    if k.code == KeyCode::Char('c') && k.modifiers == KeyModifiers::SUPER {
        if k.kind == KeyEventKind::Press
            && app.ui_visible
            && app.prompt_focused
            && app.draft.selection_range().is_some()
        {
            app.draft.handle_key(k);
        }
        return false;
    }
    // Original is_paste_key exact Ctrl/Cmd+V predicate, one provider read. Route
    // through the existing bounded/sanitized paste insertion and batch barrier.
    if k.code == KeyCode::Char('v')
        && (k.modifiers == KeyModifiers::CONTROL || k.modifiers == KeyModifiers::SUPER)
    {
        if k.kind == KeyEventKind::Press && app.focus_for_paste() {
            app.paste_in_batch = true;
            app.last_paste = Some(Instant::now());
            #[cfg(any(test, feature = "dev-demo"))]
            if let Some(bytes) = app.image_clipboard_fixture.take() {
                app.stage_image(image_ingress::Input::Bytes(bytes), tx);
                return false;
            }
            if app.live {
                #[cfg(target_os = "macos")]
                let baseline = clipboard_native::clipboard_change_count();
                #[cfg(not(target_os = "macos"))]
                let baseline = None;
                app.stage_image(image_ingress::Input::ClipboardSnapshot(baseline), tx);
            } else if let Some(text) = app.clipboard.get() {
                if let Some(paths) = image_ingress::explicit_paths(&text) {
                    app.stage_image(image_ingress::Input::Paths(paths), tx);
                } else {
                    app.append(&text);
                }
            }
        }
        return false;
    }
    // Original handle_scrollback_key focus predicate (no file viewer/vim route).
    // The focus key is consumed; it is never inserted into the textarea.
    if !app.prompt_focused
        && matches!(k.code, KeyCode::Tab | KeyCode::Char(' '))
        && k.kind == KeyEventKind::Press
        && k.modifiers.is_empty()
        && app.ui_visible
    {
        app.prompt_focused = true;
        app.focus_in_batch = true;
        return false;
    }
    if k.code == KeyCode::Enter {
        if k.kind != KeyEventKind::Press || !app.can_edit() {
            return false;
        }
        // This bypasses ImageComposer::handle_key for the extracted Enter
        // router, so preserve PromptWidget::handle_key's entry transition.
        app.draft.begin_interaction();
        // Match Grok's bare-Enter submit and modified-Enter newline routing.
        // Apple Terminal rescue is captured at input read, before batch queuing.
        let newline = k
            .modifiers
            .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::SUPER);
        let outcome = upstream_enter::route(&mut app.draft, &k, newline);
        match outcome {
            upstream_enter::EnterOutcome::Submit => {
                if !app.paste_in_batch
                    && !app.focus_in_batch
                    && app
                        .last_paste
                        .is_none_or(|at| at.elapsed() >= Duration::from_millis(150))
                {
                    app.submit(tx);
                }
            }
            upstream_enter::EnterOutcome::NewlineInserted => {}
            upstream_enter::EnterOutcome::PassThrough => app.input(k),
        }
        return false;
    }
    if k.modifiers.contains(KeyModifiers::CONTROL) {
        match k.code {
            KeyCode::Char('q') => {
                if app.pending.is_some() {
                    app.quit_review = true;
                } else {
                    return true;
                }
            }
            KeyCode::Home | KeyCode::End if app.can_edit() => {
                let end = app.draft.text().len();
                app.draft
                    .set_cursor(if k.code == KeyCode::Home { 0 } else { end });
            }
            // Ctrl+H and Ctrl+U belong to the original textarea (backspace
            // and kill-to-line-start), not Dot history/clear shortcuts.
            _ => app.input(k),
        }
        return false;
    }
    match k.code {
        KeyCode::PageDown if !app.prompt_focused && k.modifiers.is_empty() => {
            let area = app.timeline_area.get();
            let rows = ui::viewport_height(app, area).saturating_sub(1).max(1);
            apply_scroll(app, rows as i32, area);
        }
        KeyCode::PageUp if !app.prompt_focused && k.modifiers.is_empty() => {
            if app.scroll == 0 && !app.older_failed {
                request(app, tx, true);
            }
            let area = app.timeline_area.get();
            let rows = ui::viewport_height(app, area).saturating_sub(1).max(1);
            apply_scroll(app, -(rows as i32), area);
        }
        KeyCode::Home if !app.prompt_focused && k.modifiers.is_empty() => {
            app.fold_anchor = None;
            app.reading_anchor = None;
            app.follow_latest = false;
            app.scroll = 0;
        }
        KeyCode::End if !app.prompt_focused && k.modifiers.is_empty() => {
            app.goto_bottom();
        }
        _ => app.input(k),
    }
    false
}
// History pixels are content, not input focus. Keep visible placements when
// another window has focus; only occluding UI and viewport changes hide them.
fn paint_inline_history(app: &App, area: Rect) -> String {
    let room = app
        .dots
        .get(app.selected)
        .and_then(|d| d.room.as_deref())
        .unwrap_or("");
    inline_history::paint_with_pending(
        &app.native_media,
        room,
        &app.history_frame.borrow(),
        area,
        app.image_painted.get().is_none()
            && !app.quit_review
            && app.pending.is_none()
            && app.picker.context.is_none()
            && !app.language_menu.open(),
        app.history_layout_pending.get(),
    )
}
// Necessary terminal adapter: the upstream cursor policy is reused verbatim,
// while stock ratatui replaces Grok's custom writer/inline terminal backend.
#[derive(Default)]
struct Presenter {
    cursor: upstream_cursor::CursorState,
    last: Option<ratatui::buffer::Buffer>,
}
impl Presenter {
    fn paint(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        app: &App,
    ) -> io::Result<()> {
        use crossterm::QueueableCommand;
        terminal.autoresize()?;
        let (pos, changed) = {
            let mut frame = terminal.get_frame();
            draw(&mut frame, app);
            let pos = if app.received_image.is_some() || app.image_view_loading {
                None
            } else {
                ui::cursor_position(app, frame.area())
            };
            let buffer = frame.buffer_mut();
            let changed = self.last.as_ref() != Some(buffer);
            self.last = Some(buffer.clone());
            (pos, changed)
        };
        let action = self.cursor.action(pos, changed);
        terminal
            .backend_mut()
            .queue(crossterm::terminal::BeginSynchronizedUpdate)?;
        terminal.flush()?;
        let size = terminal.size()?;
        let inline = paint_inline_history(app, Rect::new(0, 0, size.width, size.height));
        std::io::Write::write_all(terminal.backend_mut(), inline.as_bytes())?;
        if !inline_history::upload_pending()
            && let Some(escapes) = app.image_escapes.borrow_mut().take()
        {
            terminal::overlay::PostFlush::from(escapes).write_to(terminal.backend_mut())?;
        }
        self.cursor.apply(action, terminal.backend_mut());
        terminal.swap_buffers();
        terminal
            .backend_mut()
            .queue(crossterm::terminal::EndSynchronizedUpdate)?;
        std::io::Write::flush(terminal.backend_mut())?;
        app.commit_image_preview();
        Ok(())
    }
}
const MAX_INPUT_BATCH: usize = 64;
const INPUT_BATCH_BUDGET: Duration = Duration::from_millis(4);
#[derive(Debug, PartialEq, Eq)]
enum InputFlow {
    Continue,
    Paint,
    Quit,
}
fn dispatch_input(
    app: &mut App,
    event: Event,
    frame: Rect,
    current: Rect,
    tx: &mpsc::Sender<Job>,
) -> InputFlow {
    // Grok's InputOutcome::Changed repaints completion geometry before the
    // next pointer/key event. Match that contract for Dot's batched input loop.
    let language_was_open = app.language_menu.open();
    let modal = (
        app.quit_review,
        app.pending.is_some(),
        app.prompt_focused,
        app.window_focused,
    );
    let resized = matches!(event, Event::Resize(_, _));
    let quit = match event {
        Event::Key(k) => {
            app.ui_visible &= current == frame
                && current.width >= ui::MIN_WIDTH
                && current.height >= ui::MIN_HEIGHT;
            key(app, k, tx)
        }
        Event::Mouse(m) => {
            if current != frame {
                app.history_selection.borrow_mut().clear();
                app.scrollbar_drag = None;
                false
            } else {
                app.ui_visible = current.width >= ui::MIN_WIDTH && current.height >= ui::MIN_HEIGHT;
                mouse(app, m, current, tx)
            }
        }
        Event::FocusLost => {
            app.history_selection.borrow_mut().clear();
            app.history_pointer.set(None);
            app.window_focused = false;
            app.focus_in_batch = true;
            app.scroller.cancel();
            app.scrollbar_drag = None;
            app.prompt_mouse_down = false;
            app.history_press = None;
            app.last_history_click = None;
            false
        }
        Event::FocusGained => {
            app.window_focused = true;
            app.focus_in_batch = true;
            app.reassert_mouse_capture = true;
            if app.should_restore_prompt_on_focus_gained() {
                app.prompt_focused = true;
            }
            false
        }
        Event::Paste(s) if app.window_focused => {
            app.history_selection.borrow_mut().clear();
            app.history_pointer.set(None);
            app.paste_in_batch = true;
            app.last_paste = Some(Instant::now());
            app.scroller.cancel();
            app.scrollbar_drag = None;
            if app.focus_for_paste() {
                if let Some(paths) = image_ingress::explicit_paths(&s) {
                    app.stage_image(image_ingress::Input::Paths(paths), tx);
                } else {
                    // Original bracketed paste inserts text immediately; native
                    // attachment probing is deferred, text/snapshot checked.
                    #[cfg(target_os = "macos")]
                    let attachment_probe = if app.live
                        && clipboard_paste::paste_payload_needs_clipboard_attachment_probe(&s)
                    {
                        clipboard_paste::attachment_probe_gate(Some(&s))
                    } else {
                        None
                    };
                    app.append(&s);
                    #[cfg(target_os = "macos")]
                    if let Some(change_count) = attachment_probe {
                        app.stage_image(
                            image_ingress::Input::Bracketed {
                                payload: s,
                                change_count,
                            },
                            tx,
                        );
                    }
                }
            }
            false
        }
        Event::Resize(_, _) => {
            app.history_selection.borrow_mut().clear();
            app.history_pointer.set(None);
            if !app.follow_latest && app.reading_anchor.is_none() {
                app.reading_anchor = ui::snapshot_anchor(app);
            }
            app.scroller.cancel();
            app.scrollbar_drag = None;
            app.history_press = None;
            app.last_history_click = None;
            app.ui_visible = false;
            false
        }
        _ => false,
    };
    app.sync_language_menu();
    if app.can_edit() && !app.language_menu.open() {
        if let Some(query) = app.picker.update(app.draft.text(), app.draft.cursor()) {
            let _ = tx.send(Job::FindFiles(query));
        }
    } else {
        app.picker.dismiss();
    }
    app.draft.update_hover();
    app.draft.sync_images_with_textarea();
    if quit {
        InputFlow::Quit
    } else if language_was_open
        || app.language_menu.open()
        || resized
        || modal
            != (
                app.quit_review,
                app.pending.is_some(),
                app.prompt_focused,
                app.window_focused,
            )
    {
        InputFlow::Paint
    } else {
        InputFlow::Continue
    }
}
fn run(args: Args) -> Result<(), String> {
    #[cfg(feature = "dev-demo")]
    if args.demo_image {
        return image_demo::run();
    }
    let demo = args.is_demo();
    // This is an interactive application, not a credential-taking batch command.
    // Launching it directly starts the requested real connection; no repeated banner.
    // A public screenshot fixture is independent of private saved preferences.
    // No preference save path is supplied in this screenshot-only scene.
    let (language_path, language_error) = if args.is_showcase() {
        i18n::set(i18n::Language::English);
        (None, None)
    } else {
        i18n::load()
    };
    require_terminal(io::stdin().is_terminal(), io::stdout().is_terminal())?;
    let mut app = App::new();
    app.language_path = language_path.clone();
    if let Some(error) = language_error {
        app.status = error;
    }
    let mut source = if demo {
        #[cfg(feature = "dev-demo")]
        {
            app = App::demo();
            app.language_path = language_path.clone();
            if args.demo_clipboard_image {
                app.image_clipboard_fixture = Some(image_ingress::synthetic());
            }
            let mut fake = FakeBackend::new();
            fake.delay_ms = args.demo_delay_ms.min(5000);
            fake.incoming = args.demo_incoming;
            if args.demo_history > 0 {
                fake.messages = (0..args.demo_history).map(|i| Message {
                    id: format!("{i:06}"), role: "dot".into(),
                    text: format!("## Offline message {i}\n\n**中文 rich history** with *emphasis* and [link](https://example.com).\n\n```rust\nfn measure_{i}() {{ let values: Vec<_> = (0..100).collect(); println!(\"{{:?}}\", values); }}\n```\n\n{}", "- A wrapped Markdown list with 中文 and inline `code`.\n".repeat(12)),
                    files:vec![],created_at: "2026-11-04T06:29:00Z".into()
                }).collect();
                app.messages = fake
                    .messages
                    .iter()
                    .rev()
                    .take(32)
                    .cloned()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
            }
            if args.demo_inline_fixtures {
                fake.messages = (0..4)
                    .map(|i| Message {
                        id: format!("fixture-{i}"),
                        role: if i % 2 == 0 {
                            "you".into()
                        } else {
                            "dot".into()
                        },
                        text: format!(
                            "Synthetic media {i}: checkerboard / transparency / wide / portrait"
                        ),
                        files: vec![format!("fixture-{i}")],
                        created_at: "2026-11-05T00:00:00Z".into(),
                    })
                    .collect();
                app.messages = fake.messages.clone();
            }
            if args.demo_inline_large_fixtures {
                fake.messages = (0..2)
                    .map(|i| Message {
                        id: format!("large-{i}"),
                        role: "you".into(),
                        text: format!("Offline large PNG {i}"),
                        files: vec![format!("fixture-large-{i}")],
                        created_at: String::new(),
                    })
                    .collect();
                app.messages = fake.messages.clone();
            }
            if args.demo_delay_ms > 0 {
                app.dots.clear();
                app.messages.clear();
                app.begin("开发模拟连接");
            }
            if args.showcase {
                showcase::configure(&mut app, &mut fake);
            }
            Source::Demo(fake)
        }
        #[cfg(not(feature = "dev-demo"))]
        {
            return Err("Development backend is not included in this build.".into());
        }
    } else {
        let path = args
            .auth
            .or_else(|| std::env::var_os("CODEX_HOME").map(|p| PathBuf::from(p).join("auth.json")))
            .or_else(|| platform_fs::home_dir().map(|p| p.join(".codex/auth.json")))
            .ok_or("Cannot locate login path")?;
        diagnostics::init()?;
        app.clipboard.use_system();
        Source::PendingLive(LiveConfig {
            auth_path: path,
            no_proxy: args.no_proxy,
        })
    };
    let (tx, rxj) = mpsc::channel();
    let (txr, rx) = mpsc::channel();
    // Grok defers clipboard reads/preview preparation independently from agent
    // network work. Dot uses one bounded serial worker (max two pending pastes)
    // and the existing draft-epoch reply fence, preserving paste order.
    let (stage_tx, stage_rx) = mpsc::sync_channel(2);
    app.image_stage_tx = Some(stage_tx);
    let stage_reply = txr.clone();
    let fake = !app.live;
    thread::spawn(move || {
        #[cfg(target_os = "macos")]
        if !fake {
            clipboard_native::clipboard_prewarm();
        }
        while let Ok(Job::StageImage(epoch, input)) = stage_rx.recv() {
            let result = image_ingress::stage(input, fake);
            if stage_reply.send(Reply::StagedImage(epoch, result)).is_err() {
                break;
            }
        }
    });
    thread::spawn(move || {
        while let Ok(job) = rxj.recv() {
            if let Job::LoadHistoryImage(dot, file) = job {
                let client = match &source {
                    Source::Live(b) => Some(b.clone()),
                    _ => None,
                };
                #[cfg(any(test, feature = "dev-demo"))]
                let fake = matches!(source, Source::Demo(_));
                #[cfg(not(any(test, feature = "dev-demo")))]
                let fake = false;
                let reply = txr.clone();
                thread::spawn(move || {
                    let room = dot.room.clone().unwrap_or_default();
                    let result = if let Some(client) = client {
                        client.view_image(&dot, &file)
                    } else {
                        #[cfg(any(test, feature = "dev-demo"))]
                        {
                            if fake {
                                demo_image_view(&dot, &file)
                            } else {
                                Err("图片后台未连接".into())
                            }
                        }
                        #[cfg(not(any(test, feature = "dev-demo")))]
                        {
                            let _ = fake;
                            Err("图片后台未连接".into())
                        }
                    }
                    .and_then(|image| {
                        native_media::prepare(image).map_err(|e| format!("本地图片准备：{e}"))
                    });
                    let _ = reply.send(Reply::HistoryImage(room, file, result));
                });
                continue;
            }
            let r = source.perform(job);
            if txr.send(r).is_err() {
                break;
            }
        }
    });
    if app.live || (demo && app.dots.is_empty()) {
        app.begin("准备连接并读取 dot 列表");
        app.last_result = "尚未完成消息读取".into();
        app.status = if app.uncertain {
            "上次发送结果未确认，发送已锁定。请先在官方应用核对。".into()
        } else {
            "读取 dot 列表…".into()
        };
        tx.send(Job::Dots).map_err(|_| "Worker unavailable")?;
    }
    enable_raw_mode().map_err(|_| "Not an interactive terminal")?;
    let _guard = TerminalGuard;
    if std::env::var_os("NO_COLOR").is_none() {
        use std::io::Write;
        let _ = io::stdout().write_all(b"\x1b]12;rgb:c8/c8/c8\x07");
    }
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        crossterm::cursor::Hide, // CursorState starts hidden, matching Grok terminal setup.
        EnableBracketedPaste,
        EnableMouseCapture,
        EnableFocusChange
    )
    .map_err(|_| "Cannot enter terminal")?;
    let _keyboard_session = keyboard_protocol::Session::negotiate();
    app.keyboard_confirmed = _keyboard_session.confirmed;
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        if std::env::var_os("NO_COLOR").is_none() {
            use std::io::Write;
            let _ = io::stdout().write_all(b"\x1b]112\x07");
        }
        let _ = execute!(
            io::stdout(),
            crossterm::terminal::EndSynchronizedUpdate,
            DisableBracketedPaste,
            DisableMouseCapture,
            DisableFocusChange,
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
        original(info);
    }));
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))
        .map_err(|_| "Cannot initialize terminal")?;
    let mut polled = Instant::now();
    let mut last_viewport = None;
    let mut pending_input = std::collections::VecDeque::new();
    let mut presenter = Presenter {
        cursor: upstream_cursor::CursorState::new(),
        last: None,
    };
    loop {
        app.drain(&rx, &tx);
        if app.reassert_mouse_capture {
            execute!(io::stdout(), EnableMouseCapture)
                .map_err(|_| "Cannot restore mouse capture")?;
            app.reassert_mouse_capture = false;
        }
        let size = terminal.size().map_err(|_| "Cannot read terminal size")?;
        app.ui_visible = size.width >= 42 && size.height >= 18;
        let viewport = Rect::new(0, 0, size.width, size.height);
        if last_viewport != Some(viewport) {
            app.history_selection.borrow_mut().clear();
            if !app.follow_latest && app.reading_anchor.is_none() {
                app.reading_anchor = ui::reading_anchor(&app);
            }
            app.scrollbar_drag = None;
            app.scroller.cancel();
        }
        last_viewport = Some(viewport);

        let delta = app.scroller.flush(Instant::now());
        if app.ui_visible {
            apply_scroll(&mut app, delta, viewport);
        }
        if !app.ui_visible {
            app.scroller.cancel();
        } else if !app.quit_review {
            sync_history_position(&mut app, viewport);
            let timeline = ui::timeline(&app, viewport);
            if !timeline.pending || timeline.total > 0 {
                let limit = timeline.limit();
                app.scroll = if app.follow_latest {
                    limit
                } else {
                    app.scroll.min(limit)
                };
            }
        }
        if app.window_focused && app.ui_visible && !app.quit_review {
            history_selection::tick(&mut app, viewport, Instant::now());
        }
        presenter
            .paint(&mut terminal, &app)
            .map_err(|_| "Terminal draw failed")?;
        app.warm_visible_media(&tx);
        let mut wait = app
            .scroller
            .next_flush_in(Instant::now())
            .unwrap_or(Duration::from_millis(100))
            .min(Duration::from_millis(100));
        if let Some(selection_wait) = history_selection::wait(&app) {
            wait = wait.min(selection_wait);
        }
        if inline_history::upload_pending() {
            wait = wait.min(Duration::from_millis(8));
        }
        if pending_input.is_empty() {
            let mut events = Vec::new();
            let mut started = None;
            for index in 0..MAX_INPUT_BATCH {
                if !event::poll(if index == 0 { wait } else { Duration::ZERO })
                    .map_err(|_| "Terminal event failed")?
                {
                    break;
                }
                let started = started.get_or_insert_with(Instant::now);
                events.push(upstream_input::TimedInputEvent::now(
                    event::read().map_err(|_| "Terminal read failed")?,
                ));
                if started.elapsed() >= INPUT_BATCH_BUDGET {
                    break;
                }
            }
            let events = upstream_input::normalize_batch(events);
            app.focus_in_batch = false;
            app.paste_in_batch = events.iter().any(|e| matches!(e.event, Event::Paste(_)));
            pending_input.extend(events);
        }
        let started = Instant::now();
        let mut quit = false;
        for _ in 0..MAX_INPUT_BATCH {
            let Some(event) = pending_input.pop_front() else {
                break;
            };
            let (w, h) = crossterm::terminal::size().map_err(|_| "Cannot read terminal size")?;
            match dispatch_input(&mut app, event.event, viewport, Rect::new(0, 0, w, h), &tx) {
                InputFlow::Quit => {
                    quit = true;
                    break;
                }
                InputFlow::Paint => break,
                InputFlow::Continue => {}
            }
            if started.elapsed() >= INPUT_BATCH_BUDGET {
                break;
            }
        }
        if quit {
            break;
        }
        if polled.elapsed() >= Duration::from_secs(10)
            || app.read_retry_at.is_some_and(|at| Instant::now() >= at)
        {
            polled = Instant::now();
            poll_latest(&mut app, &tx);
        }
    }
    // Restore first, then leave a redacted result in ordinary terminal scrollback.
    drop(terminal);
    drop(_keyboard_session); // Pop while the session's alternate-screen stack is active.
    drop(_guard);
    Ok(())
}
fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("dot-tui: {}", i18n::ui(&e));
        std::process::exit(1)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn press(c: KeyCode) -> event::KeyEvent {
        event::KeyEvent::new(c, KeyModifiers::NONE)
    }
    #[test]
    fn startup_shows_distinct_phases_and_keeps_count() {
        let (mut a, (tx, rx)) = (App::demo(), mpsc::channel());
        a.live = true;
        a.dots.clear();
        a.messages.clear();
        a.begin("准备连接并读取 dot 列表");
        assert!(a.status_line().contains("dot 列表"));
        a.apply(Reply::Dots(Ok(vec![demo_dot()])), &tx);
        assert!(matches!(rx.try_recv(), Ok(Job::Page(..))));
        assert!(a.status_line().contains("读取消息"));
        a.apply(Reply::Page(0, false, Ok(demo_messages())), &tx);
        assert!(a.last_result.contains("2 条"));
        request(&mut a, &tx, false);
        assert!(a.last_result.contains("2 条"));
        assert!(a.busy);
    }
    #[test]
    fn disconnected_worker_is_not_silent_busy() {
        let (mut a, (tx, _)) = (App::demo(), mpsc::channel());
        let (reply_tx, reply_rx) = mpsc::channel();
        drop(reply_tx);
        a.begin("读取消息");
        a.drain(&reply_rx, &tx);
        assert!(a.worker_failed && !a.busy && a.poll_paused);
        assert!(a.status_line().contains("断开"));
    }
    #[test]
    fn single_dot_does_not_silently_select_from_many() {
        let (mut a, (tx, rx)) = (App::demo(), mpsc::channel());
        a.apply(Reply::Dots(Ok(vec![demo_dot(), demo_dot()])), &tx);
        assert!(a.dots.is_empty() && a.poll_paused);
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn past_messages_are_immutable_under_input() {
        let mut a = App::demo();
        let before = a.messages.clone();
        a.append("new draft");
        a.append("\nsecond line");
        assert_eq!(a.messages, before);
    }
    fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn mouse_wheel_clamps_and_pauses_follow() {
        let (mut a, (tx, _rx)) = (App::demo(), mpsc::channel());
        a.messages[0].text = "many rows\n".repeat(100);
        a.expanded_messages.insert("1".into(), true);
        a.scroll = u16::MAX as usize;
        let area = Rect::new(0, 0, 100, 30);
        ui::settle(&a, area);
        let limit = ui::scroll_limit(&a, area);
        mouse(
            &mut a,
            mouse_event(MouseEventKind::ScrollUp, 10, 10),
            area,
            &tx,
        );
        let delta = a
            .scroller
            .flush(Instant::now() + Duration::from_millis(120));
        apply_scroll(&mut a, delta, area);
        assert!(a.scroll < limit && !a.follow_latest);
        apply_scroll(&mut a, 100000, area);
        assert_eq!(a.scroll, limit);
        assert!(a.follow_latest);
    }
    #[test]
    fn late_refresh_does_not_override_user_scroll() {
        let (mut a, (tx, _rx)) = (App::demo(), mpsc::channel());
        a.begin("读取消息");
        a.follow_latest = false;
        a.scroll = 5;
        a.apply(Reply::Page(0, false, Ok(demo_messages())), &tx);
        assert_eq!(a.scroll, 5);
        assert!(!a.follow_latest);
    }
    #[test]
    fn accepted_send_remains_visible_after_refresh() {
        let (mut a, (tx, rx)) = (App::demo(), mpsc::channel());
        a.append("accepted");
        a.submit(&tx);
        let mut fake = Source::Demo(FakeBackend::new());
        a.apply(fake.perform(rx.try_recv().unwrap()), &tx);
        assert!(a.last_result.contains("接受"));
        a.apply(fake.perform(rx.try_recv().unwrap()), &tx);
        assert!(a.last_result.contains("接受") && a.last_result.contains("条消息"));
    }
    #[test]
    fn default_is_real_single_conversation_with_enter_send() {
        let args = Args::try_parse_from(["dot-tui"]).unwrap();
        assert!(!args.is_demo() && !args.no_proxy);
        let a = App::new();
        assert!(a.live && a.dots.is_empty() && a.messages.is_empty() && a.pending.is_none());
    }
    #[test]
    fn explicit_no_proxy_is_available() {
        let args = Args::try_parse_from(["dot-tui", "--no-proxy"]).unwrap();
        assert!(args.no_proxy);
        assert!(Args::try_parse_from(["dot-tui", "--proxy", "http://localhost:7897"]).is_err());
    }
    #[test]
    fn no_startup_consent_switch_or_prompt_is_required() {
        use clap::CommandFactory;
        let help = Args::command().render_long_help().to_string();
        assert!(!help.contains("accept-unofficial-api-risk"));
        assert!(Args::try_parse_from(["dot-tui"]).is_ok());
    }
    #[test]
    fn noninteractive_start_is_rejected_before_account_setup() {
        for (stdin, stdout) in [(false, false), (false, true), (true, false)] {
            assert!(require_terminal(stdin, stdout).is_err());
        }
        assert!(require_terminal(true, true).is_ok());
    }
    #[test]
    fn production_has_no_demo_argument() {
        #[cfg(not(feature = "dev-demo"))]
        {
            assert!(Args::try_parse_from(["dot-tui", "--demo"]).is_err());
            assert!(Args::try_parse_from(["dot-tui", "--showcase"]).is_err());
        }
    }
    #[test]
    fn merge_deduplicates() {
        let mut a = demo_messages();
        merge_messages(&mut a, demo_messages());
        assert_eq!(a.len(), 2)
    }
    #[test]
    fn draw_unicode_and_small() {
        for (w, h) in [(100, 30), (20, 5), (42, 18)] {
            let mut t = Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
            let mut a = App::demo();
            ui::settle(&a, Rect::new(0, 0, w, h));
            t.draw(|f| draw(f, &a)).unwrap();
            a.append("中文\n多行");
            ui::settle(&a, Rect::new(0, 0, w, h));
            t.draw(|f| draw(f, &a)).unwrap();
        }
    }

    #[test]
    fn uncertain_result_preserves_history_and_requires_explicit_submission() {
        let (mut a, (tx, rx)) = (App::demo(), mpsc::channel());
        a.append("test");
        a.submit(&tx);
        assert_eq!(a.draft(), "");
        let mut b = FakeBackend::new();
        b.fail_uncertain = true;
        let mut source = Source::Demo(b);
        a.apply(source.perform(rx.try_recv().unwrap()), &tx);
        assert!(a.uncertain && a.can_edit());
        a.submit(&tx);
        assert!(rx.try_recv().is_err());
        key(&mut a, press(KeyCode::Up), &tx);
        assert_eq!(a.draft(), "test");
        assert!(rx.try_recv().is_err());
        key(&mut a, press(KeyCode::Enter), &tx);
        assert!(matches!(rx.try_recv(), Ok(Job::Send(..))));
    }
    #[test]
    fn late_success_and_failure_never_overwrite_new_input() {
        for result in [
            Ok("accepted".into()),
            Err(SendError::Uncertain("unknown".into())),
            Err(SendError::NotSent("upload failed".into())),
        ] {
            let (tx, rx) = mpsc::channel();
            let mut a = App::demo();
            a.append("first");
            a.submit(&tx);
            let Job::Send(g, p) = rx.try_recv().unwrap() else {
                panic!()
            };
            a.append("next draft");
            assert_eq!(a.draft(), "next draft");
            a.apply(Reply::Sent(g, p.key, result), &tx);
            assert_eq!(a.draft(), "next draft");
        }
    }
    #[test]
    fn unknown_error_survives_background_refresh_without_input_lock() {
        let (tx, _) = mpsc::channel();
        let mut a = App::demo();
        a.uncertain = true;
        a.send_error = Some("message timeout".into());
        a.begin("读取消息");
        assert!(a.status_line().contains("message timeout"));
        a.apply(Reply::Page(0, false, Ok(demo_messages())), &tx);
        assert!(a.status_line().contains("message timeout"));
        a.append("new input");
        assert_eq!(a.draft(), "new input");
    }
    #[test]
    fn read_reply_does_not_drop_queued_send_or_allow_duplicate() {
        let (tx, rx) = mpsc::channel();
        let mut a = App::demo();
        a.begin("读取消息");
        a.append("first");
        a.submit(&tx);
        let Job::Send(g, p) = rx.try_recv().unwrap() else {
            panic!()
        };
        a.append("new draft");
        a.apply(Reply::Page(g, false, Ok(demo_messages())), &tx);
        assert_eq!(a.pending.as_ref().unwrap().key, p.key);
        assert_eq!(a.draft(), "new draft");
        a.submit(&tx);
        assert!(rx.try_recv().is_err());
        a.apply(
            Reply::Sent(g, p.key, Err(SendError::Uncertain("timeout".into()))),
            &tx,
        );
        assert_eq!(a.draft(), "new draft");
        assert!(a.pending.is_none());
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn pending_quit_warns_and_escape_waits() {
        let (mut a, (tx, _rx)) = (App::demo(), mpsc::channel());
        a.append("test");
        a.submit(&tx);
        let k = event::KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert!(!key(&mut a, k, &tx));
        assert!(a.quit_review);
        assert!(!key(&mut a, press(KeyCode::Esc), &tx));
        assert!(!a.quit_review);
    }
    #[test]
    fn draft_preserves_long_text_and_filters_terminal_controls() {
        let mut a = App::demo();
        a.append("\x1b]52;x\x07\n中文");
        assert!(!a.draft().contains('\x1b'));
        a.append(&"a".repeat(17000));
        assert!(a.draft().ends_with(&"a".repeat(17000)));
    }
    #[test]
    fn stale_send_cannot_clear_current_draft() {
        let (mut a, (tx, _rx)) = (App::demo(), mpsc::channel());
        a.append("keep");
        a.apply(Reply::Sent(999, "other".into(), Ok("id".into())), &tx);
        assert_eq!(a.draft(), "keep")
    }
}

#[cfg(test)]
mod interaction_tests;

#[cfg(test)]
mod response_tests;
