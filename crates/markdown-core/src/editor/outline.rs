mod backdrop;

use super::{MarkdownEditor, appearance};
#[cfg(test)]
use gpui::Pixels;
use gpui::{
    AnyElement, AvailableSpace, Bounds, ContentMask, Context, Corners, CursorStyle, FontWeight,
    MouseButton, PathBuilder, ScrollHandle, Window, canvas, div, fill, point, prelude::*, px, rgb,
    rgba, size,
};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

const CLOSED_WIDTH: f32 = 32.;
const OPEN_WIDTH: f32 = 192.;
const MAX_HEIGHT: f32 = 420.;
const DURATION: Duration = Duration::from_millis(180);

#[derive(Default)]
pub(super) struct Outline {
    pub expanded: bool,
    pub has_headings: bool,
    progress: f32,
    transition: Option<(Instant, f32)>,
    scroll: ScrollHandle,
    scroll_target: Option<(usize, usize)>,
    reveal_pending: bool,
    active: Option<usize>,
    pub jumped_to: Option<usize>,
    collapsed: HashSet<usize>,
    backdrop: backdrop::Backdrop,
    #[cfg(test)]
    bounds: Option<Bounds<Pixels>>,
}

impl Outline {
    pub fn collapse_immediately(&mut self) {
        self.expanded = false;
        self.progress = 0.;
        self.transition = None;
    }

    pub fn clear_backdrop(&mut self, window: &mut Window) {
        self.backdrop.clear(window);
    }

    pub fn reset(&mut self) {
        self.backdrop.invalidate();
        let backdrop = std::mem::take(&mut self.backdrop);
        *self = Self {
            backdrop,
            ..Self::default()
        };
    }

    pub fn document_changed(&mut self) {
        self.collapsed.clear();
    }

    pub fn is_open(&self) -> bool {
        self.expanded || self.progress > 0.
    }

    pub fn expand(&mut self, expanded: bool) {
        if expanded != self.expanded {
            self.advance(Instant::now());
            self.expanded = expanded;
            self.transition = Some((Instant::now(), self.progress));
        }
    }

    fn advance(&mut self, now: Instant) -> f32 {
        if let Some((start, from)) = self.transition {
            let t = (now.duration_since(start).as_secs_f32() / DURATION.as_secs_f32()).min(1.);
            let target = if self.expanded { 1. } else { 0. };
            self.progress = from + (target - from) * (1. - (1. - t).powi(3));
            if t == 1. {
                self.transition = None;
            }
        }
        self.progress
    }
}

struct Heading {
    source: usize,
    line: usize,
    level: usize,
    title: String,
    parent: Option<usize>,
    depth: usize,
    has_children: bool,
}

fn headings(editor: &MarkdownEditor) -> Vec<Heading> {
    let mut entries: Vec<Heading> = Vec::new();
    let mut ancestors: Vec<usize> = Vec::new();
    for (line, projected) in editor
        .projection
        .lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.heading > 0 && !line.code)
    {
        while ancestors
            .last()
            .is_some_and(|parent| entries[*parent].level >= projected.heading)
        {
            ancestors.pop();
        }
        let parent = ancestors.last().copied();
        if let Some(parent) = parent {
            entries[parent].has_children = true;
        }
        entries.push(Heading {
            source: projected.source.start,
            line,
            level: projected.heading,
            title: if projected.text.trim().is_empty() {
                "Untitled section".into()
            } else {
                projected.text.trim().into()
            },
            parent,
            depth: ancestors.len(),
            has_children: false,
        });
        ancestors.push(entries.len() - 1);
    }
    entries
}

fn visibility(entries: &[Heading], collapsed: &HashSet<usize>) -> Vec<bool> {
    let mut visible = Vec::with_capacity(entries.len());
    for entry in entries {
        visible.push(
            entry.parent.is_none_or(|parent| {
                visible[parent] && !collapsed.contains(&entries[parent].source)
            }),
        );
    }
    visible
}

