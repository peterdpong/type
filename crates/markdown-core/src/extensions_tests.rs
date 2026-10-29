use crate::{Document, projection::Projection};
use pulldown_cmark::Alignment;

#[test]
fn bare_heading_markers_stay_literal_in_imported_blocks() {
    for level in 1..=6 {
        let hashes = "#".repeat(level);
        for prefix in ["", "   ", "> ", "> > ", "- "] {
            for ending in ["", "\n", "\r\n"] {
                let source = format!("{prefix}{hashes}{ending}");
                let p = Projection::new(&source, false);
                assert_eq!(p.lines[0].heading, 0, "{source:?}");
                assert!(p.lines[0].text.ends_with(&hashes), "{source:?}");

                let source = format!("{prefix}{hashes} {ending}");
                let p = Projection::new(&source, false);
                assert_eq!(p.lines[0].heading, level, "{source:?}");
            }
        }
        let p = Projection::new(&format!("{hashes}Title"), false);
        assert_eq!(p.lines[0].heading, 0);
        assert_eq!(p.lines[0].text, format!("{hashes}Title"));
    }
}

#[test]
fn tables_have_editable_aligned_cells_and_inline_faces() {
    let source = "| Style | Sample | Alignment |\r\n| :--- | :---: | ---: |\r\n| Bold | **Téxt** | 1 |\r\n| Code | `value` | 2 |";
    let p = Projection::new(source, false);
    assert_eq!(p.tables.len(), 1);
    assert_eq!(p.lines.len(), 9);
    assert_eq!(
        p.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
        [
            "Style",
            "Sample",
            "Alignment",
            "Bold",
            "Téxt",
            "1",
            "Code",
            "value",
            "2"
        ]
    );
    assert_eq!(p.lines[0].cell.as_ref().unwrap().alignment, Alignment::Left);
    assert_eq!(
        p.lines[1].cell.as_ref().unwrap().alignment,
        Alignment::Center
    );
    assert_eq!(
        p.lines[2].cell.as_ref().unwrap().alignment,
        Alignment::Right
    );
    assert!(p.lines[4].spans[0].face.bold);
    assert!(p.lines[7].spans[0].face.code);
    for line in p.lines {
        for (i, _) in line.text.char_indices() {
            assert!(source.is_char_boundary(line.source_index(i)));
        }
    }
}

#[test]
fn references_autolinks_and_images_keep_destinations_out_of_text() {
    let source = "A [**label**][id], [id][] and [id]. <https://example.com>.\n\n[id]: https://example.com \"Title\"\n\n![Alt 🦀](./picture.png \"Caption\")";
    let p = Projection::new(source, false);
    assert_eq!(p.links.len(), 4);
    assert_eq!(p.lines[0].text, "A label, id and id. https://example.com.");
    assert_eq!(p.pictures[0].destination, "./picture.png");
    assert_eq!(p.pictures[0].title, "Caption");
    assert_eq!(p.lines.last().unwrap().text, "Alt 🦀");
    assert!(!p.lines.iter().any(|l| l.text.contains("[id]:")));
    assert_eq!(Document::new(source).text(), source);
}

#[test]
fn setext_headings_quotes_breaks_and_comments_are_structural() {
    let source = "Title\n=====\n\nSubtitle\n--------\n\n> Outer\n> > Inner\n> > > Deep\n\nFirst  \nSecond\\\nThird\n\n<!-- preserved\ncomment -->\n\nA <!-- inline --> B";
    let p = Projection::new(source, false);
    assert_eq!(p.lines[0].heading, 1);
    assert_eq!(p.lines[2].heading, 2);
    assert_eq!(
        p.lines
            .iter()
            .filter(|l| l.quote)
            .map(|l| (l.text.as_str(), l.quote_depth))
            .collect::<Vec<_>>(),
        [("Outer", 1), ("Inner", 2), ("Deep", 3)]
    );
    assert!(p.lines.iter().any(|l| l.text == "First"));
    assert!(p.lines.iter().any(|l| l.text == "Second"));
    assert_eq!(p.lines.last().unwrap().text, "A  B");
    assert!(!p.lines.iter().any(|l| l.text.contains("comment")));
}

