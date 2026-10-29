use super::{Copy, Format, MarkdownEditor};
use crate::appearance;
use gpui::{
    Animation, AnimationExt, AnyElement, AvailableSpace, Bounds, BoxShadow, Context, CursorStyle,
    Div, FontWeight, MouseButton, Pixels, Point, Size, Stateful, Window, div, ease_out_quint,
    point, prelude::*, px, rgb, rgba,
};
use std::time::Duration;

fn button(id: &'static str, label: &'static str, active: bool) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .h(px(28.))
        .min_w(px(30.))
        .px_2()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .cursor_pointer()
        .when(active, |view| view.bg(rgb(appearance::BORDER)))
        .hover(|view| view.bg(rgb(appearance::SUBTLE)))
        .child(label)
}

fn position(
    anchor: Bounds<Pixels>,
    viewport: Bounds<Pixels>,
    size: Size<Pixels>,
) -> Option<Point<Pixels>> {
    let left = viewport.left() + px(8.);
    let right = viewport.right() - px(8.) - size.width;
    let top = viewport.top() + px(appearance::TOP_GUTTER);
    let bottom = viewport.bottom() - px(8.) - size.height;
    if right < left || bottom < top {
        return None;
    }
    let above = anchor.top() - size.height - px(8.);
    let y = if above >= top {
        above
    } else {
        anchor.bottom() + px(8.)
    };
    Some(point(
        (anchor.center().x - size.width / 2.).clamp(left, right),
        y.clamp(top, bottom),
    ))
}

pub(super) fn show(
    editor: &mut MarkdownEditor,
    window: &mut Window,
    cx: &mut Context<MarkdownEditor>,
) {
    #[cfg(test)]
    {
        editor.toolbelt_bounds = None;
    }
    let selection = editor.document.selection();
    if selection.is_empty()
        || editor.find.open
        || editor.selecting
        || editor.toolbelt_dismissed
        || editor.outline.is_open()
        || editor.marked.is_some()
        || !editor.focus.is_focused(window)
    {
        return;
    }
    let visible = editor.content_bounds();
    let anchor = editor
        .layout
        .iter()
        .zip(&editor.projection.lines)
        .flat_map(|(line, projected)| line.selection_rects(projected, &selection))
        .map(|rect| rect.intersect(&visible))
        .find(|rect| rect.size.width > px(0.) && rect.size.height > px(0.));
    let Some(anchor) = anchor else {
        return;
    };
    let first = editor.projection.line_for_source(selection.start);
    let last = editor
        .projection
        .line_for_source(selection.end.saturating_sub(1));
    let line = &editor.projection.lines[first];
    let prefix = crate::document::block_prefix(&editor.document.text()[line.source.clone()]);
    let inline = first == last
        && !editor.projection.editing_metadata
        && !line.code
        && !line.rule
        && selection.start >= line.source.start + prefix.len()
        && selection.end <= line.source.end
        && !editor.document.text()[selection.clone()].trim().is_empty();
    let mut belt = div()
        .id("selection-toolbelt")
        .debug_selector(|| "selection-toolbelt".into())
        .flex()
        .items_center()
        .gap_1()
        .p_1()
        .rounded_lg()
        .border_1()
        .border_color(rgb(appearance::BORDER))
        .bg(rgb(0xffffff))
        .font_family(appearance::FONT_FAMILY)
        .text_size(px(appearance::UI_SIZE))
        .text_color(rgb(appearance::TEXT))
        .cursor(CursorStyle::Arrow)
        .shadow(vec![BoxShadow {
            color: rgba(0x00000018).into(),
            offset: point(px(0.), px(4.)),
            blur_radius: px(14.),
            spread_radius: px(0.),
        }])
        .occlude()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation());
    if inline {
        belt = belt
            .child(
                button("toolbelt-bold", "B", editor.format_active(Format::Bold))
                    .font_weight(FontWeight::SEMIBOLD)
                    .on_click(cx.listener(|editor, _, window, cx| {
                        editor.format(Format::Bold, cx);
                        window.focus(&editor.focus);
                    })),
            )
            .child(
                button("toolbelt-italic", "I", editor.format_active(Format::Italic))
                    .italic()
                    .on_click(cx.listener(|editor, _, window, cx| {
                        editor.format(Format::Italic, cx);
                        window.focus(&editor.focus);
                    })),
            )
            .child(
                button("toolbelt-code", "<>", editor.format_active(Format::Code)).on_click(
                    cx.listener(|editor, _, window, cx| {
                        editor.format(Format::Code, cx);
                        window.focus(&editor.focus);
                    }),
                ),
            )
            .child(
                div()
                    .w(px(1.))
                    .h(px(16.))
                    .mx_1()
                    .bg(rgb(appearance::BORDER)),
            );
    }
    let mut belt: AnyElement = belt
        .child(
            button("toolbelt-copy", "Copy", false)
                .on_click(cx.listener(|editor, _, window, cx| editor.copy(&Copy, window, cx))),
        )
        .with_animation(
            "toolbelt-entrance",
            Animation::new(Duration::from_millis(140)).with_easing(ease_out_quint()),
            |view, progress| view.opacity(progress),
        )
        .into_any_element();
    let measured = belt.layout_as_root(AvailableSpace::min_size(), window, cx);
    let mut viewport = editor.bounds;
    if editor.projection.lines.iter().any(|line| line.heading > 0) {
        viewport.origin.x += px(64.);
        viewport.size.width -= px(64.);
    }
    if let Some(origin) = position(anchor, viewport, measured) {
        #[cfg(test)]
        {
            editor.toolbelt_bounds = Some(Bounds::new(origin, measured));
        }
        window.defer_draw(belt, origin, 1);
    }
}
