use crate::editor::{Face, MarkdownEditor, appearance};
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
    font::new_from_name,
    line::CTLine,
    string_attributes::{kCTFontAttributeName, kCTForegroundColorFromContextAttributeName},
};
use gpui::{Bounds, Pixels, Point, RenderImage, point, px, size};
use image::{Frame, RgbaImage, imageops};

const SCALE: f32 = 0.5;
const PAD: f32 = 32.;
const BLUR: f32 = 10.;
const TINT: f32 = 0.84;

struct Fill {
    bounds: Bounds<Pixels>,
    color: u32,
}
struct Text {
    origin: Point<Pixels>,
    value: String,
    size: Pixels,
    face: Face,
    color: u32,
}
pub(super) struct Snapshot {
    region: Bounds<Pixels>,
    fills: Vec<Fill>,
    text: Vec<Text>,
}

impl Snapshot {
    pub fn capture(editor: &MarkdownEditor, bounds: Bounds<Pixels>) -> Self {
        let region = Bounds::new(
            bounds.origin - point(px(PAD), px(PAD)),
            bounds.size + size(px(PAD * 2.), px(PAD * 2.)),
        );
        let clip = region.intersect(&editor.content_bounds());
        let mut snapshot = Self {
            region,
            fills: Vec::new(),
            text: Vec::new(),
        };
        let mut fill = |bounds: Bounds<Pixels>, color| {
            let bounds = bounds.intersect(&clip);
            if !bounds.is_empty() {
                snapshot.fills.push(Fill { bounds, color });
            }
        };
        for panel in &editor.code_panels {
            fill(panel.bounds, appearance::SUBTLE);
        }
        for (line, projected) in editor.layout.iter().zip(&editor.projection.lines) {
            if line.origin.y + line.height < clip.top() || line.origin.y > clip.bottom() {
                continue;
            }
            if projected.quote {
                fill(
                    Bounds::new(
                        point(
                            line.origin.x - px(appearance::BODY_SIZE * 1.25 + 2.),
                            line.origin.y,
                        ),
                        size(px(2.), line.height),
                    ),
                    appearance::BORDER,
                );
            }
            if projected.rule {
                fill(
                    Bounds::new(
                        line.origin + point(px(0.), px(11.)),
                        size(line.width, px(1.)),
                    ),
                    appearance::BORDER,
                );
            }
            for span in &projected.spans {
                if span.face.code && !projected.code {
                    let source = projected.source_index(span.range.start)
                        ..projected.source_index(span.range.end);
                    for rect in line.selection_rects(projected, &source) {
                        fill(rect, appearance::SUBTLE);
                    }
                }
            }
            for rect in line.selection_rects(projected, &editor.document.selection()) {
                fill(rect, appearance::SELECTION);
            }
            if !projected.marker.is_empty() {
                snapshot.text.push(Text {
                    origin: line.origin
                        + point(-px(appearance::BODY_SIZE * 1.5), line.baseline_offset),
                    value: projected.marker.clone(),
                    size: px(appearance::BODY_SIZE),
                    face: Face::default(),
                    color: appearance::TEXT,
                });
            }
            let mut boundaries = vec![0];
            boundaries.extend(
                line.shape
                    .wrap_boundaries()
                    .iter()
                    .map(|wrap| line.shape.runs()[wrap.run_ix].glyphs[wrap.glyph_ix].index),
            );
            boundaries.push(projected.text.len());
            for (row, range) in boundaries.windows(2).enumerate() {
                let y = line.origin.y + line.line_height * row;
                if y + line.line_height < clip.top() || y > clip.bottom() {
                    continue;
                }
                for span in &projected.spans {
                    let start = range[0].max(span.range.start);
                    let end = range[1].min(span.range.end);
                    if start >= end {
                        continue;
                    }
                    let origin = line.position(start) + point(px(0.), line.baseline_offset);
                    if origin.x > clip.right() {
                        continue;
                    }
                    let mut face = span.face;
                    face.bold |= projected.heading > 0;
                    let color = if projected.quote {
                        appearance::MUTED
                    } else {
                        appearance::heading_style(projected.heading)
                            .map_or(appearance::TEXT, |style| style.color)
                    };
                    snapshot.text.push(Text {
                        origin,
                        value: projected.text[start..end].to_string(),
                        size: line.font_size,
                        face,
                        color,
                    });
                }
            }
        }
        snapshot
    }
}

fn number(value: Pixels) -> f64 {
    f32::from(value) as f64
}
fn color(color: u32) -> (f64, f64, f64) {
    (
        ((color >> 16) & 255) as f64 / 255.,
        ((color >> 8) & 255) as f64 / 255.,
        (color & 255) as f64 / 255.,
    )
}

