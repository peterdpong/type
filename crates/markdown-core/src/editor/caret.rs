use super::{Face, MarkdownEditor, heading_text_run};
use gpui::{Bounds, Context, FontId, Pixels, Point, Task, TextSystem, Window, point, px, size};
use std::time::{Duration, Instant};

const MOVE: Duration = Duration::from_millis(85);
const HOLD: Duration = Duration::from_millis(500);
const FADE: Duration = Duration::from_millis(100);
const HALF_CYCLE: Duration = Duration::from_millis(500);
const CYCLE: Duration = Duration::from_secs(1);
const WIDTH: f32 = 1.;

#[derive(Clone, Copy, Debug)]
pub(super) struct Visual {
    pub bounds: Bounds<Pixels>,
    pub opacity: f32,
}

impl Visual {
    fn snap_to_pixels(mut self, scale: f32) -> Self {
        let snap = |value: Pixels| (value * scale).round() / scale;
        let top = snap(self.bounds.top());
        let bottom = snap(self.bounds.bottom());
        let width = snap(self.bounds.size.width).max(px(1. / scale));
        self.bounds = Bounds::new(
            point(snap(self.bounds.right()) - width, top),
            size(width, (bottom - top).max(px(1. / scale))),
        );
        self
    }
}

struct Motion {
    from: Bounds<Pixels>,
    started: Instant,
}

pub(super) struct Caret {
    target: Option<Bounds<Pixels>>,
    motion: Option<Motion>,
    viewport: Option<(Bounds<Pixels>, Pixels)>,
    idle_since: Instant,
    pending_motion: bool,
    steady: bool,
    timer: Option<Task<()>>,
    wake_at: Option<Instant>,
}

impl Caret {
    pub fn new(now: Instant) -> Self {
        Self {
            target: None,
            motion: None,
            viewport: None,
            idle_since: now,
            pending_motion: false,
            steady: false,
            timer: None,
            wake_at: None,
        }
    }

    pub fn activity(&mut self, now: Instant) {
        self.idle_since = now;
        self.pending_motion = true;
        self.timer = None;
        self.wake_at = None;
    }

    fn bounds(&self, now: Instant) -> Option<Bounds<Pixels>> {
        let target = self.target?;
        let Some(motion) = &self.motion else {
            return Some(target);
        };
        let t = (now.saturating_duration_since(motion.started).as_secs_f32() / MOVE.as_secs_f32())
            .min(1.);
        let eased = 1. - (1. - t).powi(3);
        Some(Bounds::new(
            motion.from.origin + (target.origin - motion.from.origin) * eased,
            size(
                target.size.width,
                motion.from.size.height + (target.size.height - motion.from.size.height) * eased,
            ),
        ))
    }

    fn place(
        &mut self,
        target: Option<Bounds<Pixels>>,
        viewport: (Bounds<Pixels>, Pixels),
        steady: bool,
        now: Instant,
    ) {
        let previous = self.bounds(now);
        let snap = steady || self.viewport != Some(viewport) || previous.is_none();
        if previous.is_none() || self.steady != steady {
            self.idle_since = now;
        }
        if snap || target.is_none() {
            self.motion = None;
        } else if target != self.target {
            self.motion = previous
                .zip(target)
                .filter(|(from, to)| {
                    self.pending_motion
                        && (to.origin.y - from.origin.y).abs() <= (to.size.height + px(8.)) * 2.
                })
                .map(|(from, _)| Motion { from, started: now });
        }
        self.target = target;
        self.viewport = Some(viewport);
        self.steady = steady;
        self.pending_motion = false;
        if self
            .motion
            .as_ref()
            .is_some_and(|motion| now.saturating_duration_since(motion.started) >= MOVE)
        {
            self.motion = None;
        }
    }

