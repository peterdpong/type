use gpui::{Bounds, Pixels, Window};

/// Forward tab input through a non-draggable native child view.
pub(crate) fn update(window: &Window, controls: Bounds<Pixels>) {
    #[cfg(all(target_os = "macos", not(test)))]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        if let RawWindowHandle::AppKit(handle) = handle.as_raw() {
            // SAFETY: GPUI owns the NSView; rendering runs on the AppKit main thread.
            unsafe {
                mac::update_view(handle.ns_view.as_ptr().cast(), controls);
            }
        }
    }
    #[cfg(any(not(target_os = "macos"), test))]
    let _ = (window, controls);
}

#[cfg(all(target_os = "macos", not(test)))]
pub(crate) mod mac {
    // objc 0.2 macros reference an undeclared cargo-clippy feature.
    #![allow(unexpected_cfgs)]
    use cocoa::{
        base::{id, nil},
        foundation::{NSPoint, NSRect, NSSize},
    };
    use gpui::{Bounds, Pixels};
    use objc::{
        class,
        declare::ClassDecl,
        msg_send,
        runtime::{BOOL, Class, NO, Object, Sel},
        sel, sel_impl,
    };

    extern "C" fn cannot_move_window(_: &Object, _: Sel) -> BOOL {
        NO
    }

    extern "C" fn accepts_first_mouse(view: &Object, _: Sel, event: id) -> BOOL {
        unsafe {
            let parent: id = msg_send![view, superview];
            if parent == nil {
                NO
            } else {
                msg_send![parent, acceptsFirstMouse: event]
            }
        }
    }

    fn input_class() -> &'static Class {
        Class::get("TypeTabInputView").unwrap_or_else(|| {
            let mut class = ClassDecl::new("TypeTabInputView", class!(NSView)).unwrap();
            unsafe {
                class.add_method(
                    sel!(mouseDownCanMoveWindow),
                    cannot_move_window as extern "C" fn(&Object, Sel) -> BOOL,
                );
                class.add_method(
                    sel!(acceptsFirstMouse:),
                    accepts_first_mouse as extern "C" fn(&Object, Sel, id) -> BOOL,
                );
            }
            class.register()
        })
    }

    /// # Safety
    /// `parent` must be a live NSView on the main thread.
    pub(crate) unsafe fn update_view(parent: id, controls: Bounds<Pixels>) {
        unsafe {
            let class = input_class();
            let children: id = msg_send![parent, subviews];
            let count: usize = msg_send![children, count];
            let mut input = nil;
            for i in 0..count {
                let child: id = msg_send![children, objectAtIndex: i];
                let matches: BOOL = msg_send![child, isKindOfClass: class];
                if matches != NO {
                    input = child;
                    break;
                }
            }
            let bounds: NSRect = msg_send![parent, bounds];
            let flipped: BOOL = msg_send![parent, isFlipped];
            let rect = NSRect::new(
                NSPoint::new(
                    bounds.origin.x + f32::from(controls.left()) as f64,
                    if flipped != NO {
                        bounds.origin.y + f32::from(controls.top()) as f64
                    } else {
                        bounds.origin.y + bounds.size.height - f32::from(controls.bottom()) as f64
                    },
                ),
                NSSize::new(
                    f32::from(controls.size.width) as f64,
                    f32::from(controls.size.height) as f64,
                ),
            );
            if input == nil {
                let allocated: id = msg_send![class, alloc];
                input = msg_send![allocated, initWithFrame: rect];
                let _: () = msg_send![parent, addSubview: input];
                let _: () = msg_send![input, release];
            } else {
                let previous: NSRect = msg_send![input, frame];
                if (
                    previous.origin.x,
                    previous.origin.y,
                    previous.size.width,
                    previous.size.height,
                ) != (
                    rect.origin.x,
                    rect.origin.y,
                    rect.size.width,
                    rect.size.height,
                ) {
                    let _: () = msg_send![input, setFrame: rect];
                }
            }
        }
    }
}
