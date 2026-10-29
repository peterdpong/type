use gpui::{
    App, LineLayout, Pixels, Point, SharedString, TextAlign, TextRun, Window, WrapBoundary,
    WrappedLine, WrappedLineLayout, point, px,
};
use std::{ops::Deref, sync::Arc};
use unicode_segmentation::UnicodeSegmentation;

pub(super) fn script_metrics(script: i8) -> (f32, f32) {
    match script {
        0 => (1., 0.),
        n if n > 0 => (0.6, -0.35),
        _ => (0.6, 0.075),
    }
}

#[derive(Clone, Default)]
pub(super) struct StyledLine {
    line: WrappedLine,
    styles: Vec<(usize, TextRun, i8)>,
}

impl Deref for StyledLine {
    type Target = WrappedLine;
    fn deref(&self) -> &Self::Target {
        &self.line
    }
}

impl StyledLine {
    pub fn new(
        text: SharedString,
        font_size: Pixels,
        runs: &[TextRun],
        scripts: &[i8],
        width: Pixels,
        window: &Window,
    ) -> gpui::Result<Self> {
        let mut line = window
            .text_system()
            .shape_text(text.clone(), font_size, runs, Some(width), None)?
            .into_iter()
            .next()
            .unwrap_or_default();
        if scripts.iter().all(|script| *script == 0) {
            return Ok(Self {
                line,
                styles: Vec::new(),
            });
        }
        assert_eq!(runs.len(), scripts.len());
        let mut layout = LineLayout {
            font_size,
            width: px(0.),
            ascent: line.ascent(),
            descent: line.descent(),
            runs: Vec::new(),
            len: text.len(),
        };
        let mut offset = 0;
        let mut styles = Vec::new();
        for (run, &script) in runs.iter().zip(scripts) {
            let end = offset + run.len;
            let shaped = window.text_system().shape_line(
                text[offset..end].to_owned().into(),
                font_size * script_metrics(script).0,
                std::slice::from_ref(run),
                None,
            );
            for mut run in shaped.runs.clone() {
                for glyph in &mut run.glyphs {
                    glyph.index += offset;
                    glyph.position.x += layout.width;
                }
                layout.runs.push(run);
            }
            layout.width += shaped.width;
            styles.push((end, run.clone(), script));
            offset = end;
        }
        let boundaries = wrap_boundaries(&layout, &text, width);
        *line = Arc::new(WrappedLineLayout {
            unwrapped_layout: Arc::new(layout),
            wrap_boundaries: boundaries.into_iter().collect(),
            wrap_width: Some(width),
        });
        Ok(Self { line, styles })
    }

    pub fn paint(
        &self,
        origin: Point<Pixels>,
        line_height: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) -> gpui::Result<()> {
        if self.styles.is_empty() {
            return self
                .line
                .paint(origin, line_height, TextAlign::Left, None, window, cx);
        }
        let layout = &self.line.unwrapped_layout;
        let baseline = (line_height - layout.ascent - layout.descent) / 2. + layout.ascent;
        let glyphs = layout
            .runs
            .iter()
            .enumerate()
            .flat_map(|(run_ix, run)| {
                run.glyphs.iter().enumerate().map(move |(glyph_ix, glyph)| {
                    (WrapBoundary { run_ix, glyph_ix }, run.font_id, glyph)
                })
            })
            .collect::<Vec<_>>();
        let mut row = 0;
        let mut row_x = px(0.);
        for (index, (boundary, font, glyph)) in glyphs.iter().enumerate() {
            if self.line.wrap_boundaries().get(row) == Some(boundary) {
                row += 1;
                row_x = glyph.position.x;
            }
            let style = self
                .styles
                .partition_point(|(end, _, _)| *end <= glyph.index);
            let (_, run, script) = &self.styles[style];
            let (scale, shift) = script_metrics(*script);
            let font_size = layout.font_size * scale;
            let position = origin
                + point(
                    glyph.position.x - row_x,
                    line_height * row + baseline + layout.font_size * shift + glyph.position.y,
                );
            if glyph.is_emoji {
                window.paint_emoji(position, *font, glyph.id, font_size)?;
            } else {
                window.paint_glyph(position, *font, glyph.id, font_size, run.color)?;
            }
            let next_x = glyphs
                .get(index + 1)
                .map_or(layout.width, |(_, _, next)| next.position.x);
            let width = next_x - glyph.position.x;
            if let Some(underline) = &run.underline {
                let mut underline = *underline;
                underline.color = Some(underline.color.unwrap_or(run.color));
                window.paint_underline(
                    position + point(px(0.), layout.descent * scale * 0.618),
                    width,
                    &underline,
                );
            }
            if let Some(strike) = &run.strikethrough {
                let mut strike = *strike;
                strike.color = Some(strike.color.unwrap_or(run.color));
                window.paint_strikethrough(
                    position - point(px(0.), layout.ascent * scale * 0.3),
                    width,
                    &strike,
                );
            }
        }
        Ok(())
    }
}

