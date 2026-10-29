use super::{Projection, appearance, heading_text_run};
use gpui::{prelude::*, *};
use std::sync::Arc;

const MAX_LINES: usize = 48;
const MAX_BYTES: usize = 8192;

struct PreviewLine {
    text: SharedString,
    runs: Vec<TextRun>,
    heading: usize,
    code: bool,
    quote: bool,
    marker: SharedString,
    depth: usize,
    checkbox: Option<bool>,
    rule: bool,
}

/// A read-only document thumbnail.
#[derive(Clone, IntoElement)]
pub struct MarkdownPreview {
    lines: Arc<[PreviewLine]>,
}

impl MarkdownPreview {
    pub(super) fn new(projection: &Projection) -> Self {
        let mut remaining = MAX_BYTES;
        let lines = projection
            .lines
            .iter()
            .take(MAX_LINES)
            .map_while(|line| {
                if remaining == 0 {
                    return None;
                }
                let mut end = line.text.len().min(remaining);
                while !line.text.is_char_boundary(end) {
                    end -= 1;
                }
                remaining = if end < line.text.len() {
                    0
                } else {
                    remaining - end
                };
                let checked = line.checkbox.is_some_and(|checkbox| checkbox.checked);
                let runs = line
                    .spans
                    .iter()
                    .take_while(|span| span.range.start < end)
                    .map(|span| {
                        let mut run = heading_text_run(
                            span.range.end.min(end) - span.range.start,
                            span.face,
                            line.heading,
                        );
                        if line.code {
                            run.background_color = None;
                        }
                        if line.quote || checked {
                            run.color = rgb(appearance::MUTED).into();
                        }
                        if checked {
                            run.strikethrough = Some(StrikethroughStyle {
                                color: None,
                                thickness: px(0.4),
                            });
                        }
                        if let Some(underline) = &mut run.underline {
                            underline.thickness = px(0.4);
                        }
                        if let Some(strike) = &mut run.strikethrough {
                            strike.thickness = px(0.4);
                        }
                        run
                    })
                    .collect();
                Some(PreviewLine {
                    text: line.text[..end].to_owned().into(),
                    runs,
                    heading: line.heading,
                    code: line.code,
                    quote: line.quote,
                    marker: line.marker.clone().into(),
                    depth: line.list_depth.unwrap_or(0),
                    checkbox: line.checkbox.map(|c| c.checked),
                    rule: line.rule,
                })
            })
            .collect::<Vec<_>>();
        Self {
            lines: lines.into(),
        }
    }
}

struct Row {
    line: WrappedLine,
    origin: Point<Pixels>,
    height: Pixels,
    background: Option<Bounds<Pixels>>,
    quote: Option<Bounds<Pixels>>,
    rule: Option<Bounds<Pixels>>,
    marker: Option<(ShapedLine, Point<Pixels>)>,
}

