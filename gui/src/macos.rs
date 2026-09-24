use block2::RcBlock;
use objc2::{
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
};
use objc2_app_kit::{
    NSView, NSViewFrameDidChangeNotification, NSWindow, NSWindowButton,
    NSWindowDidExitFullScreenNotification, NSWindowStyleMask,
};
use objc2_foundation::{
    NSNotification, NSNotificationCenter, NSNotificationName, NSObjectProtocol,
};
use std::{
    cell::{Cell, RefCell},
    ptr::NonNull,
    sync::Mutex,
};
use tauri::{Manager, WebviewWindow, WindowEvent};

#[derive(Clone, Copy)]
struct Layout {
    left: f64,
    center_y: f64,
}

struct WindowChrome(Mutex<Layout>);

struct FrameObservers {
    container: usize,
    tokens: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
}
impl Drop for FrameObservers {
    fn drop(&mut self) {
        for token in &self.tokens {
            // These are the observer tokens returned by this notification center.
            unsafe { NSNotificationCenter::defaultCenter().removeObserver((**token).as_ref()) };
        }
    }
}
thread_local! {
    static OBSERVERS: RefCell<Option<FrameObservers>> = const { RefCell::new(None) };
    static POSITIONING: Cell<bool> = const { Cell::new(false) };
}
struct Positioning;
impl Drop for Positioning {
    fn drop(&mut self) {
        POSITIONING.set(false);
    }
}

fn observe_layout(
    window: &WebviewWindow,
    native: &NSWindow,
    close: &NSView,
    parent: &NSView,
    container: &NSView,
) {
    let id = container as *const NSView as usize;
    if OBSERVERS.with_borrow(|current| {
        current
            .as_ref()
            .is_some_and(|current| current.container == id)
    }) {
        return;
    }
    let center = NSNotificationCenter::defaultCenter();
    let observed = window.clone();
    let callback = RcBlock::new(move |_: NonNull<NSNotification>| {
        // Frame notifications are synchronous on AppKit's main thread. Ignore
        // our own changes; reapply after AppKit resets its titlebar during draw.
        if !POSITIONING.get() {
            let _ = position(&observed);
        }
    });
    for view in [close, parent, container] {
        view.setPostsFrameChangedNotifications(true);
    }
    // All objects below belong to this window and send notifications on the main
    // thread. The block captures only a Send + Sync Tauri handle, not NS objects.
    let notifications: [(&NSNotificationName, &AnyObject); 4] = unsafe {
        [
            (NSViewFrameDidChangeNotification, close),
            (NSViewFrameDidChangeNotification, parent),
            (NSViewFrameDidChangeNotification, container),
            (NSWindowDidExitFullScreenNotification, native),
        ]
    };
    let tokens = notifications
        .into_iter()
        .map(|(name, object)| unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(name),
                Some(object),
                None,
                &callback,
            )
        })
        .collect();
    OBSERVERS.with_borrow_mut(|current| {
        *current = Some(FrameObservers {
            container: id,
            tokens,
        })
    });
}

pub fn install(app: &tauri::App) -> tauri::Result<()> {
    app.manage(WindowChrome(Mutex::new(Layout {
        left: 21.0,
        center_y: 28.0,
    })));
    if let Some(window) = app.get_webview_window("main") {
        position(&window)?;
        let observed = window.clone();
        window.on_window_event(move |event| {
            if matches!(
                event,
                WindowEvent::Resized(_)
                    | WindowEvent::ScaleFactorChanged { .. }
                    | WindowEvent::Focused(true)
                    | WindowEvent::ThemeChanged(_)
            ) {
                // AppKit can restore its default placement after fullscreen or
                // appearance changes even when the HTML toolbar did not resize.
                let _ = position(&observed);
            }
        });
    }
    Ok(())
}

pub fn update(window: WebviewWindow, left: f64, center_y: f64) -> Result<(), String> {
    if !left.is_finite()
        || !center_y.is_finite()
        || !(0.0..=128.0).contains(&left)
        || !(14.0..=160.0).contains(&center_y)
    {
        return Err("Invalid window chrome geometry".into());
    }
    *window
        .state::<WindowChrome>()
        .0
        .lock()
        .map_err(|error| error.to_string())? = Layout { left, center_y };
    position(&window).map_err(|error| error.to_string())
}

fn position(window: &WebviewWindow) -> tauri::Result<()> {
    let layout = *window.state::<WindowChrome>().0.lock().unwrap();
    let observed = window.clone();
    window.with_webview(move |webview| {
        // Tauri executes this closure on the AppKit main thread and owns the
        // NSWindow for its duration. No retained native pointer leaves it.
        let Some(window) = (unsafe { webview.ns_window().cast::<NSWindow>().as_ref() }) else {
            return;
        };
        if window.styleMask().contains(NSWindowStyleMask::FullScreen) {
            return;
        }
        let Some(close) = window.standardWindowButton(NSWindowButton::CloseButton) else {
            return;
        };
        let Some(minimize) = window.standardWindowButton(NSWindowButton::MiniaturizeButton) else {
            return;
        };
        // This is the same titlebar container used by Wry's traffic-light inset.
        let Some(parent) = (unsafe { close.superview() }) else {
            return;
        };
        let Some(container) = (unsafe { parent.superview() }) else {
            return;
        };
        POSITIONING.set(true);
        let _positioning = Positioning;
        observe_layout(&observed, window, &close, &parent, &container);
        let close_in_container = close.convertRect_toView(close.bounds(), Some(&container));
        let mut titlebar = container.frame();
        // DOM coordinates start at the top; AppKit coordinates start at the bottom.
        // Use the real native button bounds instead of treating Tauri's inset as
        // the button's top edge (it includes the native titlebar's own padding).
        titlebar.size.height =
            layout.center_y + close_in_container.origin.y + close_in_container.size.height / 2.0;
        titlebar.origin.y = window.frame().size.height - titlebar.size.height;
        if container.frame() != titlebar {
            container.setFrame(titlebar);
        }

        let spacing = minimize.frame().origin.x - close.frame().origin.x;
        for (index, button) in [
            Some(close),
            Some(minimize),
            window.standardWindowButton(NSWindowButton::ZoomButton),
        ]
        .into_iter()
        .enumerate()
        {
            if let Some(button) = button {
                let in_window = button.convertRect_toView(button.bounds(), None);
                let mut origin = NSView::frame(&button).origin;
                origin.x += layout.left + index as f64 * spacing - in_window.origin.x;
                if button.frame().origin != origin {
                    button.setFrameOrigin(origin);
                }
            }
        }
    })
}