fn jump(
    editor: &mut MarkdownEditor,
    source: usize,
    window: &mut Window,
    cx: &mut Context<MarkdownEditor>,
) {
    let Some(index) = editor
        .projection
        .lines
        .iter()
        .position(|line| line.source.start == source && line.heading > 0 && !line.code)
    else {
        return;
    };
    let Some(layout) = editor.layout.get(index) else {
        return;
    };
    let scroll =
        (layout.origin.y + editor.scroll - editor.bounds.top() - px(editor.top_gutter() + 8.))
            .clamp(px(0.), editor.max_scroll);
    let cursor = editor.projection.lines[index].source_index(0);
    editor.move_to(cursor, false, cx);
    editor.selecting = false;
    editor.preferred_x = None;
    editor.toolbelt_dismissed = true;
    editor.reveal_cursor = false;
    editor.scroll = scroll;
    editor.outline.jumped_to = Some(source);
    window.focus(&editor.focus);
}

fn icon(branch: bool, collapsed: bool, selected: bool) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let center = bounds.center();
            let color = rgb(if selected {
                appearance::TEXT
            } else {
                appearance::MUTED
            });
            if branch {
                let mut path = PathBuilder::stroke(px(1.25));
                if collapsed {
                    path.move_to(center + point(px(-2.), px(-3.)));
                    path.line_to(center + point(px(2.), px(0.)));
                    path.line_to(center + point(px(-2.), px(3.)));
                } else {
                    path.move_to(center + point(px(-3.), px(-2.)));
                    path.line_to(center + point(px(0.), px(2.)));
                    path.line_to(center + point(px(3.), px(-2.)));
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            } else {
                window.paint_quad(
                    fill(
                        Bounds::new(center - point(px(3.), px(0.75)), size(px(6.), px(1.5))),
                        color,
                    )
                    .corner_radii(px(0.75)),
                );
            }
        },
    )
    .size_full()
}

