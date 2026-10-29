use super::{Visual, bounds_from_glyph};
use core_foundation::{
    attributed_string::CFMutableAttributedString,
    base::{CFRange, TCFType},
    boolean::CFBoolean,
    string::CFString,
};
use core_graphics::{
    base::{kCGBitmapByteOrder32Big, kCGImageAlphaPremultipliedLast},
    color_space::CGColorSpace,
    context::CGContext,
    geometry::{CGAffineTransform, CGPoint, CGRect, CGSize},
};
use core_text::{
    font::CTFont,
    font_descriptor::kCTFontOrientationHorizontal,
    line::CTLine,
    string_attributes::{kCTFontAttributeName, kCTForegroundColorFromContextAttributeName},
};
use gpui::{Bounds, Pixels, point, px, size};
use image::RgbaImage;

fn glyph_bounds(font: &CTFont, character: char) -> Bounds<Pixels> {
    let mut glyph = 0;
    let character = character as u16;
    assert!(unsafe { font.get_glyphs_for_characters(&character, &mut glyph, 1) });
    let rect = font.get_bounding_rects_for_glyphs(kCTFontOrientationHorizontal, &[glyph]);
    Bounds::new(
        point(px(rect.origin.x as f32), px(rect.origin.y as f32)),
        size(px(rect.size.width as f32), px(rect.size.height as f32)),
    )
}

fn letter_bounds(font: &CTFont) -> Bounds<Pixels> {
    glyph_bounds(font, 'S')
        .union(&glyph_bounds(font, 'g'))
        .union(&glyph_bounds(font, 'i'))
}

#[test]
fn actual_system_font_placeholder_and_caret_align_within_one_device_pixel() {
    let font = crate::appearance::native_font(15., false, false).unwrap();
    let scale = 2.;
    let (width, height) = (248, 92);
    let logical_height = height as f64 / scale;
    let mut context = CGContext::create_bitmap_context(
        None,
        width,
        height,
        8,
        width * 4,
        &CGColorSpace::create_device_rgb(),
        kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big,
    );
    context.scale(scale, scale);
    context.set_rgb_fill_color(252. / 255., 252. / 255., 252. / 255., 1.);
    context.fill_rect(CGRect::new(
        &CGPoint::new(0., 0.),
        &CGSize::new(124., logical_height),
    ));
    context.set_text_matrix(&CGAffineTransform::new(1., 0., 0., 1., 0., 0.));
    context.set_allows_font_subpixel_positioning(true);
    context.set_should_subpixel_position_fonts(true);
    context.set_allows_font_subpixel_quantization(false);
    context.set_should_subpixel_quantize_fonts(false);

    let mut text = CFMutableAttributedString::new();
    text.replace_str(&CFString::new("Start writing…"), CFRange::init(0, 0));
    let range = CFRange::init(0, text.char_len());
    unsafe {
        text.set_attribute(range, kCTFontAttributeName, &font);
        text.set_attribute(
            range,
            kCTForegroundColorFromContextAttributeName,
            &CFBoolean::true_value(),
        );
    }
    let line = CTLine::new_with_attributed_string(text.as_concrete_TypeRef());
    let metrics = line.get_typographic_bounds();
    let baseline = point(
        px(16.),
        px((8. + (26.25 - metrics.ascent - metrics.descent) / 2. + metrics.ascent) as f32),
    );
    context.set_rgb_fill_color(112. / 255., 112. / 255., 112. / 255., 1.);
    context.set_text_position(
        f32::from(baseline.x) as f64,
        logical_height - f32::from(baseline.y) as f64,
    );
    line.draw(&context);

    let caret = Visual {
        bounds: bounds_from_glyph(baseline, letter_bounds(&font), px(15.), 0),
        opacity: 1.,
    }
    .snap_to_pixels(scale as f32)
    .bounds;
    context.set_rgb_fill_color(17. / 255., 17. / 255., 17. / 255., 1.);
    context.fill_rect(CGRect::new(
        &CGPoint::new(
            f32::from(caret.left()) as f64,
            logical_height - f32::from(caret.bottom()) as f64,
        ),
        &CGSize::new(
            f32::from(caret.size.width) as f64,
            f32::from(caret.size.height) as f64,
        ),
    ));
    let image = RgbaImage::from_raw(width as u32, height as u32, context.data().to_vec()).unwrap();
    let ink_rows = |left, right| {
        (0..height as u32)
            .filter(|&y| (left..right).any(|x| image.get_pixel(x, y)[0] < 230))
            .collect::<Vec<_>>()
    };
    let caret_rows = ink_rows(30, 32);
    let text_rows = ink_rows(33, width as u32);
    assert!(!text_rows.is_empty());
    // Antialiasing can extend one pixel beyond geometric glyph bounds.
    for (caret, text) in [
        (caret_rows.first().unwrap(), text_rows.first().unwrap()),
        (caret_rows.last().unwrap(), text_rows.last().unwrap()),
    ] {
        assert!(
            caret.abs_diff(*text) <= 1,
            "caret {caret} does not align with text {text}"
        );
    }
    if let Ok(path) = std::env::var("MARKDOWN_CARET_PREVIEW") {
        image.save(path).unwrap();
    }
}

#[test]
fn script_caret_matches_scaled_system_font_metrics() {
    let regular = crate::appearance::native_font(15., false, false).unwrap();
    for script in [-1, 1] {
        let font = regular.clone_with_font_size(9.);
        let baseline = point(px(20.), px(40.));
        let caret = bounds_from_glyph(baseline, letter_bounds(&font), px(15.), script);
        let shift = px(15.) * if script > 0 { -0.35 } else { 0.075 };
        let ink = letter_bounds(&font);
        assert!((caret.top() - (baseline.y + shift - ink.bottom())).abs() < px(0.02));
        assert!((caret.bottom() - (baseline.y + shift - ink.top())).abs() < px(0.02));
    }
}

#[test]
fn native_system_font_preserves_weight_and_italic_styles() {
    use core_text::font_descriptor::{TraitAccessors, kCTFontItalicTrait};
    let regular = crate::appearance::native_font(15., false, false).unwrap();
    let regular_weight = regular.all_traits().normalized_weight();
    for bold in [false, true] {
        for italic in [false, true] {
            let font = crate::appearance::native_font(15., bold, italic).unwrap();
            assert_eq!(font.symbolic_traits() & kCTFontItalicTrait != 0, italic);
            assert_eq!(font.all_traits().normalized_weight() > regular_weight, bold);
        }
    }
}
