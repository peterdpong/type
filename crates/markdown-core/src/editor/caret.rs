use gpui::{Bounds, FontId, Pixels, Point, TextSystem, point, px, size};
const WIDTH: f32 = 1.;
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
