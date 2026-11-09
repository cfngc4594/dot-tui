//! Bounded offline layout worker. Network replies and input remain on their
//! existing paths; Markdown parsing/Syntect and full-history wrapping never run
//! in the interactive paint path. At most one queued job and one reply exist.
use crate::backend::Message;
use ratatui::text::Line;
use std::{
    cell::RefCell,
    sync::{Arc, mpsc},
    thread,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    epoch: u64,
    width: u16,
    inline: bool,
}
pub(super) struct AttachmentHit {
    pub row: usize,
    pub column: u16,
    pub width: u16,
    pub file: String,
    pub height: u16,
    pub image_rows: Option<u16>,
    /// Human attachment chip; shares Grok's image-preview renderer on hover.
    pub preview_number: Option<usize>,
}
pub(super) struct MediaRows {
    pub start: usize,
    pub button: usize,
}
pub(super) struct MessageRows {
    pub id: String,
    pub start: usize,
    pub end: usize,
    pub attachments: Vec<AttachmentHit>,
    pub media: Vec<MediaRows>,
}
pub(super) struct Prepared {
    pub messages: Vec<MessageRows>,
    pub width: u16,
    pub lines: Vec<Line<'static>>,
}
struct Job {
    key: Key,
    messages: Arc<Vec<Message>>,
    expanded: std::collections::HashMap<String, bool>,
    media: std::collections::HashMap<String, crate::upstream_media::Info>,
}
struct Worker {
    tx: mpsc::SyncSender<Job>,
    rx: mpsc::Receiver<(Key, Arc<Prepared>)>,
}
#[derive(Default)]
struct State {
    epoch: u64,
    media: std::collections::HashMap<String, crate::upstream_media::Info>,
    requested: Option<Key>,
    wanted: Option<Key>,
    source: Option<Arc<Vec<Message>>>,
    ready: Option<(Key, Arc<Prepared>)>,
    worker: Option<Worker>,
    submitted: usize,
}
#[derive(Default)]
pub(crate) struct TimelineCache(RefCell<State>);
impl TimelineCache {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn media_error(&self, file: &str) -> Option<String> {
        self.0
            .borrow()
            .media
            .get(file)
            .and_then(|info| info.error.clone())
    }
    pub fn set_media(&self, file: String, info: crate::upstream_media::Info) {
        let mut s = self.0.borrow_mut();
        if s.media.get(&file) != Some(&info) {
            s.media.insert(file, info);
            s.epoch = s.epoch.wrapping_add(1);
            s.requested = None;
        }
    }
    pub fn invalidate(&self) {
        let mut s = self.0.borrow_mut();
        s.epoch = s.epoch.wrapping_add(1);
        s.source = None;
        s.requested = None;
    }
    pub(super) fn get(
        &self,
        messages: &[Message],
        expanded: &std::collections::HashMap<String, bool>,
        width: u16,
        _height: u16,
    ) -> (Option<Arc<Prepared>>, bool) {
        let mut s = self.0.borrow_mut();
        let key = Key {
            epoch: s.epoch,
            width,
            inline: crate::terminal::image::scrollback_inline_overlay_active(),
        };
        s.wanted = Some(key);
        if messages.is_empty() {
            return (
                Some(Arc::new(Prepared {
                    width,
                    lines: Vec::new(),
                    messages: Vec::new(),
                })),
                false,
            );
        }
        if s.worker.is_none() {
            let (tx, jobs) = mpsc::sync_channel::<Job>(1);
            let (replies, rx) = mpsc::sync_channel(1);
            thread::spawn(move || {
                while let Ok(job) = jobs.recv() {
                    // Superseded viewport jobs in the one-slot queue are cheap to drop.
                    let job = jobs.try_recv().unwrap_or(job);
                    let (full, ranges) = super::transcript_layout_media(
                        &job.messages,
                        job.key.width as usize,
                        &job.expanded,
                        &job.media,
                        job.key.inline,
                    );
                    let prepared = Prepared {
                        width: job.key.width,
                        lines: full,
                        messages: ranges,
                    };
                    if replies.send((job.key, Arc::new(prepared))).is_err() {
                        break;
                    }
                }
            });
            s.worker = Some(Worker { tx, rx });
        }
        while let Ok((completed, prepared)) = s.worker.as_ref().unwrap().rx.try_recv() {
            if Some(completed) == s.wanted {
                s.ready = Some((completed, prepared));
            }
        }
        let exact = s.ready.as_ref().is_some_and(|(ready, _)| *ready == key);
        let media = s.media.clone();
        if !exact && s.requested != Some(key) {
            let source = s
                .source
                .get_or_insert_with(|| Arc::new(messages.to_vec()))
                .clone();
            if s.worker
                .as_ref()
                .unwrap()
                .tx
                .try_send(Job {
                    expanded: expanded.clone(),
                    media,
                    key,
                    messages: source,
                })
                .is_ok()
            {
                s.requested = Some(key);
                s.submitted += 1;
            }
        }
        // Grok entry.rs::ensure_cached keys wrapped output by width, not the
        // viewport height. Prompt growth changes clipping/scroll limits only;
        // retain the same content rows and recompute screen geometry in ui.
        // Dot adaptation: this worker cache is still asynchronous on rewrap.
        let usable = s
            .ready
            .as_ref()
            .filter(|(ready, _)| ready.width == width && ready.inline == key.inline)
            .map(|(_, ready)| ready.clone());
        (usable, !exact)
    }
    // Anchor capture may use the last actually displayed snapshot while a new
    // geometry is pending. This never grants stale rendering or hit targets.
    pub(super) fn last_prepared(&self) -> Option<Arc<Prepared>> {
        self.0
            .borrow()
            .ready
            .as_ref()
            .map(|(_, prepared)| prepared.clone())
    }
    #[cfg(test)]
    pub fn submitted(&self) -> usize {
        self.0.borrow().submitted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_queue_drops_stale_results_and_keeps_latest_geometry() {
        let cache = TimelineCache::new();
        let (tx, jobs) = mpsc::sync_channel(1);
        let (replies, rx) = mpsc::sync_channel(1);
        cache.0.borrow_mut().worker = Some(Worker { tx, rx });
        let messages = crate::demo_messages();
        assert!(cache.get(&messages, &Default::default(), 96, 18).1);
        assert!(cache.get(&messages, &Default::default(), 76, 12).1);
        assert_eq!(cache.submitted(), 1, "only one queued snapshot");
        let old = jobs.try_recv().unwrap();
        cache.get(&messages, &Default::default(), 76, 12);
        assert_eq!(cache.submitted(), 2);
        let latest = jobs.try_recv().unwrap();
        replies
            .send((
                old.key,
                Arc::new(Prepared {
                    width: 96,
                    lines: vec![Line::from("old")],
                    messages: Vec::new(),
                }),
            ))
            .unwrap();
        assert!(
            cache
                .get(&messages, &Default::default(), 76, 12)
                .0
                .is_none(),
            "wrong-width result must never paint or own hit targets"
        );
        replies
            .send((
                latest.key,
                Arc::new(Prepared {
                    width: 76,
                    lines: vec![Line::from("latest")],
                    messages: Vec::new(),
                }),
            ))
            .unwrap();
        let (ready, pending) = cache.get(&messages, &Default::default(), 76, 12);
        assert!(!pending);
        assert_eq!(ready.unwrap().lines[0].width(), 6);
        cache.invalidate();
        let (_, pending) = cache.get(&messages, &Default::default(), 76, 12);
        assert!(
            pending,
            "refresh needs a new revision even at the same width"
        );
    }
    #[test]
    fn height_only_resize_retains_exact_frame_without_worker_job() {
        let cache = TimelineCache::new();
        let (tx, jobs) = mpsc::sync_channel(1);
        let (replies, rx) = mpsc::sync_channel(1);
        cache.0.borrow_mut().worker = Some(Worker { tx, rx });
        let messages = crate::demo_messages();
        cache.get(&messages, &Default::default(), 96, 18);
        let job = jobs.try_recv().unwrap();
        replies
            .send((
                job.key,
                Arc::new(Prepared {
                    width: 96,
                    lines: vec![Line::from("retained history")],
                    messages: Vec::new(),
                }),
            ))
            .unwrap();
        let (before, pending) = cache.get(&messages, &Default::default(), 96, 18);
        assert!(!pending);
        for height in [17, 9, 24, 18] {
            let (after, pending) = cache.get(&messages, &Default::default(), 96, height);
            assert!(
                !pending,
                "height-only change must never produce a blank async frame"
            );
            assert!(Arc::ptr_eq(
                before.as_ref().unwrap(),
                after.as_ref().unwrap()
            ));
            assert!(jobs.try_recv().is_err());
        }
        assert_eq!(cache.submitted(), 1);
    }

    #[test]
    fn prompt_growth_keeps_history_anchor_and_current_viewport_geometry() {
        use ratatui::prelude::*;
        let mut app = crate::App::demo();
        app.messages[0].text = "visible-history-line\n".repeat(80);
        app.expanded_messages
            .insert(app.messages[0].id.clone(), true);
        app.follow_latest = false;
        app.scroll = 4;
        let area = Rect::new(0, 0, 100, 30);
        super::super::settle(&app, area);
        let before = super::super::timeline(&app, area);
        let anchor = super::super::reading_anchor(&app).unwrap();
        let jobs = app.timeline_cache.submitted();
        app.append(&"draft line\n".repeat(30));
        // Deliberately no settle/wait: inspect the very first resize frame.
        let after = super::super::timeline(&app, area);
        assert!(!after.pending);
        assert!(Arc::ptr_eq(
            before.prepared.as_ref().unwrap(),
            after.prepared.as_ref().unwrap()
        ));
        assert!(after.content.height < before.content.height);
        assert_eq!(app.timeline_cache.submitted(), jobs);
        assert_eq!(super::super::reading_anchor(&app).unwrap().id, anchor.id);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| super::super::draw(f, &app)).unwrap();
        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(rendered.contains("visible-history-line"));
        assert!(
            super::super::attachment_hit(&app, area, after.content.x, after.content.bottom())
                .is_none()
        );
    }
    #[test]
    fn prompt_resize_reclips_image_and_hit_targets_without_blank_frame() {
        use super::super::{attachment_hit, draw, settle, timeline};
        use ratatui::prelude::*;
        let _protocol = crate::terminal::image::set_protocol_for_test(
            crate::terminal::image::GraphicsProtocol::Kitty,
        );
        let mut app = crate::App::demo();
        app.messages = vec![Message {
            id: "image-fixture".into(),
            role: "dot".into(),
            text: "portrait".into(),
            created_at: String::new(),
            files: vec!["image-file".into()],
        }];
        app.timeline_cache.set_media(
            "image-file".into(),
            crate::upstream_media::Info {
                error: None,
                name: "fixture.png".into(),
                dimensions: (96, 400),
            },
        );
        let area = Rect::new(0, 0, 100, 30);
        settle(&app, area);
        let before = timeline(&app, area);
        let image = before.prepared.as_ref().unwrap().messages[0]
            .attachments
            .iter()
            .find(|hit| hit.image_rows.is_some())
            .unwrap();
        app.scroll = image.row + 3;
        app.follow_latest = false;
        app.append(&"draft\n".repeat(30));
        let after = timeline(&app, area);
        assert!(!after.pending);
        assert!(after.content.height < before.content.height);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
        let placements = app.history_frame.borrow();
        assert_eq!(placements.len(), 1);
        let placement = &placements[0];
        assert_eq!(placement.top_crop, 3);
        assert_eq!(placement.area.y, after.content.y);
        assert!(placement.area.bottom() <= after.content.bottom());
        assert_eq!(
            attachment_hit(&app, area, placement.area.x, placement.area.y),
            Some("image-file".into())
        );
        assert_eq!(
            attachment_hit(&app, area, placement.area.x, after.content.bottom()),
            None
        );
    }
}
