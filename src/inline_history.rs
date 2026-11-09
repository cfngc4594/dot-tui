//! Grok 2bdd1d6 Apache-2.0: original media placement/crop/delete helpers.
//! Dot adaptation: worker-preencoded uploads, bounded frame output and admission.
//! Kitty requires finishing chunks before ANY other graphics commands; cancel
//! closes the current transfer with an empty final chunk before deleting it.
use crate::{native_media::Store, terminal::image};
use ratatui::layout::Rect;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
};
pub const UPLOAD_FRAME_BYTES: usize = 32 * 1024;
pub const MAX_FRAME_BYTES: usize = 40 * 1024;
#[derive(Clone, Debug)]
pub struct Placement {
    pub instance: String,
    pub file: String,
    pub area: Rect,
    pub full_rows: u16,
    pub top_crop: u16,
}
struct Upload {
    key: String,
    media_key: String,
    id: u32,
    offset: usize,
}
#[derive(Default)]
struct State {
    ids: HashMap<String, u32>,
    next: u32,
    viewport: Rect,
    retired: Vec<u32>,
    shape: Vec<(String, Rect, u16, u16)>,
    upload: Option<Upload>,
}
thread_local! {static STATE:RefCell<State>=RefCell::new(State::default());}
fn cancel(s: &mut State, out: &mut String) {
    if let Some(u) = s.upload.take() {
        if u.offset > 0 {
            out.push_str("\x1b_Gm=0,q=2;\x1b\\");
        }
        out.push_str(&image::clear_kitty_image(u.id));
    }
}
pub fn clear() -> String {
    STATE.with(|state| {
        let mut s = state.borrow_mut();
        let mut out = String::new();
        cancel(&mut s, &mut out);
        for (_, id) in s.ids.drain() {
            out.push_str(&image::clear_kitty_image(id));
        }
        for id in s.retired.drain(..) {
            out.push_str(&image::clear_kitty_image(id));
        }
        s.shape.clear();
        out
    })
}
pub fn upload_pending() -> bool {
    STATE.with(|s| s.borrow().upload.is_some())
}
fn advance(s: &mut State, store: &Store, out: &mut String) {
    let Some(mut u) = s.upload.take() else { return };
    let Some(data) = store.prepared(&u.media_key).and_then(|p| p.transmission()) else {
        s.upload = Some(u);
        cancel(s, out);
        return;
    };
    while u.offset < data.len() {
        let tail = &data[u.offset..];
        let Some(end) = tail.find("\x1b\\").map(|n| n + 2) else {
            s.upload = Some(u);
            cancel(s, out);
            return;
        };
        // Whole original 4096-byte base64 chunks only. Never split APC escapes.
        if out.len() + end + 64 > UPLOAD_FRAME_BYTES {
            break;
        }
        if u.offset == 0 {
            out.push_str(&tail[..end].replacen(",i=0,", &format!(",i={},", u.id), 1));
        } else {
            out.push_str(&tail[..end]);
        }
        u.offset += end;
    }
    if u.offset == data.len() {
        s.ids.insert(u.key, u.id);
    } else {
        s.upload = Some(u);
    }
}
fn place(s: &State, store: &Store, room: &str, placements: &[Placement], out: &mut String) {
    for p in placements.iter().take(crate::native_media::MAX_ENTRIES) {
        let media_key = format!("{room}/{}", p.file);
        let key = format!("{media_key}/{}", p.instance);
        if let Some(id) = s.ids.get(&key)
            && let Some(media) = store.image(&media_key)
            && let Some((w, h)) = media.preview_dimensions()
            && let Some(esc) =
                image::place_inline_image(&[], w, h, p.area, p.full_rows, p.top_crop, *id, false)
        {
            out.push_str(&esc);
        }
    }
}
#[cfg(test)]
pub fn paint(
    store: &Store,
    room: &str,
    placements: &[Placement],
    viewport: Rect,
    enabled: bool,
) -> String {
    paint_with_pending(store, room, placements, viewport, enabled, false)
}
pub fn paint_with_pending(
    store: &Store,
    room: &str,
    placements: &[Placement],
    viewport: Rect,
    enabled: bool,
    layout_pending: bool,
) -> String {
    STATE.with(|state| {
        let mut s = state.borrow_mut();
        let mut out = String::from("\x1b7");
        let placements = if enabled && image::scrollback_inline_overlay_active() {
            placements
        } else {
            &[]
        };
        let shape: Vec<_> = placements
            .iter()
            .take(crate::native_media::MAX_ENTRIES)
            .map(|p| {
                (
                    format!("{room}/{}/{}", p.file, p.instance),
                    p.area,
                    p.top_crop,
                    p.full_rows,
                )
            })
            .collect();
        // Screen coordinates/crops and unrelated entries do not identify the
        // transfer. Keep the same room/file/instance stream through scrolling
        // and asynchronous resize layout gaps, then place at the latest geometry.
        let keep_upload = s.upload.as_ref().is_some_and(|u| {
            enabled
                && image::scrollback_inline_overlay_active()
                && u.media_key.starts_with(&format!("{room}/"))
                && (layout_pending || shape.iter().any(|(key, _, _, _)| key == &u.key))
        });
        if s.upload.is_some() && !keep_upload {
            cancel(&mut s, &mut out);
        }
        if s.viewport != viewport {
            let stale: Vec<_> = s.ids.drain().map(|(_, id)| id).collect();
            s.retired.extend(stale);
            s.viewport = viewport;
        }
        // A partial upload owns the graphics stream exclusively until completed.
        let continuing = s.upload.is_some();
        if continuing {
            advance(&mut s, store, &mut out);
            if s.upload.is_some() {
                out.push_str("\x1b8");
                return out;
            }
        }
        for id in s.retired.drain(..) {
            out.push_str(&image::clear_kitty_image(id));
        }
        if layout_pending && enabled {
            s.shape.clear();
            for id in s.ids.values() {
                out.push_str(&image::clear_kitty_image(*id));
            }
            out.push_str("\x1b8");
            return out;
        }
        let wanted: HashSet<_> = placements
            .iter()
            .take(crate::native_media::MAX_ENTRIES)
            .filter(|p| store.cached(&format!("{room}/{}", p.file)).is_some())
            .map(|p| format!("{room}/{}/{}", p.file, p.instance))
            .collect();
        let gone: Vec<_> = s
            .ids
            .keys()
            .filter(|k| !wanted.contains(*k))
            .cloned()
            .collect();
        for key in gone {
            if let Some(id) = s.ids.remove(&key) {
                out.push_str(&image::clear_kitty_image(id));
            }
        }
        // Defensive terminal compatibility: explicitly remove moved/cropped
        // placements before replacing them. Lowercase d=i keeps cached pixels.
        // Do this only after a partial upload has finished its final chunk.
        let changed = s.shape != shape;
        if changed {
            for (key, area, crop, rows) in &shape {
                if s.shape
                    .iter()
                    .any(|(old_key, old_area, old_crop, old_rows)| {
                        old_key == key && (old_area != area || old_crop != crop || old_rows != rows)
                    })
                    && let Some(id) = s.ids.get(key)
                {
                    out.push_str(&image::clear_kitty_image(*id));
                }
            }
        }
        // Stationary frames need no repeated placement commands. Completion or
        // an explicit clear still forces replay even if geometry is unchanged.
        if changed || continuing {
            place(&s, store, room, placements, &mut out);
        }
        s.shape = shape;
        if !continuing {
            for p in placements.iter().take(crate::native_media::MAX_ENTRIES) {
                let media_key = format!("{room}/{}", p.file);
                let key = format!("{media_key}/{}", p.instance);
                if s.ids.contains_key(&key)
                    || store
                        .prepared(&media_key)
                        .and_then(|p| p.transmission())
                        .is_none()
                {
                    continue;
                }
                // Hide existing placements before a long exclusive stream so
                // scrolling cannot leave their old pixel positions on screen.
                // Lowercase Kitty d=i preserves data for later placement.
                if store
                    .prepared(&media_key)
                    .and_then(|p| p.transmission())
                    .is_some_and(|data| data.len() + out.len() + 512 > UPLOAD_FRAME_BYTES)
                {
                    for id in s.ids.values() {
                        out.push_str(&image::clear_kitty_image(*id));
                    }
                    s.shape.clear();
                }
                s.next = s.next.wrapping_add(1).max(2);
                let id = s.next;
                s.upload = Some(Upload {
                    key,
                    media_key,
                    id,
                    offset: 0,
                });
                advance(&mut s, store, &mut out);
                if s.upload.is_none() {
                    place(&s, store, room, placements, &mut out);
                }
                break;
            }
        }
        out.push_str("\x1b8");
        debug_assert!(out.len() <= MAX_FRAME_BYTES);
        out
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (Store, image::TestProtocolGuard) {
        let guard = image::set_protocol_for_test(image::GraphicsProtocol::Kitty);
        clear();
        let bytes = crate::image_ingress::synthetic_media("fixture-0");
        let mut c = crate::image_composer::ImageComposer::default();
        c.stage_bytes(bytes, None).unwrap();
        let mut pic = c.images.remove(0);
        pic.preview = crate::prompt_images::PromptImagePreview::ready_for_test(
            pic.encoded_bytes.as_ref().unwrap().to_vec(),
            pic.dimensions.unwrap(),
        );
        let mut store = Store::default();
        store.retain(
            "room/file".into(),
            crate::native_media::prepare(pic).unwrap(),
        );
        (store, guard)
    }
    fn placement() -> Placement {
        Placement {
            instance: "m:1".into(),
            file: "file".into(),
            area: Rect::new(4, 3, 40, 8),
            full_rows: 16,
            top_crop: 0,
        }
    }
    #[test]
    fn pixels_transmitted_once_then_cropped_placed_and_deleted_offscreen() {
        let (store, _guard) = setup();
        let v = Rect::new(0, 0, 80, 30);
        let mut p = placement();
        let first = paint(&store, "room", &[p.clone()], v, true);
        assert!(first.contains("a=t"));
        assert!(first.contains("a=p"));
        assert!(first.contains("y=0,"));
        p.top_crop = 4;
        p.area.height = 5;
        let next = paint(&store, "room", &[p], v, true);
        assert!(!next.contains("a=t"));
        assert!(next.contains("y=49,"));
        assert!(next.contains("h=61,"));
        let gone = paint(&store, "room", &[], v, true);
        assert!(gone.contains("a=d"));
        assert!(!clear().contains("a=d"));
    }
    #[test]
    fn resize_retransmits_modal_clears_and_exit_deletes() {
        let (store, _guard) = setup();
        let v = Rect::new(0, 0, 80, 30);
        let p = placement();
        paint(&store, "room", std::slice::from_ref(&p), v, true);
        let resize = paint(
            &store,
            "room",
            std::slice::from_ref(&p),
            Rect::new(0, 0, 90, 32),
            true,
        );
        assert!(resize.contains("a=d"));
        assert!(resize.contains("a=t"));
        let modal = paint(&store, "room", std::slice::from_ref(&p), v, false);
        assert!(modal.contains("a=d"));
        assert!(!modal.contains("a=p"));
        paint(&store, "room", &[p], v, true);
        assert!(clear().contains("a=d"));
    }
    #[test]
    fn repeated_file_in_two_messages_has_distinct_placements() {
        let (store, _guard) = setup();
        let p = placement();
        let mut q = p.clone();
        q.instance = "second:1".into();
        q.area.y = 15;
        let mut output = paint(
            &store,
            "room",
            &[p.clone(), q.clone()],
            Rect::new(0, 0, 80, 30),
            true,
        );
        output.push_str(&paint(
            &store,
            "room",
            &[p, q],
            Rect::new(0, 0, 80, 30),
            true,
        ));
        assert_eq!(output.matches("a=t").count(), 2);
        assert!(output.matches("a=p").count() >= 2);
        clear();
    }
    #[test]
    fn moving_and_cropping_two_instances_clears_only_changed_placements_before_replay() {
        let (store, _guard) = setup();
        let v = Rect::new(0, 0, 80, 30);
        let mut p = placement();
        let mut q = p.clone();
        q.instance = "other:1".into();
        q.area.y = 15;
        let frame =
            |p: &Placement, q: &Placement| paint(&store, "room", &[p.clone(), q.clone()], v, true);
        frame(&p, &q);
        frame(&p, &q);
        let still = frame(&p, &q);
        assert!(!still.contains("a=d") && !still.contains("a=p") && !still.contains("a=t"));
        p.area.y += 1;
        p.top_crop = 2;
        let moved = frame(&p, &q);
        assert_eq!(moved.matches("a=d,d=i").count(), 1);
        assert!(moved.contains("a=d,d=i,i=2,"));
        assert!(!moved.contains("a=d,d=i,i=3,"));
        assert!(moved.find("a=d").unwrap() < moved.find("a=p").unwrap());
        assert!(!moved.contains("a=t"));
        q.top_crop = 3;
        q.area.height -= 1;
        let cropped = frame(&p, &q);
        assert!(cropped.contains("a=d,d=i,i=3,"));
        assert!(!cropped.contains("a=d,d=i,i=2,"));
        assert!(cropped.find("a=d").unwrap() < cropped.find("a=p").unwrap());
        assert!(!cropped.contains("a=t"));
        assert!(!frame(&p, &q).contains("a=d"));
        clear();
    }
    #[test]
    fn unsupported_protocol_never_places_or_reserves_gpu_state() {
        let (store, _guard) = setup();
        let _none = image::set_protocol_for_test(image::GraphicsProtocol::None);
        let output = paint(
            &store,
            "room",
            &[placement()],
            Rect::new(0, 0, 80, 30),
            true,
        );
        assert!(!output.contains("a=t"));
        assert!(!output.contains("a=p"));
        assert!(clear().is_empty());
    }
}
#[cfg(test)]
mod pixel_tests {
    use super::*;
    use base64::Engine;
    #[test]
    fn transmitted_png_decodes_to_real_checkerboard_and_transparent_pixels() {
        let _protocol = image::set_protocol_for_test(image::GraphicsProtocol::Kitty);
        clear();
        let mut proof = image_crate_canvas();
        for (n, file) in ["fixture-0", "fixture-1", "fixture-2", "fixture-3"]
            .iter()
            .enumerate()
        {
            let media = crate::demo_image_view(&crate::demo_dot(), file).unwrap();
            let expected = ::image::load_from_memory(&crate::image_ingress::synthetic_media(file))
                .unwrap()
                .to_rgba8();
            let mut store = Store::default();
            store.retain(
                format!("demo-room/{file}"),
                crate::native_media::prepare(media).unwrap(),
            );
            let p = Placement {
                instance: format!("m{n}"),
                file: file.to_string(),
                area: Rect::new(0, 0, 80, 20),
                full_rows: 20,
                top_crop: 0,
            };
            let output = paint(&store, "demo-room", &[p], Rect::new(0, 0, 80, 30), true);
            let mut payload = String::new();
            for escape in output.split("\x1b_G").skip(1) {
                let sequence = escape.split("\x1b\\").next().unwrap();
                if let Some((header, bytes)) = sequence.split_once(';')
                    && header.contains("m=")
                {
                    payload.push_str(bytes);
                }
            }
            let png = base64::engine::general_purpose::STANDARD
                .decode(payload)
                .unwrap();
            let actual = ::image::load_from_memory(&png).unwrap().to_rgba8();
            assert_eq!(
                actual, expected,
                "terminal payload must contain actual fixture pixels"
            );
            assert_eq!(actual[(1, 1)].0, [235, 35, 75, 255]);
            if *file == "fixture-1" {
                assert_eq!(actual[(200, 10)].0[3], 80);
            }
            let reduced = ::image::imageops::resize(
                &actual,
                240,
                160,
                ::image::imageops::FilterType::Nearest,
            );
            let x = (n % 2) * 260 + 10;
            let y = (n / 2) * 180 + 10;
            ::image::imageops::overlay(&mut proof, &reduced, x as i64, y as i64);
        }
        if let Some(dir) = std::env::var_os("DOT_TEST_EVIDENCE") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            proof.save(dir.join("decoded-protocol-pixels.png")).unwrap();
        }
        clear();
    }
    fn image_crate_canvas() -> ::image::RgbaImage {
        ::image::RgbaImage::from_fn(520, 360, |x, y| {
            ::image::Rgba(if y < 180 {
                [27, 27, 27, 255]
            } else if x < 260 {
                [17, 17, 17, 255]
            } else {
                [60, 60, 60, 255]
            })
        })
    }
}
#[cfg(test)]
mod bounded_tests {
    use super::*;
    fn large() -> (Store, image::TestProtocolGuard, Placement) {
        let guard = image::set_protocol_for_test(image::GraphicsProtocol::Kitty);
        clear();
        let media = crate::demo_image_view(&crate::demo_dot(), "fixture-large-0").unwrap();
        let mut store = Store::default();
        assert!(store.retain(
            "demo-room/fixture-large-0".into(),
            crate::native_media::prepare(media).unwrap()
        ));
        (
            store,
            guard,
            Placement {
                instance: "m".into(),
                file: "fixture-large-0".into(),
                area: Rect::new(3, 3, 70, 20),
                full_rows: 20,
                top_crop: 0,
            },
        )
    }
    fn validate(stream: &str) {
        let mut partial = false;
        for esc in stream.split("\x1b_G").skip(1) {
            let packet = esc.split("\x1b\\").next().unwrap();
            let head = packet.split(';').next().unwrap();
            if partial {
                assert!(
                    head.split(',')
                        .all(|f| f.starts_with("q=") || f.starts_with("m=")),
                    "interleaved graphics during upload: {head}"
                );
            }
            if head.contains("m=1") {
                partial = true;
            }
            if head.contains("m=0") {
                partial = false;
            }
        }
        assert!(!partial, "stream left incomplete");
    }
    #[test]
    fn large_transfer_is_worker_encoded_and_frame_bounded_without_graphics_interleaving() {
        let (store, _guard, p) = large();
        let v = Rect::new(0, 0, 100, 38);
        let mut all = String::new();
        let mut frames = 0;
        loop {
            let output = paint(&store, "demo-room", std::slice::from_ref(&p), v, true);
            assert!(output.len() <= MAX_FRAME_BYTES);
            all.push_str(&output);
            frames += 1;
            if !upload_pending() {
                break;
            }
            assert!(frames < 100);
        }
        assert!(frames > 10);
        assert_eq!(all.matches("a=t,").count(), 1);
        assert!(all.contains("a=p,"));
        validate(&all);
        clear();
    }
    #[test]
    fn moving_during_partial_upload_never_interleaves_delete_and_replaces_after_completion() {
        let (store, _guard, mut p) = large();
        let v = Rect::new(0, 0, 100, 38);
        let mut all = paint(&store, "demo-room", std::slice::from_ref(&p), v, true);
        assert!(upload_pending());
        for n in 0..100 {
            p.top_crop = (n % 3) as u16;
            p.area.y = 3 + (n % 2) as u16;
            let out = paint(&store, "demo-room", std::slice::from_ref(&p), v, true);
            if upload_pending() {
                assert!(!out.contains("a=d") && !out.contains("a=p"));
            }
            all.push_str(&out);
            if !upload_pending() {
                break;
            }
        }
        assert!(!upload_pending());
        assert_eq!(all.matches("a=t,").count(), 1);
        validate(&all);
        p.top_crop += 1;
        let moved = paint(&store, "demo-room", std::slice::from_ref(&p), v, true);
        assert!(moved.find("a=d").unwrap() < moved.find("a=p").unwrap());
        assert!(!moved.contains("a=t"));
        clear();
    }
    #[test]
    fn repeated_resize_modal_and_exit_close_partial_upload_before_any_other_graphics() {
        let (store, _guard, p) = large();
        let mut all = String::new();
        for width in [100, 90, 80, 100, 90, 100] {
            all.push_str(&paint(
                &store,
                "demo-room",
                std::slice::from_ref(&p),
                Rect::new(0, 0, width, 38),
                true,
            ));
            assert!(upload_pending());
        }
        all.push_str(&paint(
            &store,
            "demo-room",
            std::slice::from_ref(&p),
            Rect::new(0, 0, 100, 38),
            false,
        ));
        assert!(!upload_pending());
        validate(&all);
        let mut quit = paint(&store, "demo-room", &[p], Rect::new(0, 0, 100, 38), true);
        quit.push_str(&clear());
        validate(&quit);
        assert!(!upload_pending());
    }
}
#[cfg(test)]
mod continuous_tests {
    use super::*;
    #[test]
    fn still_visible_large_image_finishes_once_while_crop_position_resize_and_layout_change_each_frame()
     {
        let _guard = image::set_protocol_for_test(image::GraphicsProtocol::Kitty);
        clear();
        let media = crate::demo_image_view(&crate::demo_dot(), "fixture-large-0").unwrap();
        let mut store = Store::default();
        store.retain(
            "demo-room/fixture-large-0".into(),
            crate::native_media::prepare(media).unwrap(),
        );
        let mut p = Placement {
            instance: "same".into(),
            file: "fixture-large-0".into(),
            area: Rect::new(3, 4, 70, 12),
            full_rows: 20,
            top_crop: 0,
        };
        let mut stream = String::new();
        let mut finished = false;
        for frame in 0..80 {
            p.area.y = 3 + (frame % 2);
            p.top_crop = frame % 6;
            let viewport = Rect::new(0, 0, 90 + frame % 2, 38);
            let pending = frame > 0 && frame % 3 == 1;
            let placements = if pending {
                &[][..]
            } else {
                std::slice::from_ref(&p)
            };
            let output =
                paint_with_pending(&store, "demo-room", placements, viewport, true, pending);
            assert!(output.len() <= MAX_FRAME_BYTES);
            stream.push_str(&output);
            if frame > 0 && !upload_pending() {
                finished = true;
                break;
            }
        }
        assert!(
            finished,
            "continuous scrolling must not restart indefinitely"
        );
        assert_eq!(stream.matches("a=t,").count(), 1);
        assert!(
            !stream.contains("m=0,q=2;"),
            "no partial-transfer abort while still needed"
        );
        assert!(stream.contains("m=0;"));
        clear();
    }
}
