mod backend;
mod grok_markdown;
mod grok_scroll;
mod journal;
mod sanitize;
mod ui;
use backend::{Backend, Dot, Message, SendError};
use clap::Parser;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use journal::Journal;
use ratatui::prelude::*;
use sanitize::strip_control_chars as clean;
use std::{
    collections::HashMap,
    io::{self, IsTerminal},
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(
    version,
    about = "Native single-conversation dot client. Connects to your real dot; every send requires review."
)]
struct Args {
    /// Read messages without enabling the composer.
    #[arg(long)]
    read_only: bool,
    /// Existing official Codex login file, not an inline token.
    #[arg(long)]
    auth: Option<PathBuf>,
    /// Development only: fake local backend; never reads credentials.
    #[cfg(feature = "dev-demo")]
    #[arg(long, conflicts_with = "auth")]
    demo: bool,
}
impl Args {
    fn is_demo(&self) -> bool {
        #[cfg(feature = "dev-demo")]
        {
            self.demo
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
}
struct App {
    dots: Vec<Dot>,
    selected: usize,
    messages: Vec<Message>,
    scroll: u16,
    status: String,
    busy: bool,
    generation: u64,
    live: bool,
    older_exhausted: bool,
    enable_send: bool,
    drafts: HashMap<String, String>,
    review: Option<SendPlan>,
    review_scroll: u16,
    pending: Option<SendPlan>,
    uncertain: bool,
    quit_review: bool,
    journal: Option<Journal>,
    follow_latest: bool,
    poll_paused: bool,
    ui_visible: bool,
    operation: Option<(String, Instant)>,
    last_result: String,
    worker_failed: bool,
    last_send_accepted: bool,
    scroller: grok_scroll::Scroller,
}
impl App {
    fn new() -> Self {
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
            enable_send: true,
            drafts: HashMap::new(),
            review: None,
            review_scroll: 0,
            pending: None,
            uncertain: false,
            quit_review: false,
            journal: None,
            follow_latest: true,
            poll_paused: false,
            ui_visible: true,
            operation: None,
            last_result: "尚未完成消息读取".into(),
            worker_failed: false,
            last_send_accepted: false,
            scroller: grok_scroll::Scroller::new(),
        }
    }
    #[cfg(any(test, feature = "dev-demo"))]
    fn demo() -> Self {
        let mut app = Self::new();
        app.live = false;
        app.dots = vec![demo_dot()];
        app.messages = demo_messages();
        app.status = "开发模拟 · 不访问账户".into();
        app.last_result = "开发模拟 · 非真实消息".into();
        app
    }
    fn begin(&mut self, phase: &str) {
        self.busy = true;
        self.operation = Some((phase.into(), Instant::now()));
        self.status = phase.into();
    }
    fn status_line(&self) -> String {
        match &self.operation {
            Some((phase, started)) => format!(
                "{phase} · {}秒（本阶段最多30秒）",
                started.elapsed().as_secs()
            ),
            None => self.status.clone(),
        }
    }
    fn worker_failure(&mut self, reason: &str) {
        self.worker_failed = true;
        self.poll_paused = true;
        self.busy = false;
        self.operation = None;
        if self.pending.is_some() {
            self.uncertain = true;
            self.status = format!("{reason}；发送结果不确定，请核对官方应用，勿重发。");
        } else {
            self.status = format!("{reason}；未能确认本次读取结果，请退出后重启。");
        }
    }
    fn watchdog(&mut self, now: Instant) {
        if self.operation.as_ref().is_some_and(|(_, start)| {
            now.saturating_duration_since(*start) >= Duration::from_secs(30)
        }) {
            self.worker_failure("后台阶段超过30秒");
        }
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
    fn draft(&self) -> &str {
        self.dots
            .get(self.selected)
            .and_then(|d| self.drafts.get(&d.id))
            .map(String::as_str)
            .unwrap_or("")
    }
    fn append(&mut self, text: &str) {
        if self.worker_failed
            || !self.enable_send
            || self.review.is_some()
            || self.pending.is_some()
            || self.uncertain
        {
            return;
        }
        if !text.is_empty() {
            self.last_send_accepted = false;
        }
        if let Some(d) = self.dots.get(self.selected) {
            let draft = self.drafts.entry(d.id.clone()).or_default();
            let room = 16000usize.saturating_sub(draft.chars().count());
            draft.extend(clean(text, true).chars().take(room));
        }
    }
    fn review_send(&mut self) {
        if self.worker_failed || !self.enable_send || self.busy || self.uncertain {
            return;
        }
        let Some(dot) = self.dots.get(self.selected).cloned() else {
            return;
        };
        if self.live && dot.room.is_none() {
            self.status = "该 dot 尚无消息房间，请先在官方应用中开始对话。".into();
            return;
        }
        let text = self.draft().to_owned();
        if text.trim().is_empty() {
            return;
        }
        self.review_scroll = 0;
        self.review = Some(SendPlan {
            dot,
            text,
            key: uuid::Uuid::new_v4().to_string(),
        });
    }
    fn confirm_send(&mut self, tx: &mpsc::Sender<Job>) {
        if !self.ui_visible {
            self.status = "窗口过小，请放大并审阅后确认发送。".into();
            return;
        }
        let Some(plan) = self.review.take() else {
            return;
        };
        if self.worker_failed || self.busy || self.uncertain {
            return;
        }
        if let Some(j) = &self.journal {
            if let Err(e) = j.begin(&plan.key) {
                self.uncertain = true;
                self.status = e;
                return;
            }
        }
        self.pending = Some(plan.clone());
        self.begin("提交消息");
        if tx.send(Job::Send(self.generation, plan.clone())).is_err() {
            self.busy = false;
            self.operation = None;
            self.pending = None;
            let cleared = self
                .journal
                .as_ref()
                .map(|j| j.complete(&plan.key).is_ok())
                .unwrap_or(true);
            self.uncertain = !cleared;
            self.status = "后台不可用，消息未发出。".into();
        } else {
            self.status = "正在提交；退出或超时不能撤回请求，请勿重复发送。".into();
        }
    }
    fn apply(&mut self, reply: Reply, tx: &mpsc::Sender<Job>) {
        if self.worker_failed {
            return;
        }
        match reply {
            Reply::Dots(result) => {
                self.busy = false;
                self.operation = None;
                match result {
                    Ok(d) => {
                        // Single-dot client: never silently select a recipient from several.
                        if d.len() > 1 {
                            self.dots.clear();
                            self.poll_paused = true;
                            self.status = "发现多个 dot，无法确定当前会话；请使用官方应用。".into();
                            self.last_result = self.status.clone();
                            return;
                        }
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
                        self.status = e;
                        self.last_result = self.status.clone();
                        self.poll_paused = true
                    }
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
                        let count = messages.len();
                        self.last_result = format!(
                            "{}本页已读取 {count} 条消息",
                            if self.last_send_accepted {
                                "提交已获服务接受；"
                            } else {
                                ""
                            }
                        );
                        let old = self.messages.first().map(|m| m.id.clone());
                        if !older {
                            self.messages = messages;
                            if self.follow_latest {
                                self.scroll = u16::MAX;
                            }
                            self.older_exhausted = count < 32;
                        } else {
                            merge_messages(&mut self.messages, messages);
                            self.older_exhausted =
                                count < 32 || self.messages.first().map(|m| m.id.clone()) == old;
                        }
                        if !self.uncertain {
                            self.status = format!(
                                "已读取 {count} 条；只代表当前分页。Ctrl+R 最新，Ctrl+H 较早。"
                            );
                        }
                    }
                    Err(e) => {
                        self.status = e;
                        self.last_result = self.status.clone();
                        self.poll_paused = true
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
                let plan = self.pending.take().unwrap();
                match result {
                    Ok(_message_id) => {
                        self.last_send_accepted = true;
                        self.last_result = "服务已接受消息（不代表 dot 已完成）".into();
                        let clear = self
                            .journal
                            .as_ref()
                            .map(|j| j.complete(&key))
                            .unwrap_or(Ok(()));
                        self.uncertain = clear.is_err();
                        self.drafts.insert(plan.dot.id, String::new());
                        self.status = clear.err().unwrap_or_else(|| {
                            "服务已接受消息（不代表 dot 已回复）。正在刷新…".into()
                        });
                        self.follow_latest = true;
                        request(self, tx, false);
                    }
                    Err(SendError::Rejected(e)) => {
                        self.uncertain = self
                            .journal
                            .as_ref()
                            .map(|j| j.complete(&key).is_err())
                            .unwrap_or(false);
                        self.status = e;
                    }
                    Err(SendError::Uncertain(e)) => {
                        self.uncertain = true;
                        self.status = e;
                        self.last_result = self.status.clone();
                        self.poll_paused = true;
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
    vec![Message { id:"1".into(), role:"you".into(), text:"请展示加粗、代码块和消息时间，便于对照界面。".into(), created_at:"2026-11-04T06:28:00Z".into() }, Message {
        id:"2".into(), role:"dot".into(),
        text:"这是一段**开发测试消息**，用于对照原版 Markdown 渲染，不来自真实账户。\n\n保留 **加粗**、*斜体*、`行内代码` 和 [源码链接](https://github.com/xai-org/grok-build)。\n\n```rust\nfn main() {\n    println!(\"hello, dot\");\n}\n```\n\n- 消息保持原始顺序\n- 输入框随内容增高\n- 鼠标和触控板滚动\n\n> 发送前仍需审阅确认。".into(),
        created_at:"2026-11-04T06:29:00Z".into(),
    }]
}

enum Job {
    Dots,
    Page(u64, Dot, Option<String>),
    Send(u64, SendPlan),
}
enum Reply {
    Dots(Result<Vec<Dot>, String>),
    Page(u64, bool, Result<Vec<Message>, String>),
    Sent(u64, String, Result<String, SendError>),
}
#[cfg(any(test, feature = "dev-demo"))]
struct FakeBackend {
    messages: Vec<Message>,
    fail_uncertain: bool,
    sends: usize,
}
#[cfg(any(test, feature = "dev-demo"))]
impl FakeBackend {
    fn new() -> Self {
        Self {
            messages: demo_messages(),
            fail_uncertain: false,
            sends: 0,
        }
    }
    fn send(&mut self, text: &str) -> Result<String, SendError> {
        self.sends += 1;
        if self.fail_uncertain {
            return Err(SendError::Uncertain("模拟结果不确定；已锁定发送。".into()));
        }
        let n = self.messages.len() + 1;
        let id = n.to_string();
        self.messages.push(Message {
            id: id.clone(),
            role: "you".into(),
            text: text.into(),
            created_at: format!("{n:06}"),
        });
        self.messages.push(Message {
            id: (n + 1).to_string(),
            role: "dot".into(),
            text: "离线模拟回复：已收到文字，没有访问真实账户。".into(),
            created_at: format!("{:06}", n + 1),
        });
        Ok(id)
    }
}
enum Source {
    PendingLive(PathBuf),
    Failed(String),
    #[cfg(any(test, feature = "dev-demo"))]
    Demo(FakeBackend),
    Live(Backend),
}
impl Source {
    fn perform(&mut self, job: Job) -> Reply {
        if let Self::PendingLive(path) = self {
            *self = match Backend::new(path.clone()) {
                Ok(b) => Self::Live(b),
                Err(e) => Self::Failed(e),
            };
        }
        match job {
            Job::Dots => Reply::Dots(match self {
                #[cfg(any(test, feature = "dev-demo"))]
                Self::Demo(_) => Ok(vec![demo_dot()]),
                Self::Live(b) => b.list_dots(),
                Self::Failed(e) => Err(e.clone()),
                Self::PendingLive(_) => unreachable!(),
            }),
            Job::Page(g, d, before) => Reply::Page(
                g,
                before.is_some(),
                match self {
                    #[cfg(any(test, feature = "dev-demo"))]
                    Self::Demo(b) => Ok(if before.is_some() {
                        vec![]
                    } else {
                        b.messages
                            .iter()
                            .rev()
                            .take(32)
                            .cloned()
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect()
                    }),
                    Self::Live(b) => b.messages(&d, before.as_deref()),
                    Self::Failed(e) => Err(e.clone()),
                    Self::PendingLive(_) => unreachable!(),
                },
            ),
            Job::Send(g, p) => Reply::Sent(
                g,
                p.key.clone(),
                match self {
                    #[cfg(any(test, feature = "dev-demo"))]
                    Self::Demo(b) => b.send(&p.text),
                    Self::Live(b) => b.send(&p.dot, &p.text, &p.key),
                    Self::Failed(e) => Err(SendError::Rejected(e.clone())),
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
    old.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
}
fn request(app: &mut App, tx: &mpsc::Sender<Job>, older: bool) {
    if app.worker_failed || app.busy {
        return;
    }
    if older && (app.older_exhausted || app.messages.len() >= 288) {
        app.status = "已达历史边界或288条安全上限。Ctrl+R 返回最新页。".into();
        return;
    }
    if let Some(d) = app.dots.get(app.selected).cloned() {
        let before = if older {
            app.messages.first().map(|m| m.id.clone())
        } else {
            None
        };
        if tx.send(Job::Page(app.generation, d, before)).is_ok() {
            app.begin("读取消息");
            app.follow_latest = !older;
            if !app.uncertain {
                app.status = "正在读取…".into()
            }
        } else {
            app.worker_failure("后台连接已断开");
        }
    }
}
fn draw(f: &mut Frame, app: &App) {
    ui::draw(f, app);
}
fn apply_scroll(app: &mut App, delta: i32, area: Rect) {
    if delta == 0 || app.quit_review {
        return;
    }
    let limit = ui::scroll_limit(app, area);
    let reviewing = app.review.is_some();
    let scroll = if reviewing {
        &mut app.review_scroll
    } else {
        &mut app.scroll
    };
    *scroll = ((*scroll).min(limit) as i64 + delta as i64).clamp(0, limit as i64) as u16;
    if !reviewing {
        app.follow_latest = *scroll == limit;
    }
}
fn mouse(app: &mut App, m: MouseEvent, area: Rect, tx: &mpsc::Sender<Job>) -> bool {
    match m.kind {
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            if app.quit_review {
                return false;
            }
            app.scroller
                .set_viewport_height(ui::viewport_height(app, area));
            let up = m.kind == MouseEventKind::ScrollUp;
            if up && app.review.is_none() {
                app.follow_latest = false;
            }
            let delta = app.scroller.push(up, Instant::now());
            apply_scroll(app, delta, area);
        }
        MouseEventKind::Down(MouseButton::Left) => {
            app.scroller.cancel();
            if !app.ui_visible {
                return false;
            }
            if let Some(action) = ui::hit_test(area, m.column, m.row) {
                use ui::Action;
                if app.quit_review {
                    match action {
                        Action::Confirm => return true,
                        Action::Cancel => app.quit_review = false,
                        _ => {}
                    }
                } else if app.review.is_some() {
                    match action {
                        Action::Confirm => app.confirm_send(tx),
                        Action::Cancel => app.review = None,
                        _ => {}
                    }
                } else {
                    match action {
                        Action::Review => app.review_send(),
                        Action::Refresh => {
                            return key(
                                app,
                                event::KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
                                tx,
                            );
                        }
                        Action::Earlier => request(app, tx, true),
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
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
    }
}
/// Returns true only for an explicit exit. A confirmation modal owns all keystrokes.
fn key(app: &mut App, k: event::KeyEvent, tx: &mpsc::Sender<Job>) -> bool {
    app.scroller.cancel();
    if k.kind == KeyEventKind::Release {
        return false;
    }
    if app.quit_review {
        match k.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => return true,
            KeyCode::Esc => app.quit_review = false,
            _ => {}
        }
        return false;
    }
    if app.review.is_some() {
        match k.code {
            KeyCode::Esc => app.review = None,
            KeyCode::Char('y') | KeyCode::Char('Y') => app.confirm_send(tx),
            KeyCode::PageDown => app.review_scroll = app.review_scroll.saturating_add(10),
            KeyCode::PageUp => app.review_scroll = app.review_scroll.saturating_sub(10),
            _ => {}
        }
        return false;
    }
    if k.modifiers.contains(KeyModifiers::CONTROL) {
        match k.code {
            KeyCode::Char('q') | KeyCode::Char('c') => {
                if app.pending.is_some() {
                    app.quit_review = true;
                } else {
                    return true;
                }
            }
            KeyCode::Char('s') => app.review_send(),
            KeyCode::Char('r') => {
                if app.worker_failed {
                    return false;
                }
                app.poll_paused = false;
                if app.dots.is_empty() && !app.busy {
                    app.begin("读取 dot 列表");
                    if tx.send(Job::Dots).is_err() {
                        app.worker_failure("后台连接已断开");
                    }
                } else {
                    request(app, tx, false)
                }
            }
            KeyCode::Char('h') => request(app, tx, true),
            KeyCode::Char('u') if app.pending.is_none() && !app.uncertain => {
                if let Some(d) = app.dots.get(app.selected) {
                    app.drafts.remove(&d.id);
                }
            }
            _ => {}
        }
        return false;
    }
    match k.code {
        KeyCode::PageDown => {
            app.follow_latest = false;
            app.scroll = app.scroll.saturating_add(10);
        }
        KeyCode::PageUp => {
            app.follow_latest = false;
            app.scroll = app.scroll.saturating_sub(10);
        }
        KeyCode::Home if !app.enable_send => app.scroll = 0,
        KeyCode::Backspace if app.pending.is_none() && !app.uncertain && app.enable_send => {
            if let Some(d) = app.dots.get(app.selected) {
                app.drafts.entry(d.id.clone()).or_default().pop();
            }
        }
        KeyCode::Enter => app.append("\n"),
        KeyCode::Char(c) => app.append(&c.to_string()),
        _ => {}
    }
    false
}
fn run(args: Args) -> Result<(), String> {
    let demo = args.is_demo();
    // This is an interactive application, not a credential-taking batch command.
    // Launching it directly starts the requested real connection; no repeated banner.
    require_terminal(io::stdin().is_terminal(), io::stdout().is_terminal())?;
    let mut app = App::new();
    app.enable_send = !args.read_only;
    let mut source = if demo {
        #[cfg(feature = "dev-demo")]
        {
            app = App::demo();
            app.enable_send = !args.read_only;
            Source::Demo(FakeBackend::new())
        }
        #[cfg(not(feature = "dev-demo"))]
        {
            return Err("Development backend is not included in this build.".into());
        }
    } else {
        let path = args
            .auth
            .or_else(|| std::env::var_os("CODEX_HOME").map(|p| PathBuf::from(p).join("auth.json")))
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".codex/auth.json")))
            .ok_or("Cannot locate login path")?;
        if app.enable_send {
            let dir = std::env::var_os("HOME")
                .map(|p| PathBuf::from(p).join(".dot-tui-send-state"))
                .ok_or("Cannot locate send state")?;
            let j = Journal::new(dir)?;
            app.uncertain = j.blocked();
            app.journal = Some(j);
        }
        Source::PendingLive(path)
    };
    let (tx, rxj) = mpsc::channel();
    let (txr, rx) = mpsc::channel();
    thread::spawn(move || {
        while let Ok(job) = rxj.recv() {
            let r = source.perform(job);
            if txr.send(r).is_err() {
                break;
            }
        }
    });
    if app.live {
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
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableMouseCapture
    )
    .map_err(|_| "Cannot enter terminal")?;
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
        original(info);
    }));
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))
        .map_err(|_| "Cannot initialize terminal")?;
    let mut polled = Instant::now();
    loop {
        app.drain(&rx, &tx);
        app.watchdog(Instant::now());
        let size = terminal.size().map_err(|_| "Cannot read terminal size")?;
        app.ui_visible = size.width >= 42 && size.height >= 18;
        let viewport = Rect::new(0, 0, size.width, size.height);
        let delta = app.scroller.flush(Instant::now());
        apply_scroll(&mut app, delta, viewport);
        if app.review.is_some() {
            app.review_scroll = app
                .review_scroll
                .min(ui::review_scroll_limit(&app, viewport));
        } else if !app.quit_review {
            let limit = ui::scroll_limit(&app, viewport);
            app.scroll = if app.follow_latest {
                limit
            } else {
                app.scroll.min(limit)
            };
        }
        terminal
            .draw(|f| draw(f, &app))
            .map_err(|_| "Terminal draw failed")?;
        let wait = app
            .scroller
            .next_flush_in(Instant::now())
            .unwrap_or(Duration::from_millis(100))
            .min(Duration::from_millis(100));
        if event::poll(wait).map_err(|_| "Terminal event failed")? {
            match event::read().map_err(|_| "Terminal read failed")? {
                Event::Key(k) => {
                    // A resize between draw and key arrival must never allow blind confirmation.
                    app.ui_visible &= crossterm::terminal::size()
                        .map(|(w, h)| w >= 42 && h >= 18 && w == size.width && h == size.height)
                        .unwrap_or(false);
                    if key(&mut app, k, &tx) {
                        break;
                    }
                }
                Event::Mouse(m) => {
                    let current =
                        crossterm::terminal::size().map_err(|_| "Cannot read terminal size")?;
                    if current != (size.width, size.height) {
                        continue;
                    }
                    app.ui_visible = current.0 >= 42 && current.1 >= 18;
                    if mouse(&mut app, m, Rect::new(0, 0, current.0, current.1), &tx) {
                        break;
                    }
                }
                Event::Paste(s) => {
                    app.scroller.cancel();
                    app.append(&s);
                }
                Event::Resize(_, _) => app.scroller.cancel(),
                _ => {}
            }
        }
        if app.live && polled.elapsed() >= Duration::from_secs(10) {
            polled = Instant::now();
            if !app.busy
                && !app.poll_paused
                && app.follow_latest
                && app.review.is_none()
                && !app.quit_review
            {
                request(&mut app, &tx, false)
            }
        }
    }
    // Restore first, then leave a redacted result in ordinary terminal scrollback.
    drop(terminal);
    drop(_guard);
    eprintln!(
        "dot-tui: {} | {}",
        clean(&app.last_result, false),
        clean(&app.status_line(), false)
    );
    Ok(())
}
fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("dot-tui: {e}");
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
    fn phase_watchdog_stops_refresh_and_ignores_late_results() {
        let (mut a, (tx, rx)) = (App::demo(), mpsc::channel());
        a.begin("读取消息");
        let start = a.operation.as_ref().unwrap().1;
        a.watchdog(start + Duration::from_secs(29));
        assert!(a.busy);
        a.watchdog(start + Duration::from_secs(30));
        assert!(a.worker_failed);
        a.apply(Reply::Page(0, false, Ok(vec![])), &tx);
        request(&mut a, &tx, false);
        assert_eq!(a.messages.len(), 2);
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn pending_send_timeout_preserves_uncertainty() {
        let (mut a, (tx, _rx)) = (App::demo(), mpsc::channel());
        a.append("one attempt");
        a.review_send();
        a.confirm_send(&tx);
        let start = a.operation.as_ref().unwrap().1;
        a.watchdog(start + Duration::from_secs(31));
        assert!(a.uncertain && a.pending.is_some() && a.worker_failed);
        a.append("must not edit");
        assert_eq!(a.draft(), "one attempt");
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
    fn mouse_review_confirm_and_modal_ownership() {
        let (mut a, (tx, rx)) = (App::demo(), mpsc::channel());
        let area = Rect::new(0, 0, 100, 30);
        a.append("click confirmation");
        mouse(
            &mut a,
            mouse_event(MouseEventKind::Down(MouseButton::Left), 90, 28),
            area,
            &tx,
        );
        assert!(a.review.is_some());
        assert!(rx.try_recv().is_err());
        // Clicking refresh through a modal must not enqueue anything.
        mouse(
            &mut a,
            mouse_event(MouseEventKind::Down(MouseButton::Left), 14, 28),
            area,
            &tx,
        );
        assert!(rx.try_recv().is_err());
        mouse(
            &mut a,
            mouse_event(MouseEventKind::Down(MouseButton::Left), 86, 26),
            area,
            &tx,
        );
        assert!(matches!(rx.try_recv(), Ok(Job::Send(..))));
        mouse(
            &mut a,
            mouse_event(MouseEventKind::Down(MouseButton::Left), 86, 26),
            area,
            &tx,
        );
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn mouse_wheel_clamps_and_pauses_follow() {
        let (mut a, (tx, _rx)) = (App::demo(), mpsc::channel());
        a.messages[0].text = "many rows\n".repeat(100);
        a.scroll = u16::MAX;
        let area = Rect::new(0, 0, 100, 30);
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
        a.review_send();
        a.confirm_send(&tx);
        let mut fake = Source::Demo(FakeBackend::new());
        a.apply(fake.perform(rx.try_recv().unwrap()), &tx);
        assert!(a.last_result.contains("接受"));
        a.apply(fake.perform(rx.try_recv().unwrap()), &tx);
        assert!(a.last_result.contains("接受") && a.last_result.contains("条消息"));
    }
    #[test]
    fn default_is_real_single_conversation_with_send_confirmation() {
        let args = Args::try_parse_from(["dot-tui"]).unwrap();
        assert!(!args.is_demo() && !args.read_only);
        let a = App::new();
        assert!(a.live && a.dots.is_empty() && a.messages.is_empty() && a.pending.is_none());
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
        assert!(Args::try_parse_from(["dot-tui", "--demo"]).is_err());
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
            t.draw(|f| draw(f, &a)).unwrap();
            a.append("中文\n多行");
            a.review_send();
            t.draw(|f| draw(f, &a)).unwrap();
        }
    }
    #[test]
    fn review_cancel_has_no_job() {
        let (mut a, (tx, rx)) = (App::demo(), mpsc::channel());
        a.append("hello\nworld");
        a.review_send();
        assert!(a.review.is_some());
        key(&mut a, press(KeyCode::Esc), &tx);
        assert!(rx.try_recv().is_err());
        assert_eq!(a.draft(), "hello\nworld")
    }
    #[test]
    fn fake_roundtrip_requires_confirmation_and_once() {
        let (mut a, (tx, rx)) = (App::demo(), mpsc::channel());
        a.append("中文\n第二行");
        a.review_send();
        assert!(rx.try_recv().is_err());
        key(&mut a, press(KeyCode::Char('y')), &tx);
        key(&mut a, press(KeyCode::Char('y')), &tx);
        let mut backend = Source::Demo(FakeBackend::new());
        let reply = backend.perform(rx.try_recv().unwrap());
        assert!(rx.try_recv().is_err());
        a.apply(reply, &tx);
        let page = backend.perform(rx.try_recv().unwrap());
        a.apply(page, &tx);
        assert_eq!(a.messages.len(), 4);
        assert_eq!(a.draft(), "");
        assert!(!a.busy);
    }
    #[test]
    fn uncertain_locks_no_retry_or_cancel() {
        let (mut a, (tx, rx)) = (App::demo(), mpsc::channel());
        a.append("test");
        a.review_send();
        a.confirm_send(&tx);
        let mut b = FakeBackend::new();
        b.fail_uncertain = true;
        let mut source = Source::Demo(b);
        a.apply(source.perform(rx.try_recv().unwrap()), &tx);
        assert!(a.uncertain);
        a.review_send();
        a.confirm_send(&tx);
        assert!(rx.try_recv().is_err());
        assert_eq!(a.draft(), "test");
    }
    #[test]
    fn pending_quit_warns_and_escape_waits() {
        let (mut a, (tx, _rx)) = (App::demo(), mpsc::channel());
        a.append("test");
        a.review_send();
        a.confirm_send(&tx);
        let k = event::KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert!(!key(&mut a, k, &tx));
        assert!(a.quit_review);
        assert!(!key(&mut a, press(KeyCode::Esc), &tx));
        assert!(!a.quit_review);
    }
    #[test]
    fn draft_limits_and_controls() {
        let mut a = App::demo();
        a.append("\x1b]52;x\x07\n中文");
        assert!(!a.draft().contains('\x1b'));
        a.append(&"a".repeat(17000));
        assert_eq!(a.draft().chars().count(), 16000);
    }
    #[test]
    fn read_only_cannot_compose_or_send() {
        let (mut a, (tx, rx)) = (App::demo(), mpsc::channel());
        a.enable_send = false;
        a.append("not allowed");
        a.review_send();
        a.confirm_send(&tx);
        assert!(a.draft().is_empty());
        assert!(a.review.is_none());
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn hidden_review_cannot_send() {
        let (mut a, (tx, rx)) = (App::demo(), mpsc::channel());
        a.append("must review");
        a.review_send();
        a.ui_visible = false;
        key(&mut a, press(KeyCode::Char('y')), &tx);
        assert!(rx.try_recv().is_err());
        assert!(a.review.is_some());
        assert!(!a.busy);
        a.ui_visible = true;
        key(&mut a, press(KeyCode::Char('y')), &tx);
        assert!(rx.try_recv().is_ok());
    }
    #[test]
    fn stale_send_cannot_clear_current_draft() {
        let (mut a, (tx, _rx)) = (App::demo(), mpsc::channel());
        a.append("keep");
        a.apply(Reply::Sent(999, "other".into(), Ok("id".into())), &tx);
        assert_eq!(a.draft(), "keep")
    }
}
