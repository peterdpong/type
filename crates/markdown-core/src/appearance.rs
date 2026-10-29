//! Shared typography and colors.
pub const FONT_FAMILY: &str = ".SystemUIFont";
pub const UI_SIZE: f32 = 13.;
pub const BODY_SIZE: f32 = 15.;
pub const TOP_GUTTER: f32 = 40.;
pub const BODY_LINE_HEIGHT: f32 = BODY_SIZE * 1.75;
pub const PARAGRAPH_GAP: f32 = BODY_SIZE * 1.25;
pub const CODE_SIZE: f32 = BODY_SIZE * 0.85;
pub const CODE_LINE_HEIGHT: f32 = BODY_SIZE * 1.65;
pub const CODE_PADDING: f32 = 16.;
pub const CONTENT_WIDTH: f32 = 640.;
pub const BACKGROUND: u32 = 0xfcfcfc;
pub const TEXT: u32 = 0x111111;
pub const MUTED: u32 = 0x707070;
pub const BORDER: u32 = 0xe5e5e5;
pub const SUBTLE: u32 = 0xf2f2f2;
pub const SELECTION: u32 = 0xbcd6f2;

pub const CHROME: u32 = 0xefefed;
pub const CHROME_BORDER: u32 = 0xdededa;
pub const CHROME_HOVER: u32 = 0xe5e5e1;
pub const CHROME_RADIUS: f32 = 8.;

#[derive(Clone, Copy)]
pub(crate) struct HeadingStyle {
    pub size: f32,
    pub line_height: f32,
    pub space_before: f32,
    pub color: u32,
}

pub(crate) fn heading_style(level: usize) -> Option<HeadingStyle> {
    let (size, line_height, space_before) = match level {
        1 => (32., 44., 40.),
        2 => (26., 38., 32.),
        3 => (21., 32., 24.),
        4 => (18., 28., 20.),
        5 => (16., 26., 16.),
        6 => (15., 25., 12.),
        _ => return None,
    };
    Some(HeadingStyle {
        size,
        line_height,
        space_before,
        color: if level == 6 { MUTED } else { TEXT },
    })
}

#[cfg(target_os = "macos")]
pub(crate) fn native_font(size: f64, bold: bool, italic: bool) -> Option<core_text::font::CTFont> {
    use core_foundation::{
        base::TCFType, dictionary::CFDictionary, number::CFNumber, string::CFString,
    };
    use core_text::{font, font_descriptor::*};
    let font = font::new_from_name(".AppleSystemUIFont", size).ok()?;
    let descriptor = if bold {
        let traits = CFDictionary::from_CFType_pairs(&[(
            unsafe { CFString::wrap_under_get_rule(kCTFontWeightTrait) },
            CFNumber::from(0.3_f64).as_CFType(),
        )]);
        font.copy_descriptor()
            .create_copy_with_attributes(
                CFDictionary::from_CFType_pairs(&[(
                    unsafe { CFString::wrap_under_get_rule(kCTFontTraitsAttribute) },
                    traits.as_CFType(),
                )])
                .to_untyped(),
            )
            .ok()?
    } else {
        font.copy_descriptor()
    };
    let font = font::new_from_descriptor(&descriptor, size);
    if italic {
        font.clone_with_symbolic_traits(kCTFontItalicTrait, kCTFontItalicTrait)
    } else {
        Some(font)
    }
}
