//! Native tweaks for the overlay window on Windows and macOS, where there is no layer
//! shell: the window must stay above EVE, never take focus from it, and let clicks through
//! to the game. Input is all or nothing per window, so the overlay polls the cursor
//! ([`cursor_position`]) and takes input only while it is over the panel. From the
//! tournament tool's overlays.
//!
//! The only `unsafe` code in the app: plain calls into the window system on the window
//! GPUI created.

#[cfg(windows)]
use gpui_kit::component::ThemeMode;
use gpui_kit::{Pixels, Point, Window, point, px};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Where clicks on the overlay window go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Clicks {
    /// Through the whole window to the game.
    PassThrough,
    /// To the window (cursor over a box or the pencil, dragging).
    Taken,
}

/// Lets clicks through the window to the game, or has the window take them.
pub(crate) fn set_clicks(window: &Window, clicks: Clicks) {
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    apply(handle.as_raw(), clicks);
}

/// The cursor in the window's coordinates, wherever it is on screen (a
/// click-through window gets no mouse events to track it with).
pub(crate) fn cursor_position(window: &Window) -> Option<Point<Pixels>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    cursor(handle.as_raw(), window.scale_factor())
}

/// The `HWND` behind a Win32 window handle.
#[cfg(windows)]
#[expect(
    clippy::as_conversions,
    reason = "raw-window-handle carries the HWND as an integer; Win32 takes it back as the pointer it is"
)]
fn hwnd(handle: &raw_window_handle::Win32WindowHandle) -> windows_sys::Win32::Foundation::HWND {
    handle.hwnd.get() as windows_sys::Win32::Foundation::HWND
}

/// Extended style bits as the signed value `GetWindowLongPtrW` deals in.
#[cfg(windows)]
fn style_bits(bits: u32) -> isize {
    isize::try_from(bits).expect("window style bits fit in isize")
}

#[cfg(windows)]
fn apply(handle: RawWindowHandle, clicks: Clicks) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, HWND_TOPMOST, LWA_ALPHA, SWP_FRAMECHANGED, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos,
        WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
    };

    let RawWindowHandle::Win32(h) = handle else {
        return;
    };
    let hwnd = hwnd(&h);
    // SAFETY: `hwnd` is the live window GPUI just gave us a handle to; this only reads its
    // extended style.
    let mut style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
    // layered (needed for click-through), no taskbar button, never activated by a click:
    // EVE keeps the keyboard
    style |= style_bits(WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE);
    match clicks {
        Clicks::PassThrough => style |= style_bits(WS_EX_TRANSPARENT),
        Clicks::Taken => style &= !style_bits(WS_EX_TRANSPARENT),
    }
    // SAFETY: as above; this only changes the window's extended style.
    unsafe { SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style) };
    never_activate(hwnd);
    // a layered window needs its opacity set once, or it stays invisible
    // SAFETY: as above; this only sets the window's opacity.
    unsafe { SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA) };
    // SAFETY: as above; this only changes the window's z-order and redraws its frame.
    unsafe {
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        )
    };
}

/// Give the window the light or dark app icon, as the theme goes. The exe's own icon, the
/// one Explorer shows, stays the light one.
#[cfg(windows)]
pub(crate) fn set_window_icon(window: &Window, mode: ThemeMode) {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_SHARED, LoadImageW, SM_CXICON,
        SM_CXSMICON, SendMessageW, WM_SETICON,
    };

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        return;
    };
    let hwnd = hwnd(&h);
    // Resources 1 and 2, embedded by build.rs, named by number the way MAKEINTRESOURCE does.
    let id = std::ptr::without_provenance::<u16>(if mode.is_dark() { 2 } else { 1 });
    // SAFETY: a null name asks for this process's own module, which lives as long as the
    // process.
    let module = unsafe { GetModuleHandleW(std::ptr::null()) };
    for (which, metric) in [(ICON_SMALL, SM_CXSMICON), (ICON_BIG, SM_CXICON)] {
        // SAFETY: reads a system metric; no pointers involved.
        let size = unsafe { GetSystemMetrics(metric) };
        // SAFETY: `module` is ours and `id` names one of its icon resources. A shared icon is
        // kept by Windows for as long as the process runs.
        let icon = unsafe { LoadImageW(module, id, IMAGE_ICON, size, size, LR_SHARED) };
        if !icon.is_null() {
            let which = usize::try_from(which).expect("ICON_SMALL and ICON_BIG are 0 and 1");
            // SAFETY: `hwnd` is live and `icon` a valid shared icon handle.
            unsafe { SendMessageW(hwnd, WM_SETICON, which, icon.addr().cast_signed()) };
        }
    }
}