    fn frame(&self, now: Instant) -> (Option<Visual>, bool, Option<Instant>) {
        let Some(bounds) = self.bounds(now) else {
            return (None, false, None);
        };
        if self.steady {
            return (
                Some(Visual {
                    bounds,
                    opacity: 1.,
                }),
                false,
                None,
            );
        }
        let moving = self
            .motion
            .as_ref()
            .is_some_and(|motion| now.saturating_duration_since(motion.started) < MOVE);
        let idle = now.saturating_duration_since(self.idle_since);
        let (opacity, fading, wake) = if idle < HOLD {
            (1., false, Some(self.idle_since + HOLD))
        } else {
            let elapsed = idle - HOLD;
            let cycle = Duration::from_secs(elapsed.as_secs());
            let phase = elapsed - cycle;
            let start = self.idle_since + HOLD + cycle;
            if phase < FADE {
                let t = phase.as_secs_f32() / FADE.as_secs_f32();
                (1. - t * t * (3. - 2. * t), true, None)
            } else if phase < HALF_CYCLE {
                (0., false, Some(start + HALF_CYCLE))
            } else if phase < HALF_CYCLE + FADE {
                let t = (phase - HALF_CYCLE).as_secs_f32() / FADE.as_secs_f32();
                (t * t * (3. - 2. * t), true, None)
            } else {
                (1., false, Some(start + CYCLE))
            }
        };
        (Some(Visual { bounds, opacity }), moving || fading, wake)
    }

    fn schedule(&mut self, wake: Option<Instant>, now: Instant, cx: &mut Context<MarkdownEditor>) {
        if self.wake_at == wake && wake.is_some() {
            return;
        }
        self.timer = None;
        self.wake_at = wake;
        let Some(wake) = wake else {
            return;
        };
        let wait = cx
            .background_executor()
            .timer(wake.saturating_duration_since(now));
        self.timer = Some(cx.spawn(async move |editor, cx| {
            wait.await;
            let _ = editor.update(cx, |editor, cx| {
                if editor.caret_animation.wake_at == Some(wake) {
                    editor.caret_animation.wake_at = None;
                    cx.notify();
                }
            });
        }));
    }
}

fn target(editor: &MarkdownEditor, window: &Window) -> Option<Bounds<Pixels>> {
    let cursor = editor.document.cursor();
    let index = editor.projection.line_for_source(cursor);
    let line = editor.layout.get(index)?;
    let projected = &editor.projection.lines[index];
    let style = editor.document.typing_style();
    let face = Face {
        bold: style.bold,
        italic: style.italic,
        code: projected.code || style.code,
        ..Default::default()
    };
    let text = window.text_system();
    let font = text.resolve_font(&heading_text_run(0, face, projected.heading).font);
    let cap_height = text.cap_height(font, line.font_size);
    let cap_height = if cap_height > px(0.) {
        cap_height
    } else {
        line.font_size
    };
    // Font descent is signed on some platforms.
    let descent = text.descent(font, line.font_size).abs();
    let position = line.position(projected.display_index(cursor));
    Some(Bounds::new(
        position + point(px(-WIDTH), line.baseline_offset - cap_height),
        size(px(WIDTH), cap_height + descent),
    ))
}

pub(super) fn prepare(
    editor: &mut MarkdownEditor,
    window: &mut Window,
    cx: &mut Context<MarkdownEditor>,
) -> Option<Visual> {
    let now = cx.background_executor().now();
    let target = if editor.focus.is_focused(window)
        && window.is_window_active()
        && editor.document.selection().is_empty()
    {
        target(editor, window)
            .filter(|bounds| !bounds.intersect(&editor.content_bounds()).is_empty())
    } else {
        None
    };
    editor.caret_animation.place(
        target,
        (editor.bounds, editor.scroll),
        editor.marked.is_some() || editor.selecting,
        now,
    );
    let (visual, animate, wake) = editor.caret_animation.frame(now);
    editor.caret_animation.schedule(wake, now, cx);
    if animate {
        window.request_animation_frame();
    }
    visual.map(|visual| visual.snap_to_pixels(window.scale_factor()))
}

pub(crate) fn font_stroke(text: &TextSystem, font: FontId, font_size: Pixels) -> Bounds<Pixels> {
    ['S', 'g', 'i']
        .into_iter()
        .filter_map(|ch| text.typographic_bounds(font, font_size, ch).ok())
        .reduce(|a, b| a.union(&b))
        .filter(|bounds| bounds.size.height > px(0.))
        .unwrap_or_else(|| {
            let cap = text.cap_height(font, font_size);
            let cap = if cap > px(0.) { cap } else { font_size };
            let descent = text.descent(font, font_size).abs();
            Bounds::new(point(px(0.), -descent), size(px(WIDTH), cap + descent))
        })
}