fn wrap_boundaries(layout: &LineLayout, text: &str, width: Pixels) -> Vec<WrapBoundary> {
    let breaks = unicode_linebreak::linebreaks(text)
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    let graphemes = text
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    let glyphs = layout
        .runs
        .iter()
        .enumerate()
        .flat_map(|(run_ix, run)| {
            run.glyphs
                .iter()
                .enumerate()
                .map(move |(glyph_ix, glyph)| (WrapBoundary { run_ix, glyph_ix }, glyph))
        })
        .collect::<Vec<_>>();
    let mut result = Vec::new();
    let mut row_index = 0;
    let mut row_x = px(0.);
    let mut candidate = None;
    for (i, (boundary, glyph)) in glyphs.iter().enumerate() {
        if breaks.binary_search(&glyph.index).is_ok() && glyph.index > row_index {
            candidate = Some((*boundary, glyph.index, glyph.position.x));
        }
        let next = glyphs[i + 1..]
            .iter()
            .find(|(_, next)| next.index > glyph.index);
        let end_x = next.map_or(layout.width, |(_, next)| next.position.x);
        if end_x - row_x > width
            && glyph.index > row_index
            && graphemes.binary_search(&glyph.index).is_ok()
        {
            let (boundary, index, x) =
                candidate
                    .take()
                    .unwrap_or((*boundary, glyph.index, glyph.position.x));
            result.push(boundary);
            row_index = index;
            row_x = x;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::StyledLine;
    use crate::editor::{Face, MarkdownEditor, init, text_run};
    use gpui::{TestAppContext, point, px};
    use unicode_segmentation::UnicodeSegmentation;

    #[gpui::test]
    fn script_widths_and_wrapping_use_the_scaled_font(cx: &mut TestAppContext) {
        cx.update(init);
        let (_, cx) = cx.add_window_view(|_, cx| MarkdownEditor::new("", cx));
        cx.update(|window, _| {
            let runs = [
                text_run(2, Face::default()),
                text_run(
                    5,
                    Face {
                        bold: true,
                        italic: true,
                        ..Face::default()
                    },
                ),
                text_run(2, Face::default()),
            ];
            let parts = ["a ", "hello", " z"];
            let widths = parts
                .iter()
                .zip(&runs)
                .enumerate()
                .map(|(i, (text, run))| {
                    window
                        .text_system()
                        .shape_line(
                            (*text).into(),
                            px(if i == 1 { 9. } else { 15. }),
                            std::slice::from_ref(run),
                            None,
                        )
                        .width
                })
                .collect::<Vec<_>>();
            let scaled_width = widths.iter().copied().fold(px(0.), |a, b| a + b);
            let full = window
                .text_system()
                .shape_line("a hello z".into(), px(15.), &runs, None)
                .width;
            let width = (scaled_width + full) / 2.;
            let line = StyledLine::new(
                "a hello z".into(),
                px(15.),
                &runs,
                &[0, 1, 0],
                width,
                window,
            )
            .unwrap();
            assert!((line.unwrapped_layout.width - scaled_width).abs() < px(0.01));
            assert!(line.wrap_boundaries().is_empty());
            assert_eq!(line.position_for_index(2, px(26.)).unwrap().x, widths[0]);
            assert_eq!(
                line.position_for_index(7, px(26.)).unwrap().x,
                widths[0] + widths[1]
            );
            let ordinary = StyledLine::new(
                "a hello z".into(),
                px(15.),
                &runs,
                &[0, 0, 0],
                width,
                window,
            )
            .unwrap();
            assert!(!ordinary.wrap_boundaries().is_empty());
            assert_eq!(line.styles[1].1.font, runs[1].font);
        });
    }

    #[gpui::test]
    fn wrapped_scripts_keep_unicode_hit_testing_and_selection(cx: &mut TestAppContext) {
        cx.update(init);
        let (editor, cx) =
            cx.add_window_view(|_, cx| MarkdownEditor::new("x^hellohellohello👋界hello^ end", cx));
        editor.update_in(cx, |editor, window, _| {
            let projected = &editor.projection.lines[0];
            let runs = projected
                .spans
                .iter()
                .map(|span| text_run(span.range.len(), span.face))
                .collect::<Vec<_>>();
            let scripts = projected
                .spans
                .iter()
                .map(|span| span.face.script)
                .collect::<Vec<_>>();
            assert!(scripts.iter().any(|script| *script != 0));
            let line = StyledLine::new(
                projected.text.clone().into(),
                px(15.),
                &runs,
                &scripts,
                px(45.),
                window,
            )
            .unwrap();
            assert!(line.wrap_boundaries().len() > 1);
            let graphemes = projected
                .text
                .grapheme_indices(true)
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            for wrap in line.wrap_boundaries() {
                let index = line.runs()[wrap.run_ix].glyphs[wrap.glyph_ix].index;
                assert!(graphemes.contains(&index));
            }
            let laid = super::super::LaidLine {
                baseline_offset: (px(26.) - line.ascent() - line.descent()) / 2. + line.ascent(),
                height: line.size(px(26.)).height,
                shape: line,
                origin: point(px(0.), px(0.)),
                font_size: px(15.),
                line_height: px(26.),
                width: px(45.),
            };
            for index in graphemes {
                let position = laid.position(index);
                assert_eq!(laid.index(position + point(px(0.), px(13.))), index);
            }
            let rects = laid.display_rects(0..projected.text.len());
            assert_eq!(rects.len(), laid.shape.wrap_boundaries().len() + 1);
            assert!(rects.iter().all(|rect| rect.size.height == px(26.)));
        });
    }
}