impl RenderOnce for MarkdownPreview {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        canvas(
            move |bounds, window, _| {
                let mut rows = Vec::new();
                if !bounds.intersects(&window.content_mask().bounds) {
                    return rows;
                }
                let left = bounds.left() + px(12.);
                let width = (bounds.size.width - px(24.)).max(px(1.));
                let scale = f32::from(width) / appearance::CONTENT_WIDTH;
                let mut y = bounds.top() + px(12.);
                for (index, line) in self.lines.iter().enumerate() {
                    if y >= bounds.bottom() - px(8.) {
                        break;
                    }
                    let heading = appearance::heading_style(line.heading);
                    if index > 0
                        && let Some(heading) = heading
                    {
                        y += px(heading.space_before * scale);
                    }
                    let font_size = px(if line.code {
                        appearance::CODE_SIZE
                    } else {
                        heading.map_or(appearance::BODY_SIZE, |h| h.size)
                    } * scale);
                    let height = px(if line.code {
                        appearance::CODE_LINE_HEIGHT
                    } else {
                        heading.map_or(appearance::BODY_LINE_HEIGHT, |h| h.line_height)
                    } * scale);
                    let indent = px(if line.code {
                        appearance::CODE_PADDING
                    } else if line.quote {
                        20.
                    } else if line.checkbox.is_some() || !line.marker.is_empty() {
                        24. + line.depth as f32 * 24.
                    } else {
                        0.
                    } * scale)
                    .min(width / 2.);
                    let origin = point(left + indent, y);
                    let text_width = (width
                        - indent
                        - if line.code {
                            px(appearance::CODE_PADDING * scale)
                        } else {
                            px(0.)
                        })
                    .max(px(1.));
                    let shape = window
                        .text_system()
                        .shape_text(
                            line.text.clone(),
                            font_size,
                            &line.runs,
                            Some(text_width),
                            None,
                        )
                        .ok()
                        .and_then(|lines| lines.into_iter().next())
                        .unwrap_or_default();
                    let total = shape.size(height).height;
                    let marker_text: SharedString = match line.checkbox {
                        Some(true) => "☑".into(),
                        Some(false) => "☐".into(),
                        None => line.marker.clone(),
                    };
                    let marker = if marker_text.is_empty() {
                        None
                    } else {
                        let marker = window.text_system().shape_line(
                            marker_text.clone(),
                            px(appearance::BODY_SIZE * scale),
                            &[heading_text_run(marker_text.len(), Default::default(), 0)],
                            None,
                        );
                        let position = point(origin.x - marker.width - px(4. * scale), y);
                        Some((marker, position))
                    };
                    rows.push(Row {
                        line: shape,
                        origin,
                        height,
                        background: line
                            .code
                            .then(|| Bounds::new(point(left, y), size(width, total))),
                        quote: line
                            .quote
                            .then(|| Bounds::new(point(left, y), size(px(0.7), total))),
                        rule: line.rule.then(|| {
                            Bounds::new(point(left, y + height / 2.), size(width, px(0.5)))
                        }),
                        marker,
                    });
                    y += total;
                }
                rows
            },
            |bounds, rows, window, cx| {
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    for row in rows {
                        if let Some(background) = row.background {
                            window.paint_quad(fill(background, rgb(appearance::SUBTLE)));
                        }
                        for rect in [row.quote, row.rule].into_iter().flatten() {
                            window.paint_quad(fill(rect, rgb(appearance::BORDER)));
                        }
                        if let Some((marker, origin)) = row.marker {
                            let _ = marker.paint(origin, row.height, window, cx);
                        }
                        let _ = row.line.paint_background(
                            row.origin,
                            row.height,
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                        let _ = row.line.paint(
                            row.origin,
                            row.height,
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                    }
                });
            },
        )
        .size_full()
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_BYTES, MAX_LINES, MarkdownPreview, Projection};
    #[test]
    fn preview_preserves_rendered_styles_and_bounds_large_unicode_documents() {
        let preview = MarkdownPreview::new(&Projection::new(
            "# Title\n\n**bold** and `code`\n\n- [x] Done\n\n```rs\nlet x = 1;\n```",
            false,
        ));
        assert_eq!(preview.lines[0].heading, 1);
        assert!(
            preview
                .lines
                .iter()
                .any(|line| line.text.as_ref() == "bold and code")
        );
        assert!(preview.lines.iter().any(|line| line.checkbox == Some(true)));
        assert!(
            preview
                .lines
                .iter()
                .any(|line| line.code && line.text.contains("let x"))
        );
        let source = format!("**{}**\n{}", "🦀é".repeat(3000), "tail\n".repeat(100));
        let preview = MarkdownPreview::new(&Projection::new(&source, false));
        assert!(preview.lines.len() <= MAX_LINES);
        assert!(
            preview
                .lines
                .iter()
                .map(|line| line.text.len())
                .sum::<usize>()
                <= MAX_BYTES
        );
        for line in preview.lines.iter() {
            assert_eq!(
                line.runs.iter().map(|run| run.len).sum::<usize>(),
                line.text.len()
            );
        }
    }
}
