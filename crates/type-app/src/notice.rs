use gpui::{prelude::*, *};
use markdown_core::appearance;
use std::time::Duration;

const MOTION: Duration = Duration::from_millis(200);
const SUCCESS_TIME: Duration = Duration::from_millis(2200);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Progress,
    Success,
    Error,
}

struct Message {
    presentation: u64,
    title: SharedString,
    detail: Option<SharedString>,
    kind: Kind,
    exiting: bool,
}

#[derive(Default)]
pub(crate) struct Notice {
    message: Option<Message>,
    revision: u64,
    timer: Option<Task<()>>,
}

impl Notice {
    pub fn show(
        &mut self,
        kind: Kind,
        title: &'static str,
        detail: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.timer = None;
        self.revision += 1;
        let presentation = self
            .message
            .as_ref()
            .filter(|m| !m.exiting)
            .map_or(self.revision, |m| m.presentation);
        self.message = Some(Message {
            presentation,
            title: title.into(),
            detail: detail.map(Into::into),
            kind,
            exiting: false,
        });
        if kind == Kind::Success {
            self.schedule_dismiss(SUCCESS_TIME, cx);
        }
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.timer = None;
        self.revision += 1;
        self.message = None;
        cx.notify();
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.timer = None;
        self.schedule_dismiss(Duration::ZERO, cx);
    }

