//! Extracted from Grok Build 2bdd1d6 xai-grok-shared/src/clipboard.rs (Apache-2.0).
//! Dot adapter: use the existing attachment byte budget before native allocation.
#![allow(dead_code)]
use std::sync::OnceLock;
pub(crate) struct ImageData {
    pub(crate) data: Vec<u8>,
    mime_type: String,
}
fn image_pasteable_from_types<'a>(types: impl IntoIterator<Item = &'a [u8]>) -> bool {
    let mut has_image = false;
    for ty in types {
        if matches!(ty, b"public.file-url" | b"NSFilenamesPboardType") {
            return false;
        }
        if matches!(ty, b"public.png" | b"public.tiff" | b"public.jpeg") {
            has_image = true;
        }
    }
    has_image
}
const NATIVE_IMAGE_TYPES: &[(&[u8], &str)] = &[
    (b"public.png", "image/png"),
    (b"public.tiff", "image/tiff"),
    (b"public.jpeg", "image/jpeg"),
];
fn native_image_type_from_types(
    types: &[impl AsRef<[u8]>],
) -> Option<(&'static [u8], &'static str)> {
    if !image_pasteable_from_types(types.iter().map(|t| t.as_ref())) {
        return None;
    }
    for (uti, mime) in NATIVE_IMAGE_TYPES {
        if types.iter().any(|t| t.as_ref() == *uti) {
            return Some((uti, mime));
        }
    }
    None
}
static PASTEBOARD_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
fn appkit_loaded() -> bool {
    static LOADED: OnceLock<bool> = OnceLock::new();
    *LOADED.get_or_init(|| {
        let path = c"/System/Library/Frameworks/AppKit.framework/AppKit";
        // SAFETY: dlopen with a constant NUL-terminated path; the handle
        // is intentionally leaked (AppKit stays loaded for the process).
        let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_LAZY) };
        !handle.is_null()
    })
}
pub(crate) fn clipboard_prewarm() {
    // Framework load only (memoised); reads no pasteboard, so this is sound to run on the off-UI-thread warm-up
    let _ = appkit_loaded();
}
fn general_pasteboard() -> Option<objc2::rc::Retained<objc2::runtime::AnyObject>> {
    if !appkit_loaded() {
        return None;
    }
    let cls = objc2::runtime::AnyClass::get(c"NSPasteboard")?;
    // SAFETY: +[NSPasteboard generalPasteboard] takes no arguments and
    // returns the shared pasteboard instance (nullable-safe via Option).
    unsafe { objc2::msg_send![cls, generalPasteboard] }
}
pub(crate) fn clipboard_image_snapshot() -> (Option<u64>, bool) {
    let _guard = PASTEBOARD_LOCK.lock();
    objc2::rc::autoreleasepool(|_| {
        let Some(pb) = general_pasteboard() else {
            return (None, false);
        };
        // SAFETY: -[NSPasteboard changeCount] returns NSInteger; it is
        // monotonic and non-negative, so the cast to u64 is lossless.
        let count: isize = unsafe { objc2::msg_send![&*pb, changeCount] };
        // Collect the advertised identifiers, then classify with the shared rule (`image_pasteable_from_types`)
        // A raster type only counts when no file-URL type is advertised alongside, matching the paste-path priority (file URLs win)
        let has_image = advertised_types(&pb).is_some_and(|advertised| {
            image_pasteable_from_types(advertised.iter().map(|t| t.as_slice()))
        });
        (Some(count as u64), has_image)
    })
}
pub(crate) fn clipboard_change_count() -> Option<u64> {
    let _guard = PASTEBOARD_LOCK.lock();
    objc2::rc::autoreleasepool(|_| {
        let pb = general_pasteboard()?;
        // SAFETY: -[NSPasteboard changeCount] returns a monotonic, non-negative NSInteger; the cast to u64 is lossless. This
        // messages ONLY changeCount — no `types` scan, no data read — so it is the cheapest possible pasteboard touch for the
        // throttled poll.
        let count: isize = unsafe { objc2::msg_send![&*pb, changeCount] };
        Some(count as u64)
    })
}
fn advertised_types(pb: &objc2::runtime::AnyObject) -> Option<Vec<Vec<u8>>> {
    // SAFETY: -[NSPasteboard types] returns a nullable
    // NSArray<NSPasteboardType>; only count/objectAtIndex/UTF8String
    // are messaged on it, all data-free.
    let types: Option<objc2::rc::Retained<objc2::runtime::AnyObject>> =
        unsafe { objc2::msg_send![pb, types] };
    let types = types?;
    let n: usize = unsafe { objc2::msg_send![&*types, count] };
    let mut advertised: Vec<Vec<u8>> = Vec::with_capacity(n);
    for i in 0..n {
        let ty: Option<objc2::rc::Retained<objc2::runtime::AnyObject>> =
            unsafe { objc2::msg_send![&*types, objectAtIndex: i] };
        let Some(ty) = ty else { continue };
        let utf8: *const std::os::raw::c_char = unsafe { objc2::msg_send![&*ty, UTF8String] };
        if utf8.is_null() {
            continue;
        }
        // SAFETY: UTF8String is NUL-terminated and outlives this
        // iteration (the owning NSString is retained above).
        let bytes = unsafe { std::ffi::CStr::from_ptr(utf8) }.to_bytes();
        advertised.push(bytes.to_vec());
    }
    Some(advertised)
}
pub(crate) fn native_image_read() -> Option<ImageData> {
    if std::env::var_os("GROK_CLIPBOARD_NO_NATIVE_READ").is_some() {
        return None;
    }
    let _guard = PASTEBOARD_LOCK.lock();
    objc2::rc::autoreleasepool(|_| {
        let pb = general_pasteboard()?;
        let advertised = advertised_types(&pb)?;
        let (uti, mime) = native_image_type_from_types(&advertised)?;
        let uti_cstring = std::ffi::CString::new(uti).ok()?;
        let cls = objc2::runtime::AnyClass::get(c"NSString")?;
        // SAFETY: +[NSString stringWithUTF8String:] takes a NUL-terminated
        // C string (valid for the duration of the call) and returns a
        // nullable autoreleased NSString.
        let ns_type: Option<objc2::rc::Retained<objc2::runtime::AnyObject>> =
            unsafe { objc2::msg_send![cls, stringWithUTF8String: uti_cstring.as_ptr()] };
        let ns_type = ns_type?;
        // SAFETY: -[NSPasteboard dataForType:] takes an NSPasteboardType
        // (NSString) and returns a nullable NSData; only requested for a
        // type the pasteboard itself advertised.
        let data: Option<objc2::rc::Retained<objc2::runtime::AnyObject>> =
            unsafe { objc2::msg_send![&*pb, dataForType: &*ns_type] };
        let data = data?;
        // SAFETY: -[NSData length] returns NSUInteger.
        let len: usize = unsafe { objc2::msg_send![&*data, length] };
        if len == 0 || len > crate::attachments::MAX_FILE_BYTES {
            return None;
        }
        // SAFETY: -[NSData bytes] returns a pointer valid for `len` bytes
        // while `data` is retained (it is, until the end of this scope);
        // the bytes are copied out immediately.
        let bytes_ptr: *const std::os::raw::c_void = unsafe { objc2::msg_send![&*data, bytes] };
        if bytes_ptr.is_null() {
            return None;
        }
        let mut buf = vec![0u8; len];
        // SAFETY: source is valid for `len` reads (NSData contract),
        // destination is a fresh Vec of exactly `len` bytes, and the
        // regions cannot overlap.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes_ptr as *const u8, buf.as_mut_ptr(), len);
        }
        Some(ImageData {
            data: buf,
            mime_type: mime.to_owned(),
        })
    })
}