#[test]
fn inline_extensions_do_not_change_strike_or_code_and_preserve_unicode() {
    let source = "<mark>Héllo **bold**</mark> <kbd>⌘S</kbd> ==yellow== H~2~O x^2^ ~strike~ ~~also~~ `==literal==`";
    let p = Projection::new(source, false);
    let line = &p.lines[0];
    assert_eq!(
        line.text,
        "Héllo bold ⌘S yellow H2O x2 strike also ==literal=="
    );
    for (text, check) in [
        ("Héllo", 0),
        ("⌘S", 1),
        ("yellow", 0),
        ("2O", 2),
        ("2 strike", 3),
        ("strike", 4),
        ("==literal==", 5),
    ] {
        let i = line.text.find(text).unwrap();
        let face = line
            .spans
            .iter()
            .find(|s| s.range.contains(&i))
            .unwrap()
            .face;
        assert!(
            match check {
                0 => face.highlight,
                1 => face.keyboard,
                2 => face.script == -1 && !face.strike,
                3 => face.script == 1,
                4 => face.strike,
                _ => face.code && !face.highlight,
            },
            "{text}: {face:?}"
        );
    }
    assert_eq!(
        Projection::new("<mark>unfinished", false).lines[0].text,
        "<mark>unfinished"
    );
}

#[test]
fn footnotes_work_in_either_order_and_math_projects_without_rewriting_tex() {
    let source = "[^note]: A definition.\n\nNote[^note] and $E = mc^2$.\n\n$$\na^2 + b^2 = c^2\n$$\n\n$\\alpha + x_{12} \\leq \\pi$";
    let p = Projection::new(source, false);
    assert_eq!(p.lines[0].text, "A definition.");
    assert_eq!(p.lines[0].marker, "1.");
    assert_eq!(p.lines[2].text, "Note1 and E = mc2.");
    assert_eq!(
        p.links[0].target,
        Some(source.find("A definition").unwrap())
    );
    assert_eq!(
        p.lines.iter().find(|l| l.math).unwrap().text,
        "a2 + b2 = c2"
    );
    assert_eq!(p.lines.last().unwrap().text, "α + x12 ≤ π");
    assert_eq!(Document::new(source).text(), source);
    for line in p.lines {
        for (i, _) in line.text.char_indices() {
            assert!(source.is_char_boundary(line.source_index(i)));
        }
    }
}

#[test]
fn complete_fixture_round_trips_and_every_prefix_keeps_valid_source_maps() {
    let source = include_str!("../../../examples/extensions.md");
    assert_eq!(Document::new(source).text(), source);
    for end in source
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(source.len()))
    {
        let text = &source[..end];
        let p = Projection::new(text, false);
        for line in p.lines {
            assert_eq!(line.map.len(), line.text.len() + 1);
            assert!(line.source.end <= text.len());
            for (i, _) in line
                .text
                .char_indices()
                .chain(std::iter::once((line.text.len(), '\0')))
            {
                assert!(
                    text.is_char_boundary(line.source_index(i)),
                    "prefix {end}, {:?}",
                    line.text
                );
            }
        }
    }
}

#[test]
fn extended_inline_undo_rejects_the_formatting_and_setext_formatting_is_atomic() {
    for source in ["==highlight==", "x^2^", "H~2~", "$E = mc^2$"] {
        let mut document = Document::new("");
        for ch in source.chars() {
            document.insert(&ch.to_string());
        }
        assert!(document.undo());
        assert_eq!(
            Projection::new(document.text(), false).lines[0].text,
            source,
            "{source}"
        );
        document.redo();
        assert_eq!(document.text(), source);
    }
    let mut document = Document::new("Title\n=====\n\nBody");
    document.set_prefix("### ");
    assert_eq!(document.text(), "### Title\n\nBody");
    document.undo();
    assert_eq!(document.text(), "Title\n=====\n\nBody");
    document.set_prefix("# ");
    assert_eq!(document.text(), "Title\n\nBody");
}