pub(crate) fn bounds_from_glyph(
    baseline: Point<Pixels>,
    glyph: Bounds<Pixels>,
    font_size: Pixels,
    script: i8,
) -> Bounds<Pixels> {
    let (scale, shift) = match script {
        0 => (1., px(0.)),
        n if n > 0 => (0.6, font_size * -0.35),
        _ => (0.6, font_size * 0.075),
    };
    // Font coordinates use y up; editor coordinates use y down.
    Bounds::new(
        baseline + point(px(-WIDTH), -glyph.bottom() * scale + shift),
        size(px(WIDTH), glyph.size.height * scale),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{SelectAll, init, text_run};
    use gpui::{
        Entity, EntityInputHandler, Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext,
        VisualTestContext,
    };

    fn rect(x: f32, y: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(1.5), px(18.)))
    }

    #[test]
    fn motion_retargets_from_its_current_position_and_snaps_with_the_viewport() {
        let now = Instant::now();
        let viewport = (
            Bounds::new(point(px(0.), px(0.)), size(px(800.), px(600.))),
            px(0.),
        );
        let mut caret = Caret::new(now);
        caret.place(Some(rect(20., 60.)), viewport, false, now);
        caret.activity(now);
        caret.place(Some(rect(100., 60.)), viewport, false, now);
        assert_eq!(caret.bounds(now).unwrap(), rect(20., 60.));
        let halfway = now + MOVE / 2;
        let middle = caret.bounds(halfway).unwrap();
        assert!(middle.left() > px(20.) && middle.left() < px(100.));
        caret.activity(halfway);
        caret.place(Some(rect(140., 86.)), viewport, false, halfway);
        assert_eq!(caret.bounds(halfway).unwrap(), middle);
        assert_eq!(caret.bounds(halfway + MOVE).unwrap(), rect(140., 86.));
        caret.activity(halfway);
        caret.place(Some(rect(140., 76.)), (viewport.0, px(10.)), false, halfway);
        assert_eq!(caret.bounds(halfway).unwrap(), rect(140., 76.));
        assert!(caret.motion.is_none());
    }

    #[test]
    fn blink_fades_sleeps_between_fades_and_restarts_with_activity() {
        let now = Instant::now();
        let viewport = (
            Bounds::new(point(px(0.), px(0.)), size(px(800.), px(600.))),
            px(0.),
        );
        let mut caret = Caret::new(now);
        caret.place(Some(rect(20., 60.)), viewport, false, now);
        let (visible, animating, wake) = caret.frame(now);
        assert_eq!(visible.unwrap().opacity, 1.);
        assert!(!animating);
        assert_eq!(wake, Some(now + HOLD));
        for offset in [HOLD + FADE / 2, HOLD + HALF_CYCLE + FADE / 2] {
            let (visible, animating, wake) = caret.frame(now + offset);
            assert!((visible.unwrap().opacity - 0.5).abs() < 0.001);
            assert!(animating);
            assert!(wake.is_none());
        }
        let (visible, animating, wake) = caret.frame(now + HOLD + FADE);
        assert_eq!(visible.unwrap().opacity, 0.);
        assert!(!animating);
        assert_eq!(wake, Some(now + HOLD + HALF_CYCLE));
        let input = now + HOLD + FADE;
        caret.activity(input);
        assert_eq!(caret.frame(input).0.unwrap().opacity, 1.);
        assert_eq!(caret.frame(input).2, Some(input + HOLD));
    }

    fn setup<'a>(
        cx: &'a mut TestAppContext,
        text: &str,
    ) -> (Entity<MarkdownEditor>, &'a mut VisualTestContext) {
        cx.update(init);
        let (editor, cx) = cx.add_window_view(|window, cx| {
            let editor = MarkdownEditor::new(text, cx);
            window.focus(&editor.focus);
            editor
        });
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        cx.update(|window, _| assert!(window.is_window_active()));
        (editor, cx)
    }

    fn advance(cx: &mut VisualTestContext, duration: Duration) {
        cx.executor().advance_clock(duration);
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    }

    fn visual(editor: &Entity<MarkdownEditor>, cx: &mut VisualTestContext) -> Visual {
        editor.read_with(cx, |editor, cx| {
            editor
                .caret_animation
                .frame(cx.background_executor().now())
                .0
                .unwrap()
        })
    }

    #[gpui::test]
    fn caret_matches_the_placeholder_baseline_and_keeps_its_height_when_typing(
        cx: &mut TestAppContext,
    ) {
        let (editor, cx) = setup(cx, "");
        let empty = visual(&editor, cx).bounds;
        editor.update_in(cx, |editor, window, _| {
            let run = text_run("Start writing…".len(), Face::default());
            let text = window.text_system();
            let font = text.resolve_font(&run.font);
            let font_size = px(super::super::appearance::BODY_SIZE);
            let placeholder = text.shape_line("Start writing…".into(), font_size, &[run], None);
            let row = &editor.layout[0];
            assert_eq!(empty.size.width, px(1.));
            assert_eq!(empty.right(), row.origin.x);
            assert!(empty.left() < row.origin.x);
            let baseline = row.origin.y
                + (row.line_height - placeholder.ascent - placeholder.descent) / 2.
                + placeholder.ascent;
            assert!((empty.top() + text.cap_height(font, font_size) - baseline).abs() < px(0.001));
            assert!(
                (empty.bottom() - baseline - text.descent(font, font_size).abs()).abs() < px(0.001)
            );
        });
        cx.simulate_input("Start");
        advance(cx, MOVE);
        let typed = visual(&editor, cx).bounds;
        assert_eq!(typed.size.height, empty.size.height);
        assert_eq!(typed.top(), empty.top());
        editor.read_with(cx, |editor, _| {
            assert_eq!(typed.right(), editor.caret().unwrap().0.x);
        });
    }

    #[test]
    fn painted_caret_has_whole_device_pixel_edges_at_rest_and_during_motion() {
        let now = Instant::now();
        let viewport = (Bounds::default(), px(0.));
        let mut caret = Caret::new(now);
        let start = Bounds::new(point(px(27.5), px(19.395)), size(px(WIDTH), px(13.89)));
        let end = Bounds::new(point(px(38.2), px(45.645)), size(px(WIDTH), px(13.89)));
        caret.place(Some(start), viewport, false, now);
        caret.activity(now);
        caret.place(Some(end), viewport, false, now);
        for elapsed in [Duration::ZERO, MOVE / 3, MOVE] {
            let raw = caret.frame(now + elapsed).0.unwrap();
            for scale in [1., 1.5, 2.] {
                let painted = raw.snap_to_pixels(scale).bounds;
                for edge in [
                    painted.left(),
                    painted.right(),
                    painted.top(),
                    painted.bottom(),
                ] {
                    assert!(((edge * scale) - (edge * scale).round()).abs() < px(0.001));
                }
                assert!((painted.top() - raw.bounds.top()).abs() <= px(0.5 / scale));
                assert!((painted.bottom() - raw.bounds.bottom()).abs() <= px(0.5 / scale));
                assert!((painted.right() - raw.bounds.right()).abs() <= px(0.5 / scale));
                assert!(painted.size.width * scale >= px(1.));
            }
        }
    }

    #[gpui::test]
    fn caret_height_tracks_font_size_across_headings_blank_lines_and_code(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "Body");
        let body_height = visual(&editor, cx).bounds.size.height;
        for (source, scale) in [
            ("", 1.),
            ("\nBody", 1.),
            ("#### Heading", 18. / 15.),
            ("> Quote", 1.),
            ("# Heading", 32. / 15.),
            ("## Heading", 26. / 15.),
            ("### Heading", 21. / 15.),
            ("##### Heading", 16. / 15.),
            ("###### Heading", 1.),
            ("```\n\n```", 12.75 / 15.),
            ("```\ncode\n```", 12.75 / 15.),
        ] {
            editor.update_in(cx, |editor, _, cx| editor.set_markdown(source, cx));
            cx.run_until_parked();
            // GPUI's test font uses identical metrics for every face.
            let height = visual(&editor, cx).bounds.size.height;
            assert!(
                (height - body_height * scale).abs() < px(0.001),
                "{source:?}"
            );
        }
    }

    #[gpui::test]
    fn native_input_moves_the_logical_caret_immediately_while_the_visual_glides(
        cx: &mut TestAppContext,
    ) {
        let (editor, cx) = setup(cx, "é🦀 tail");
        let start = visual(&editor, cx).bounds;
        cx.simulate_input("x");
        editor.update_in(cx, |editor, window, cx| {
            assert_eq!(editor.markdown(), "xé🦀 tail");
            assert_eq!(editor.document.cursor(), 1);
            assert!(editor.caret().unwrap().0.x > start.left());
            assert_eq!(
                editor
                    .caret_animation
                    .bounds(cx.background_executor().now())
                    .unwrap(),
                start
            );
            let range = editor.selected_text_range(false, window, cx).unwrap().range;
            let ime_bounds = editor
                .bounds_for_range(range, Bounds::default(), window, cx)
                .unwrap();
            assert_eq!(ime_bounds.origin, editor.caret().unwrap().0);
        });
        advance(cx, MOVE / 2);
        let mid = visual(&editor, cx).bounds;
        assert!(mid.left() > start.left());
        cx.simulate_keystrokes("right");
        assert_eq!(visual(&editor, cx).bounds, mid);
        advance(cx, MOVE);
        editor.read_with(cx, |editor, cx| {
            assert_eq!(editor.document.cursor(), "xé".len());
            assert_eq!(
                editor
                    .caret_animation
                    .bounds(cx.background_executor().now())
                    .unwrap()
                    .origin,
                editor.caret_animation.target.unwrap().origin
            );
        });
    }

    #[gpui::test]
    fn blink_resets_on_input_and_stops_for_selection_blur_and_inactive_windows(
        cx: &mut TestAppContext,
    ) {
        let (editor, cx) = setup(cx, "Text");
        advance(cx, HOLD + FADE);
        assert_eq!(visual(&editor, cx).opacity, 0.);
        cx.simulate_input("a");
        assert_eq!(visual(&editor, cx).opacity, 1.);
        advance(cx, HOLD - Duration::from_millis(1));
        assert_eq!(visual(&editor, cx).opacity, 1.);
        cx.dispatch_action(SelectAll);
        editor.read_with(cx, |editor, _| {
            assert!(editor.caret_animation.target.is_none());
            assert!(editor.caret_animation.timer.is_none());
        });
        cx.simulate_keystrokes("right");
        assert_eq!(visual(&editor, cx).opacity, 1.);
        let other = cx.update(|_, cx| cx.focus_handle());
        cx.update(|window, _| window.focus(&other));
        cx.run_until_parked();
        editor.read_with(cx, |editor, _| {
            assert!(editor.caret_animation.target.is_none());
            assert!(editor.caret_animation.timer.is_none());
        });
        editor.update_in(cx, |editor, window, _| window.focus(&editor.focus));
        cx.run_until_parked();
        assert_eq!(visual(&editor, cx).opacity, 1.);
        cx.deactivate_window();
        editor.read_with(cx, |editor, _| {
            assert!(editor.caret_animation.target.is_none());
            assert!(editor.caret_animation.timer.is_none());
        });
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        assert_eq!(visual(&editor, cx).opacity, 1.);
    }

    #[gpui::test]
    fn vertical_motion_scroll_resize_and_composition_keep_the_caret_aligned(
        cx: &mut TestAppContext,
    ) {
        let (editor, cx) = setup(cx, &format!("First\nSecond\n{}", "Body\n".repeat(60)));
        let start = visual(&editor, cx).bounds;
        cx.simulate_keystrokes("down");
        assert_eq!(visual(&editor, cx).bounds, start);
        advance(cx, MOVE / 2);
        assert!(visual(&editor, cx).bounds.top() > start.top());
        let position = point(px(500.), px(200.));
        cx.simulate_mouse_move(position, None, Modifiers::default());
        cx.simulate_event(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.), px(-10.))),
            ..Default::default()
        });
        cx.simulate_resize(size(px(560.), px(400.)));
        editor.read_with(cx, |editor, cx| {
            assert!(editor.caret_animation.motion.is_none());
            assert_eq!(
                editor
                    .caret_animation
                    .bounds(cx.background_executor().now())
                    .unwrap()
                    .origin,
                editor.caret_animation.target.unwrap().origin
            );
        });
        editor.update_in(cx, |editor, window, cx| {
            editor.replace_and_mark_text_in_range(None, "あ", Some(1..1), window, cx)
        });
        cx.run_until_parked();
        advance(cx, CYCLE * 2);
        editor.read_with(cx, |editor, cx| {
            let (visible, animating, wake) =
                editor.caret_animation.frame(cx.background_executor().now());
            assert_eq!(visible.unwrap().opacity, 1.);
            assert!(!animating);
            assert!(wake.is_none());
            assert!(editor.caret_animation.timer.is_none());
            assert_eq!(
                visible.unwrap().bounds.origin,
                editor.caret_animation.target.unwrap().origin
            );
        });
        editor.update_in(cx, |editor, window, cx| {
            editor.replace_text_in_range(None, "あ", window, cx)
        });
        cx.run_until_parked();
        advance(cx, HOLD + FADE);
        assert_eq!(visual(&editor, cx).opacity, 0.);
        cx.simulate_event(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.), px(-1000.))),
            ..Default::default()
        });
        editor.read_with(cx, |editor, _| {
            assert!(editor.caret_animation.target.is_none());
            assert!(editor.caret_animation.timer.is_none());
        });
    }
}