    fn schedule_dismiss(&mut self, delay: Duration, cx: &mut Context<Self>) {
        let revision = self.revision;
        let wait = cx.background_executor().timer(delay);
        self.timer = Some(cx.spawn(async move |this, cx| {
            wait.await;
            let fading = this
                .update(cx, |this, cx| {
                    if this.revision != revision {
                        return false;
                    }
                    if let Some(message) = &mut this.message {
                        message.exiting = true;
                    }
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !fading {
                return;
            }
            cx.background_executor().timer(MOTION).await;
            let _ = this.update(cx, |this, cx| {
                if this.revision == revision {
                    this.message = None;
                    cx.notify();
                }
            });
        }));
    }

    #[cfg(test)]
    pub fn kind(&self) -> Option<Kind> {
        self.message.as_ref().map(|m| m.kind)
    }
}

impl Render for Notice {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(message) = &self.message else {
            return div().into_any_element();
        };
        let exiting = message.exiting;
        let presentation = message.presentation;
        let progress = message.kind == Kind::Progress;
        let error = message.kind == Kind::Error;
        let indicator = if progress {
            div()
                .size(px(6.))
                .rounded_full()
                .bg(rgb(appearance::MUTED))
                .with_animation(
                    "progress",
                    Animation::new(Duration::from_millis(1100))
                        .repeat()
                        .with_easing(pulsating_between(0.3, 1.)),
                    |dot, opacity| dot.opacity(opacity),
                )
                .into_any_element()
        } else {
            div()
                .text_color(rgb(if error { 0x9b483b } else { appearance::TEXT }))
                .child(if error { "!" } else { "✓" })
                .into_any_element()
        };
        div()
            .id("file-notice")
            .debug_selector(|| "file-notice".into())
            .absolute()
            .right(px(20.))
            .top(px(64.))
            .max_w((window.viewport_size().width * 0.4).min(px(360.)))
            .flex()
            .items_start()
            .gap_2()
            .px_3()
            .py_2()
            .rounded_lg()
            .border_1()
            .border_color(rgb(appearance::BORDER))
            .bg(rgb(0xffffff))
            .shadow(vec![BoxShadow {
                color: rgba(0x00000012).into(),
                offset: point(px(0.), px(4.)),
                blur_radius: px(16.),
                spread_radius: px(0.),
            }])
            .occlude()
            .child(
                div()
                    .w(px(14.))
                    .h(px(19.5))
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_shrink_0()
                    .child(indicator),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .child(message.title.clone())
                    .when_some(message.detail.clone(), |view, detail| {
                        view.child(
                            div()
                                .mt_1()
                                .text_color(rgb(appearance::MUTED))
                                .child(detail),
                        )
                    }),
            )
            .when(error, |view| {
                view.child(
                    div()
                        .id("dismiss-notice")
                        .cursor_pointer()
                        .text_color(rgb(appearance::MUTED))
                        .px_1()
                        .child("×")
                        .on_click(cx.listener(|this, _, _, cx| this.dismiss(cx))),
                )
            })
            .with_animation(
                ("notice-motion", presentation * 2 + u64::from(exiting)),
                Animation::new(MOTION).with_easing(ease_out_quint()),
                move |view, progress| {
                    let visibility = if exiting { 1. - progress } else { progress };
                    view.opacity(visibility)
                        .top(px(64. - (1. - visibility) * 8.))
                },
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{Kind, MOTION, Notice, SUCCESS_TIME};
    use gpui::{AppContext, TestAppContext};
    use std::time::Duration;

    #[gpui::test]
    fn success_waits_then_fades_before_removal(cx: &mut TestAppContext) {
        let notice = cx.new(|_| Notice::default());
        notice.update(cx, |n, cx| n.show(Kind::Success, "Saved", None, cx));
        cx.run_until_parked();
        cx.executor().advance_clock(SUCCESS_TIME);
        cx.run_until_parked();
        notice.read_with(cx, |n, _| assert!(n.message.as_ref().unwrap().exiting));
        cx.executor().advance_clock(MOTION);
        cx.run_until_parked();
        notice.read_with(cx, |n, _| assert!(n.message.is_none()));
    }

    #[gpui::test]
    fn previous_exit_cannot_remove_a_new_operation(cx: &mut TestAppContext) {
        let notice = cx.new(|_| Notice::default());
        notice.update(cx, |n, cx| n.show(Kind::Success, "Saved", None, cx));
        cx.run_until_parked();
        cx.executor().advance_clock(SUCCESS_TIME);
        cx.run_until_parked();
        notice.update(cx, |n, cx| n.show(Kind::Progress, "Opening…", None, cx));
        cx.executor().advance_clock(MOTION + SUCCESS_TIME);
        cx.run_until_parked();
        notice.read_with(cx, |n, _| {
            assert_eq!(n.kind(), Some(Kind::Progress));
            assert!(!n.message.as_ref().unwrap().exiting);
        });
        notice.update(cx, |n, cx| n.show(Kind::Success, "Opened", None, cx));
        cx.run_until_parked();
        cx.executor().advance_clock(SUCCESS_TIME);
        cx.run_until_parked();
        cx.executor().advance_clock(MOTION);
        cx.run_until_parked();
        notice.read_with(cx, |n, _| assert!(n.message.is_none()));
    }

    #[gpui::test]
    fn errors_stay_until_dismissed_and_results_do_not_restart_entrance(cx: &mut TestAppContext) {
        let notice = cx.new(|_| Notice::default());
        notice.update(cx, |n, cx| n.show(Kind::Progress, "Saving…", None, cx));
        let presentation = notice.read_with(cx, |n, _| n.message.as_ref().unwrap().presentation);
        notice.update(cx, |n, cx| {
            n.show(
                Kind::Error,
                "Couldn't save",
                Some("File changed on disk".into()),
                cx,
            )
        });
        cx.executor().advance_clock(Duration::from_secs(30));
        cx.run_until_parked();
        notice.read_with(cx, |n, _| {
            assert_eq!(n.kind(), Some(Kind::Error));
            assert_eq!(n.message.as_ref().unwrap().presentation, presentation);
        });
        notice.update(cx, |n, cx| n.dismiss(cx));
        cx.run_until_parked();
        notice.read_with(cx, |n, _| assert!(n.message.as_ref().unwrap().exiting));
        cx.executor().advance_clock(MOTION);
        cx.run_until_parked();
        notice.read_with(cx, |n, _| assert!(n.message.is_none()));
    }
}