/// GPUI answers every click with "activate", which `WS_EX_NOACTIVATE` cannot overrule, so
/// a click on the overlay would take the keyboard from EVE (and minimize it when it runs
/// full screen). Sit in front of GPUI's window procedure and answer that one message.
#[cfg(windows)]
fn never_activate(hwnd: windows_sys::Win32::Foundation::HWND) {
    use std::sync::atomic::{AtomicIsize, Ordering};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, GWLP_WNDPROC, GetWindowLongPtrW, MA_NOACTIVATE, SetWindowLongPtrW,
        WM_MOUSEACTIVATE, WNDPROC,
    };

    // GPUI's procedure, which every other message goes on to. One overlay at a time.
    static GPUI: AtomicIsize = AtomicIsize::new(0);

    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_wrap,
        reason = "MA_NOACTIVATE is 3, and LRESULT is the slot a window procedure answers in"
    )]
    const NO_ACTIVATE: LRESULT = MA_NOACTIVATE as LRESULT;

    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if msg == WM_MOUSEACTIVATE {
            return NO_ACTIVATE;
        }
        // SAFETY: GPUI holds the procedure that was installed before ours, so the stored
        // value is a live `WNDPROC` for this window.
        let gpui = unsafe { std::mem::transmute::<isize, WNDPROC>(GPUI.load(Ordering::Relaxed)) };
        // SAFETY: hands the message on, unchanged, to the procedure it was meant for.
        unsafe { CallWindowProcW(gpui, hwnd, msg, wp, lp) }
    }

    #[expect(
        clippy::as_conversions,
        reason = "SetWindowLongPtrW takes the new window procedure as a pointer-sized integer"
    )]
    let ours = (proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT as usize)
        .cast_signed();
    // SAFETY: `hwnd` is live; this only reads its window procedure.
    if unsafe { GetWindowLongPtrW(hwnd, GWLP_WNDPROC) } == ours {
        return;
    }
    // SAFETY: `hwnd` is live; swapping its procedure is what subclassing is, and `ours` hands
    // every other message to the one it replaces.
    let gpui = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, ours) };
    GPUI.store(gpui, Ordering::Relaxed);
}

#[cfg(target_os = "macos")]
fn apply(handle: RawWindowHandle, clicks: Clicks) {
    use objc2_app_kit::{NSView, NSWindowCollectionBehavior};

    let RawWindowHandle::AppKit(h) = handle else {
        return;
    };
    // SAFETY: `ns_view` is the content view of the live window GPUI created,
    // and GPUI renders (and calls us) on the main thread.
    let view: &NSView = unsafe { h.ns_view.cast::<NSView>().as_ref() };
    let Some(window) = view.window() else {
        return;
    };
    window.setIgnoresMouseEvents(clicks == Clicks::PassThrough);
    // on every Space, over full-screen apps (EVE in its own Space), not in
    // the window cycle, and stays put when the app is in the background
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    window.setHidesOnDeactivate(false);
}

/// A screen coordinate in physical pixels (GPUI is per-monitor DPI aware) as logical pixels.
#[cfg(windows)]
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "screen coordinates are far below 2^24, where f32 is exact"
)]
fn logical(physical: i32, scale: f32) -> Pixels {
    px(physical as f32 / scale)
}

#[cfg(windows)]
fn cursor(handle: RawWindowHandle, scale: f32) -> Option<Point<Pixels>> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let RawWindowHandle::Win32(h) = handle else {
        return None;
    };
    let hwnd = hwnd(&h);
    let mut p = POINT { x: 0, y: 0 };
    // SAFETY: `p` is a local the call writes the cursor's screen position into.
    let found = unsafe { GetCursorPos(&raw mut p) } != 0;
    // SAFETY: `hwnd` is the live window GPUI gave us a handle to and `p` a local the call
    // rewrites in that window's coordinates.
    let ok = found && unsafe { ScreenToClient(hwnd, &raw mut p) } != 0;
    ok.then(|| point(logical(p.x, scale), logical(p.y, scale)))
}

/// A distance in screen points as GPUI pixels.
#[cfg(target_os = "macos")]
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "screen points are far inside f32's range, and a fraction of a point is invisible"
)]
fn points(v: f64) -> Pixels {
    px(v as f32)
}

#[cfg(target_os = "macos")]
fn cursor(handle: RawWindowHandle, _scale: f32) -> Option<Point<Pixels>> {
    use objc2_app_kit::{NSEvent, NSView};

    let RawWindowHandle::AppKit(h) = handle else {
        return None;
    };
    // SAFETY: as in `apply`
    let view: &NSView = unsafe { h.ns_view.cast::<NSView>().as_ref() };
    let frame = view.window()?.frame();
    // screen points, origin bottom-left; the borderless window's content
    // fills its frame, and GPUI's origin is top-left
    let m = NSEvent::mouseLocation();
    Some(point(
        points(m.x - frame.origin.x),
        points(frame.origin.y + frame.size.height - m.y),
    ))
}