fn raster(snapshot: &Snapshot) -> Option<RgbaImage> {
    let width = (f32::from(snapshot.region.size.width) * SCALE)
        .ceil()
        .max(1.) as u32;
    let height = (f32::from(snapshot.region.size.height) * SCALE)
        .ceil()
        .max(1.) as u32;
    let logical_height = height as f64 / SCALE as f64;
    let mut context = CGContext::create_bitmap_context(
        None,
        width as usize,
        height as usize,
        8,
        width as usize * 4,
        &CGColorSpace::create_device_rgb(),
        kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big,
    );
    context.scale(SCALE as f64, SCALE as f64);
    let (r, g, b) = color(appearance::BACKGROUND);
    context.set_rgb_fill_color(r, g, b, 1.);
    context.fill_rect(CGRect::new(
        &CGPoint::new(0., 0.),
        &CGSize::new(width as f64 / SCALE as f64, logical_height),
    ));
    for fill in &snapshot.fills {
        let (r, g, b) = color(fill.color);
        context.set_rgb_fill_color(r, g, b, 1.);
        let local = fill.bounds.origin - snapshot.region.origin;
        context.fill_rect(CGRect::new(
            &CGPoint::new(
                number(local.x),
                logical_height - number(local.y + fill.bounds.size.height),
            ),
            &CGSize::new(
                number(fill.bounds.size.width),
                number(fill.bounds.size.height),
            ),
        ));
    }
    if snapshot.text.is_empty() {
        return RgbaImage::from_raw(width, height, context.data().to_vec());
    }
    context.set_text_matrix(&CGAffineTransform::new(1., 0., 0., 1., 0., 0.));
    for text in &snapshot.text {
        let font = if text.face.code {
            new_from_name(
                match (text.face.bold, text.face.italic) {
                    (false, false) => "Menlo",
                    (true, false) => "Menlo-Bold",
                    (false, true) => "Menlo-Italic",
                    (true, true) => "Menlo-BoldItalic",
                },
                number(text.size),
            )
            .ok()?
        } else {
            appearance::native_font(number(text.size), text.face.bold, text.face.italic)?
        };
        let mut attributed = CFMutableAttributedString::new();
        attributed.replace_str(&CFString::new(&text.value), CFRange::init(0, 0));
        let range = CFRange::init(0, attributed.char_len());
        // These Core Text constants are immutable global CFStrings.
        unsafe {
            attributed.set_attribute(range, kCTFontAttributeName, &font);
            attributed.set_attribute(
                range,
                kCTForegroundColorFromContextAttributeName,
                &CFBoolean::true_value(),
            );
        }
        let (r, g, b) = color(text.color);
        context.set_rgb_fill_color(r, g, b, 1.);
        let local = text.origin - snapshot.region.origin;
        context.set_text_position(number(local.x), logical_height - number(local.y));
        CTLine::new_with_attributed_string(attributed.as_concrete_TypeRef()).draw(&context);
    }
    RgbaImage::from_raw(width, height, context.data().to_vec())
}

pub(super) fn render(snapshot: Snapshot) -> Option<RenderImage> {
    let mut image = imageops::blur(&raster(&snapshot)?, BLUR * SCALE);
    let background = [
        ((appearance::CHROME >> 16) & 255) as f32,
        ((appearance::CHROME >> 8) & 255) as f32,
        (appearance::CHROME & 255) as f32,
    ];
    for pixel in image.pixels_mut() {
        for (channel, tint) in pixel.0[..3].iter_mut().zip(background) {
            *channel = (*channel as f32 * (1. - TINT) + tint * TINT).round() as u8;
        }
    }
    let padding = (PAD * SCALE) as u32;
    let mut image = imageops::crop_imm(
        &image,
        padding,
        padding,
        image.width() - padding * 2,
        image.height() - padding * 2,
    )
    .to_image();
    // Core Graphics emits RGBA; RenderImage expects BGRA.
    for pixel in image.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Some(RenderImage::new(vec![Frame::new(image)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raster_coordinates_and_blur_preserve_the_backdrop_without_sharp_edges() {
        let snapshot = Snapshot {
            region: Bounds::new(
                point(px(-PAD), px(-PAD)),
                size(px(160. + PAD * 2.), px(120. + PAD * 2.)),
            ),
            fills: vec![Fill {
                bounds: Bounds::new(point(px(20.), px(20.)), size(px(60.), px(40.))),
                color: 0x204080,
            }],
            text: vec![Text {
                origin: point(px(20.), px(100.)),
                value: "System 日本語 👋".into(),
                size: px(15.),
                face: Face {
                    bold: true,
                    ..Face::default()
                },
                color: appearance::TEXT,
            }],
        };
        let sharp = raster(&snapshot).unwrap();
        assert_eq!(sharp.get_pixel(31, 31).0, [32, 64, 128, 255]);
        assert!(
            sharp
                .enumerate_pixels()
                .any(|(_, y, pixel)| y > 55 && pixel.0[0] < 150)
        );
        let blurred = render(snapshot).unwrap();
        assert_eq!(blurred.size(0).width.0, 80);
        assert_eq!(blurred.size(0).height.0, 60);
        let data = blurred.as_bytes(0).unwrap();
        let pixel = |x: usize, y: usize| &data[(y * 80 + x) * 4..(y * 80 + x + 1) * 4];
        let outside = pixel(7, 20);
        assert!(outside[2] < 252 && outside[2] > 220);
        assert!(outside[0] > outside[2]); // BGRA: blue exceeds red.
        assert_eq!(pixel(0, 0)[3], 255);
        if let Ok(path) = std::env::var("MARKDOWN_BLUR_PREVIEW") {
            let mut preview = RgbaImage::from_raw(80, 60, data.to_vec()).unwrap();
            for pixel in preview.pixels_mut() {
                pixel.0.swap(0, 2);
            }
            imageops::resize(&preview, 640, 480, imageops::FilterType::Triangle)
                .save(path)
                .unwrap();
        }
    }
}