pub(super) fn show(
    editor: &mut MarkdownEditor,
    window: &mut Window,
    cx: &mut Context<MarkdownEditor>,
) {
    #[cfg(test)]
    {
        editor.outline.bounds = None;
    }
    editor.outline.backdrop.collect(window);
    let entries = headings(editor);
    if editor.marked.is_some() || !editor.focus.is_focused(window) {
        editor.outline.collapse_immediately();
        editor.outline.clear_backdrop(window);
        return;
    }
    if entries.is_empty() {
        editor.outline.clear_backdrop(window);
        editor.outline.reset();
        return;
    }
    let viewport = editor.content_bounds();
    let available = viewport.size.height - px(32.);
    if available < px(40.) || viewport.size.width < px(180.) {
        editor.outline.clear_backdrop(window);
        return;
    }
    let active = editor
        .outline
        .jumped_to
        .filter(|source| {
            entries.iter().any(|entry| {
                entry.source == *source
                    && editor.layout[entry.line].origin.y >= viewport.top()
                    && editor.layout[entry.line].origin.y < viewport.bottom()
            })
        })
        .or_else(|| {
            entries
                .iter()
                .rev()
                .find(|entry| {
                    editor.layout[entry.line].origin.y
                        <= editor.bounds.top() + px(editor.top_gutter() + 24.)
                })
                .map(|entry| entry.source)
        });
    let previous = editor.outline.active;
    editor.outline.active = active;
    let progress = editor.outline.advance(Instant::now());
    if editor.outline.transition.is_some() {
        window.request_animation_frame();
    }
    let visible = if editor.outline.is_open() {
        visibility(&entries, &editor.outline.collapsed)
    } else {
        vec![true; entries.len()]
    };
    let mut selected = entries
        .iter()
        .position(|entry| Some(entry.source) == active);
    while selected.is_some_and(|index| !visible[index]) {
        selected = entries[selected.unwrap()].parent;
    }
    let target = selected.map(|index| {
        (
            visible[..index].iter().filter(|visible| **visible).count(),
            visible.iter().filter(|visible| **visible).count(),
        )
    });
    let target_changed = editor.outline.scroll_target != target;
    if (previous != active
        || target_changed
        || editor.outline.reveal_pending
        || editor.outline.transition.is_some())
        && let Some((index, _)) = target
    {
        editor.outline.scroll.scroll_to_item(index);
    }
    // ScrollHandle has the previous frame's row bounds until layout completes.
    editor.outline.scroll_target = target;
    editor.outline.reveal_pending = target_changed;
    if target_changed {
        window.request_animation_frame();
    }
    let open_width = px(OPEN_WIDTH).min(viewport.size.width - px(24.));
    let open_height = px(44. + 28. * entries.len() as f32)
        .min(available)
        .min(px(MAX_HEIGHT));
    let closed_height = px(12. + 10. * entries.len() as f32)
        .min(available)
        .min(px(240.));
    let width = px(CLOSED_WIDTH) + (open_width - px(CLOSED_WIDTH)) * progress;
    let height = closed_height + (open_height - closed_height) * progress;
    let origin = point(viewport.left() + px(12.), viewport.center().y - height / 2.);
    let open_bounds = Bounds::new(
        point(origin.x, viewport.center().y - open_height / 2.),
        size(open_width, open_height),
    );
    let background = if editor.outline.is_open() {
        backdrop::prepare(editor, open_bounds, window, cx)
    } else {
        editor.outline.clear_backdrop(window);
        None
    };
    let row_height = 10. + 18. * progress;
    let rows = div()
        .id("outline-scroll")
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&editor.outline.scroll)
        .children(
            entries
                .into_iter()
                .enumerate()
                .filter(|(index, _)| visible[*index])
                .map(|(index, heading)| {
                    let selected = selected == Some(index);
                    let depth = heading.depth as f32;
                    let source = heading.source;
                    let collapsed = editor.outline.collapsed.contains(&source);
                    div()
                        .id(("outline-heading", source))
                        .debug_selector(move || format!("outline-heading-{index}"))
                        .relative()
                        .w_full()
                        .h(px(row_height))
                        .flex_shrink_0()
                        .rounded_md()
                        .cursor_pointer()
                        .when(selected, |view| {
                            view.bg(rgba(
                                (appearance::BACKGROUND << 8) | (progress * 255.) as u32,
                            ))
                        })
                        .when(progress > 0.5, |view| {
                            view.hover(|view| view.bg(rgb(appearance::CHROME_HOVER)))
                        })
                        .on_click(cx.listener(move |editor, _, window, cx| {
                            jump(editor, source, window, cx)
                        }))
                        .child(
                            div()
                                .absolute()
                                .left(px(2. + depth * 1.5))
                                .top(px((row_height - 3.) / 2.))
                                .w(px(16. - depth * 1.5))
                                .h(px(3.))
                                .rounded_full()
                                .opacity(1. - progress)
                                .bg(rgb(if selected { appearance::TEXT } else { 0xbdbdbd })),
                        )
                        .when(depth > 0., |view| {
                            view.child(
                                canvas(
                                    |_, _, _| (),
                                    move |bounds, _, window, _| {
                                        let mut path = PathBuilder::stroke(px(1.));
                                        let x = bounds.left() + px(5. + depth * 8.);
                                        path.move_to(point(x, bounds.top()));
                                        path.line_to(point(x, bounds.center().y));
                                        path.line_to(point(x + px(4.), bounds.center().y));
                                        if let Ok(path) = path.build() {
                                            window.paint_path(path, rgb(appearance::CHROME_BORDER));
                                        }
                                    },
                                )
                                .absolute()
                                .inset_0()
                                .opacity(progress),
                            )
                        })
                        .child(
                            div()
                                .id(("outline-disclosure", source))
                                .debug_selector(move || format!("outline-disclosure-{index}"))
                                .absolute()
                                .left(px(6. + depth * 8.))
                                .top(px((row_height - 14.) / 2.))
                                .size(px(14.))
                                .opacity(progress)
                                .when(heading.has_children && progress > 0.5, |view| {
                                    view.cursor_pointer()
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                        .on_click(cx.listener(move |editor, _, _, cx| {
                                            cx.stop_propagation();
                                            if !editor.outline.collapsed.remove(&source) {
                                                editor.outline.collapsed.insert(source);
                                            }
                                            cx.notify();
                                        }))
                                })
                                .child(icon(heading.has_children, collapsed, selected)),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(24. + depth * 8.))
                                .right(px(4.))
                                .top(px((row_height - 20.) / 2.))
                                .h(px(20.))
                                .truncate()
                                .opacity(progress)
                                .text_color(rgb(if selected {
                                    appearance::TEXT
                                } else {
                                    appearance::MUTED
                                }))
                                .font_weight(if selected {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::NORMAL
                                })
                                .child(heading.title),
                        )
                }),
        );
    let mut outline: AnyElement = div()
        .id("document-outline")
        .debug_selector(|| "document-outline".into())
        .relative()
        .w(width)
        .h(height)
        .flex()
        .flex_col()
        .p(px(6. + 2. * progress))
        .overflow_hidden()
        .rounded(px(appearance::CHROME_RADIUS))
        .bg(rgba((appearance::CHROME << 8) | (progress * 230.) as u32))
        .font_family(appearance::FONT_FAMILY)
        .text_size(px(appearance::UI_SIZE))
        .line_height(px(20.))
        .cursor(CursorStyle::Arrow)
        .occlude()
        .on_hover(cx.listener(|editor, hovered, _, cx| {
            if !editor.selecting || !hovered {
                editor.outline.expand(*hovered);
                cx.notify();
            }
        }))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    if let Some(background) = background {
                        window.with_content_mask(Some(ContentMask { bounds }), |window| {
                            let _ = window.paint_image(
                                open_bounds,
                                Corners::all(px(appearance::CHROME_RADIUS)),
                                background,
                                0,
                                false,
                            );
                        });
                    }
                    window.paint_quad(
                        gpui::outline(bounds, rgb(appearance::CHROME_BORDER), Default::default())
                            .corner_radii(px(appearance::CHROME_RADIUS)),
                    );
                },
            )
            .absolute()
            .inset_0()
            .opacity(progress),
        )
        .child(
            div()
                .h(px(28. * progress))
                .flex_shrink_0()
                .flex()
                .items_center()
                .overflow_hidden()
                .px(px(6.))
                .opacity(progress)
                .text_size(px(12.))
                .text_color(rgb(appearance::MUTED))
                .child("Contents"),
        )
        .child(rows)
        .into_any_element();
    let measured = outline.layout_as_root(AvailableSpace::min_size(), window, cx);
    #[cfg(test)]
    {
        editor.outline.bounds = Some(Bounds::new(origin, measured));
    }
    #[cfg(not(test))]
    let _ = measured;
    window.defer_draw(outline, origin, 0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{Format, SelectAll, Undo, init};
    use gpui::{
        Entity, Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext, VisualTestContext, size,
    };

    fn setup<'a>(
        cx: &'a mut TestAppContext,
        text: &str,
    ) -> (Entity<MarkdownEditor>, &'a mut VisualTestContext) {
        cx.update(init);
        cx.add_window_view(|window, cx| {
            let editor = MarkdownEditor::new(text, cx);
            window.focus(&editor.focus);
            editor
        })
    }

    fn settle(editor: &Entity<MarkdownEditor>, cx: &mut VisualTestContext) {
        editor.update_in(cx, |editor, _, cx| {
            if let Some((start, _)) = &mut editor.outline.transition {
                *start -= DURATION;
            }
            cx.notify();
        });
        cx.run_until_parked();
    }

    fn open(editor: &Entity<MarkdownEditor>, cx: &mut VisualTestContext) {
        let rail = cx.debug_bounds("document-outline").unwrap();
        cx.simulate_mouse_move(
            rail.origin + point(px(16.), px(10.)),
            None,
            Modifiers::default(),
        );
        settle(editor, cx);
        editor.read_with(cx, |editor, _| assert!(editor.outline.expanded));
    }

    #[gpui::test]
    fn outline_uses_visible_heading_titles_and_updates_with_edits(cx: &mut TestAppContext) {
        let source = "# **Intro**\n\n## Sub\n### Same\n#### Same\n##### Five\n###### Six\n\n```\n# Not a heading\n```\n\n### ";
        let (editor, cx) = setup(cx, source);
        editor.read_with(cx, |editor, _| {
            let entries = headings(editor);
            assert_eq!(
                entries.iter().map(|entry| entry.level).collect::<Vec<_>>(),
                [1, 2, 3, 4, 5, 6, 3]
            );
            assert_eq!(
                entries
                    .iter()
                    .map(|entry| entry.title.as_str())
                    .collect::<Vec<_>>(),
                [
                    "Intro",
                    "Sub",
                    "Same",
                    "Same",
                    "Five",
                    "Six",
                    "Untitled section"
                ]
            );
            assert_ne!(entries[2].source, entries[3].source);
            assert_eq!(editor.markdown(), source);
        });
        editor.update_in(cx, |editor, _, cx| editor.set_markdown("Plain", cx));
        cx.run_until_parked();
        assert!(editor.read_with(cx, |editor, _| editor.outline.bounds.is_none()));
        cx.dispatch_action(SelectAll);
        cx.simulate_input("# Renamed");
        editor.read_with(cx, |editor, _| {
            assert_eq!(headings(editor)[0].title, "Renamed");
            assert!(editor.outline.bounds.is_some());
        });
        editor.update_in(cx, |editor, _, cx| editor.format(Format::Heading4, cx));
        cx.run_until_parked();
        assert_eq!(
            editor.read_with(cx, |editor, _| headings(editor)[0].level),
            4
        );
        cx.dispatch_action(Undo);
        assert_eq!(
            editor.read_with(cx, |editor, _| headings(editor)[0].level),
            1
        );
    }

    #[gpui::test]
    fn hover_expands_without_reflow_or_changing_selection_and_collapses_on_exit(
        cx: &mut TestAppContext,
    ) {
        let (editor, cx) = setup(cx, "# Intro\n\nText\n\n## Next");
        cx.simulate_resize(size(px(560.), px(400.)));
        editor.update_in(cx, |editor, _, cx| {
            editor.document.select(2, 7);
            cx.notify();
        });
        cx.run_until_parked();
        let (origin, width, selection) = editor.read_with(cx, |editor, _| {
            assert!(editor.outline.bounds.unwrap().right() < editor.layout[0].origin.x);
            assert!(
                editor.toolbelt_bounds.unwrap().left() > editor.outline.bounds.unwrap().right()
            );
            (
                editor.layout[0].origin,
                editor.layout[0].width,
                editor.document.selection(),
            )
        });
        open(&editor, cx);
        editor.read_with(cx, |editor, _| {
            assert_eq!(editor.outline.bounds.unwrap().size.width, px(OPEN_WIDTH));
            assert_eq!(editor.layout[0].origin, origin);
            assert_eq!(editor.layout[0].width, width);
            assert_eq!(editor.document.selection(), selection);
            assert!(editor.toolbelt_bounds.is_none());
        });
        cx.simulate_mouse_move(point(px(500.), px(300.)), None, Modifiers::default());
        settle(&editor, cx);
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.outline.bounds.unwrap().size.width),
            px(CLOSED_WIDTH)
        );
        open(&editor, cx);
        cx.simulate_keystrokes("escape");
        settle(&editor, cx);
        editor.read_with(cx, |editor, _| {
            assert!(!editor.outline.expanded);
            assert_eq!(editor.document.selection(), selection);
        });
    }

    #[gpui::test]
    fn clicking_duplicate_titles_jumps_to_the_correct_section_without_editing(
        cx: &mut TestAppContext,
    ) {
        let source = format!(
            "# Same\n{}\n### Same\n{}\n#### Same\nTail",
            "First body\n".repeat(35),
            "Second body\n".repeat(35)
        );
        let (editor, cx) = setup(cx, &source);
        cx.simulate_resize(size(px(640.), px(400.)));
        open(&editor, cx);
        let middle = cx.debug_bounds("outline-heading-1").unwrap().center();
        cx.simulate_click(middle, Modifiers::default());
        editor.update_in(cx, |editor, window, _| {
            let entries = headings(editor);
            assert_eq!(editor.document.cursor(), entries[1].source + 4);
            assert_eq!(editor.layout[entries[1].line].origin.y, px(64.));
            assert_eq!(editor.outline.active, Some(entries[1].source));
            assert_eq!(editor.markdown(), source);
            assert!(editor.focus.is_focused(window));
            assert!(!editor.document.undo());
        });
        let last = cx.debug_bounds("outline-heading-2").unwrap().center();
        cx.simulate_click(last, Modifiers::default());
        editor.read_with(cx, |editor, _| {
            let entries = headings(editor);
            assert_eq!(editor.outline.active, Some(entries[2].source));
            assert_eq!(editor.document.cursor(), entries[2].source + 5);
            assert!(editor.layout[entries[2].line].origin.y < editor.bounds.bottom());
            assert_eq!(editor.markdown(), source);
        });
        cx.simulate_input("New ");
        assert_eq!(
            editor.read_with(cx, |editor, _| headings(editor)[2].title.clone()),
            "New Same"
        );
    }

    #[gpui::test]
    fn active_section_follows_document_scrolling(cx: &mut TestAppContext) {
        let source = format!(
            "# First\n{}\n## Second\n{}",
            "Body\n".repeat(30),
            "Tail\n".repeat(30)
        );
        let (editor, cx) = setup(cx, &source);
        cx.simulate_resize(size(px(640.), px(400.)));
        let (target, source_offset) = editor.read_with(cx, |editor, _| {
            let entries = headings(editor);
            assert_eq!(editor.outline.active, Some(0));
            (
                editor.layout[entries[1].line].origin.y - px(64.),
                entries[1].source,
            )
        });
        cx.simulate_mouse_move(point(px(500.), px(200.)), None, Modifiers::default());
        cx.simulate_event(ScrollWheelEvent {
            position: point(px(500.), px(200.)),
            delta: ScrollDelta::Pixels(point(px(0.), -target)),
            ..Default::default()
        });
        editor.read_with(cx, |editor, _| {
            assert_eq!(editor.outline.active, Some(source_offset));
            assert_eq!(editor.markdown(), source);
            assert_eq!(editor.document.cursor(), 2);
        });
    }

    #[gpui::test]
    fn long_outline_scrolls_independently_and_keeps_the_last_heading_reachable(
        cx: &mut TestAppContext,
    ) {
        let source = (0..80)
            .map(|index| format!("# Section {index}\n\nBody\n\n"))
            .collect::<String>();
        let (editor, cx) = setup(cx, &source);
        cx.simulate_resize(size(px(560.), px(340.)));
        open(&editor, cx);
        let panel = cx.debug_bounds("document-outline").unwrap();
        assert!(panel.bottom() <= px(324.) && panel.right() <= px(548.));
        let position = panel.origin + point(px(100.), px(100.));
        cx.simulate_mouse_move(position, None, Modifiers::default());
        cx.simulate_event(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.), px(-10000.))),
            ..Default::default()
        });
        editor.read_with(cx, |editor, _| {
            assert_eq!(editor.scroll, px(0.));
            assert!(editor.outline.scroll.offset().y < px(0.));
        });
        let last = cx.debug_bounds("outline-heading-79").unwrap().center();
        assert!(panel.contains(&last));
        cx.simulate_click(last, Modifiers::default());
        editor.read_with(cx, |editor, _| {
            assert_eq!(
                editor.outline.active,
                headings(editor).last().map(|heading| heading.source)
            );
            assert!(editor.scroll > px(0.));
            assert_eq!(editor.markdown(), source);
        });
    }

    #[gpui::test]
    fn composition_hides_the_outline_without_changing_the_writing_margin(cx: &mut TestAppContext) {
        use gpui::EntityInputHandler;
        let (editor, cx) = setup(cx, "# Title\n\nBody ");
        cx.simulate_resize(size(px(560.), px(400.)));
        editor.update_in(cx, |editor, _, cx| {
            editor.move_to(editor.markdown().len(), false, cx)
        });
        cx.run_until_parked();
        let left = editor.read_with(cx, |editor, _| editor.layout[2].origin.x);
        editor.update_in(cx, |editor, window, cx| {
            editor.replace_and_mark_text_in_range(None, "日", Some(1..1), window, cx)
        });
        cx.run_until_parked();
        editor.read_with(cx, |editor, _| {
            assert!(editor.outline.bounds.is_none());
            assert_eq!(editor.layout[2].origin.x, left);
        });
        editor.update_in(cx, |editor, window, cx| {
            editor.replace_text_in_range(None, "日本", window, cx)
        });
        cx.run_until_parked();
        editor.read_with(cx, |editor, _| {
            assert!(editor.outline.bounds.is_some());
            assert_eq!(editor.layout[2].origin.x, left);
            assert_eq!(editor.markdown(), "# Title\n\nBody 日本");
        });
    }

    #[gpui::test]
    fn centered_outline_keeps_its_anchor_through_hover_and_resize(cx: &mut TestAppContext) {
        let (editor, cx) = setup(cx, "# Intro\n\nBody\n\n## Details\n\nText\n\n### Notes");
        cx.simulate_resize(size(px(560.), px(400.)));
        cx.run_until_parked();
        editor.read_with(cx, |e, _| {
            let panel = e.outline.bounds.unwrap();
            assert_eq!(panel.center().y, e.content_bounds().center().y);
            assert_eq!(panel.size.width, px(CLOSED_WIDTH));
        });
        open(&editor, cx);
        editor.read_with(cx, |e, _| {
            let panel = e.outline.bounds.unwrap();
            assert_eq!(panel.center().y, e.content_bounds().center().y);
            assert_eq!(panel.size.width, px(OPEN_WIDTH));
        });
        cx.simulate_resize(size(px(700.), px(620.)));
        cx.run_until_parked();
        editor.read_with(cx, |e, _| {
            assert_eq!(
                e.outline.bounds.unwrap().center().y,
                e.content_bounds().center().y
            )
        });
    }

    #[gpui::test]
    fn disclosure_follows_heading_ancestry_and_never_moves_the_caret(cx: &mut TestAppContext) {
        let source = "# Root\n\n#### Skipped levels\n\n## Sibling\n\n##### Leaf\n\n# Next root";
        let (editor, cx) = setup(cx, source);
        open(&editor, cx);
        editor.read_with(cx, |e, _| {
            let entries = headings(e);
            assert_eq!(
                entries.iter().map(|entry| entry.depth).collect::<Vec<_>>(),
                [0, 1, 1, 2, 0]
            );
            assert_eq!(
                entries
                    .iter()
                    .map(|entry| entry.has_children)
                    .collect::<Vec<_>>(),
                [true, false, true, false, false]
            );
        });
        let before = editor.read_with(cx, |e, _| {
            (
                e.document.selection(),
                e.document.cursor(),
                e.scroll,
                e.outline.bounds,
            )
        });
        let disclosure = cx.debug_bounds("outline-disclosure-0").unwrap().center();
        cx.simulate_click(disclosure, Modifiers::default());
        cx.run_until_parked();
        editor.read_with(cx, |e, _| {
            assert_eq!(
                visibility(&headings(e), &e.outline.collapsed),
                [true, false, false, false, true]
            );
            assert_eq!(
                (
                    e.document.selection(),
                    e.document.cursor(),
                    e.scroll,
                    e.outline.bounds
                ),
                before
            );
            assert_eq!(e.markdown(), source);
        });
        cx.simulate_click(disclosure, Modifiers::default());
        cx.run_until_parked();
        editor.update_in(cx, |e, _, _| {
            assert!(
                visibility(&headings(e), &e.outline.collapsed)
                    .into_iter()
                    .all(|v| v)
            );
            assert!(!e.document.undo());
        });
        let leaf = cx.debug_bounds("outline-heading-3").unwrap().center();
        cx.simulate_click(leaf, Modifiers::default());
        editor.read_with(cx, |e, _| {
            assert_eq!(e.document.cursor(), source.find("Leaf").unwrap())
        });
    }

    #[cfg(target_os = "macos")]
    #[gpui::test]
    fn backdrop_is_cached_for_caret_frames_refreshes_on_scroll_and_releases_on_close(
        cx: &mut TestAppContext,
    ) {
        let source = format!(
            "# Heading\n\n{}\n## Next",
            "Text behind the outline.\n".repeat(60)
        );
        let (editor, cx) = setup(cx, &source);
        cx.simulate_resize(size(px(560.), px(400.)));
        open(&editor, cx);
        cx.run_until_parked();
        let first = editor.read_with(cx, |e, _| e.outline.backdrop.image_id().unwrap());
        editor.update_in(cx, |_, _, cx| cx.notify());
        cx.run_until_parked();
        assert_eq!(
            editor.read_with(cx, |e, _| e.outline.backdrop.image_id()),
            Some(first)
        );
        cx.simulate_event(ScrollWheelEvent {
            position: point(px(500.), px(200.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-80.))),
            ..Default::default()
        });
        cx.run_until_parked();
        assert_ne!(
            editor.read_with(cx, |e, _| e.outline.backdrop.image_id().unwrap()),
            first
        );
        cx.simulate_keystrokes("escape");
        settle(&editor, cx);
        assert!(editor.read_with(cx, |e, _| e.outline.backdrop.is_empty()));
    }

    #[gpui::test]
    fn folded_active_section_is_revealed_when_returning_to_the_rail(cx: &mut TestAppContext) {
        let source = format!(
            "# Root\n{}",
            (0..60)
                .map(|i| format!("\n## Section {i}\n\nBody\n"))
                .collect::<String>()
        );
        let (editor, cx) = setup(cx, &source);
        cx.simulate_resize(size(px(560.), px(400.)));
        editor.update_in(cx, |e, window, cx| {
            let target = headings(e)[40].source;
            jump(e, target, window, cx);
            e.outline.collapsed.insert(0);
        });
        cx.run_until_parked();
        open(&editor, cx);
        editor.read_with(cx, |e, _| {
            assert_eq!(e.outline.scroll_target, Some((0, 1)));
            assert_eq!(
                visibility(&headings(e), &e.outline.collapsed)
                    .iter()
                    .filter(|v| **v)
                    .count(),
                1
            );
        });
        cx.simulate_keystrokes("escape");
        settle(&editor, cx);
        editor.update_in(cx, |_, _, cx| cx.notify());
        cx.run_until_parked();
        let row = cx.debug_bounds("outline-heading-40").unwrap();
        editor.read_with(cx, |e, _| {
            assert!(e.outline.bounds.unwrap().contains(&row.center()));
        });
        cx.simulate_input("Updated ");
        assert!(editor.read_with(cx, |e, _| e.outline.collapsed.is_empty()));
    }
}
