//! Native AppKit input checks in a hidden window.
//! Run with `cargo run -p type-app --example native_header_input` on macOS.
#![allow(unexpected_cfgs)]

#[cfg(all(target_os = "macos", not(test)))]
#[allow(dead_code)]
#[path = "../src/native_header.rs"]
mod native_header;

#[cfg(all(target_os = "macos", not(test)))]
fn main() {
    use cocoa::{
        appkit::{
            NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSWindow,
            NSWindowStyleMask,
        },
        base::{id, nil},
        foundation::{NSAutoreleasePool, NSPoint, NSRect, NSSize},
    };
    use gpui::{Bounds, point, px, size};
    use objc::{
        class,
        declare::ClassDecl,
        msg_send,
        runtime::{BOOL, NO, Object, Sel, YES},
        sel, sel_impl,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    static EVENTS: AtomicUsize = AtomicUsize::new(0);

    extern "C" fn receive(_: &Object, selector: Sel, _: id) {
        let bit = match selector.name() {
            "mouseDown:" => 1,
            "mouseDragged:" => 2,
            "mouseUp:" => 4,
            "scrollWheel:" => 8,
            _ => unreachable!(),
        };
        EVENTS.fetch_or(bit, Ordering::SeqCst);
    }
    extern "C" fn first_mouse(_: &Object, _: Sel, _: id) -> BOOL {
        YES
    }

    unsafe {
        let pool = NSAutoreleasePool::new(nil);
        let app = cocoa::appkit::NSApp();
        app.setActivationPolicy_(
            NSApplicationActivationPolicy::NSApplicationActivationPolicyProhibited,
        );
        let window = NSWindow::alloc(nil).initWithContentRect_styleMask_backing_defer_(
            NSRect::new(NSPoint::new(0., 0.), NSSize::new(960., 780.)),
            NSWindowStyleMask::NSTitledWindowMask
                | NSWindowStyleMask::NSClosableWindowMask
                | NSWindowStyleMask::NSMiniaturizableWindowMask
                | NSWindowStyleMask::NSResizableWindowMask
                | NSWindowStyleMask::NSFullSizeContentViewWindowMask,
            NSBackingStoreType::NSBackingStoreBuffered,
            NO,
        );
        window.setTitlebarAppearsTransparent_(YES);
        let mut class = ClassDecl::new("TypeHeaderFixtureEditor", class!(NSView)).unwrap();
        for selector in [
            sel!(mouseDown:),
            sel!(mouseDragged:),
            sel!(mouseUp:),
            sel!(scrollWheel:),
        ] {
            class.add_method(selector, receive as extern "C" fn(&Object, Sel, id));
        }
        class.add_method(
            sel!(acceptsFirstMouse:),
            first_mouse as extern "C" fn(&Object, Sel, id) -> BOOL,
        );
        let class = class.register();
        let allocated: id = msg_send![class, alloc];
        let editor: id = msg_send![allocated, initWithFrame: NSRect::new(NSPoint::new(0., 0.), NSSize::new(960., 780.))];
        window.setContentView_(editor);
        window.orderOut_(nil);
        let root: id = msg_send![editor, superview];
        let click = NSPoint::new(180., 760.);
        let before: id = msg_send![root, hitTest: click];
        assert_eq!(before, editor);
        let movable: BOOL = msg_send![before, mouseDownCanMoveWindow];
        assert_eq!(
            movable, YES,
            "reproduce the native titlebar drag before the fix"
        );

        native_header::mac::update_view(
            editor,
            Bounds::new(point(px(84.), px(6.)), size(px(640.), px(28.))),
        );
        let input: id = msg_send![root, hitTest: click];
        assert_ne!(input, editor);
        let movable: BOOL = msg_send![input, mouseDownCanMoveWindow];
        assert_eq!(movable, NO);
        let blank: id = msg_send![root, hitTest: NSPoint::new(850., 760.)];
        assert_eq!(blank, editor);
        let movable: BOOL = msg_send![blank, mouseDownCanMoveWindow];
        assert_eq!(movable, YES, "empty header must remain draggable");
        let button: id = msg_send![root, hitTest: NSPoint::new(16., 764.)];
        assert_ne!(
            button, input,
            "traffic lights must keep their native hit target"
        );
        assert_ne!(button, editor);

        let event: id = msg_send![class!(NSEvent), mouseEventWithType: 1usize location: click
            modifierFlags: 0usize timestamp: 0f64 windowNumber: 0isize context: nil
            eventNumber: 0isize clickCount: 1isize pressure: 1f32];
        let _: () = msg_send![input, mouseDown: event];
        let _: () = msg_send![input, mouseDragged: event];
        let _: () = msg_send![input, mouseUp: event];
        let _: () = msg_send![input, scrollWheel: event];
        assert_eq!(
            EVENTS.load(Ordering::SeqCst),
            15,
            "all pointer events reach the editor"
        );
        let accepts: BOOL = msg_send![input, acceptsFirstMouse: event];
        assert_eq!(accepts, YES);
        let focusable: BOOL = msg_send![input, acceptsFirstResponder];
        assert_eq!(
            focusable, NO,
            "the overlay must not take keyboard/IME focus"
        );

        native_header::mac::update_view(
            editor,
            Bounds::new(point(px(84.), px(6.)), size(px(320.), px(28.))),
        );
        let children: id = msg_send![editor, subviews];
        let count: usize = msg_send![children, count];
        assert_eq!(count, 1, "updates reuse the same native input view");
        let former_control: id = msg_send![root, hitTest: NSPoint::new(600., 760.)];
        assert_eq!(
            former_control, editor,
            "shrinking the tabs restores the empty draggable region"
        );
        let current: id = msg_send![root, hitTest: click];
        assert_eq!(current, input);
        let _: () = msg_send![editor, release];
        let _: () = msg_send![window, release];
        let _: () = msg_send![pool, drain];
    }
    println!(
        "Native header input: tab dragging, event forwarding, traffic lights, and resize checks passed."
    );
}

#[cfg(any(not(target_os = "macos"), test))]
fn main() {}
