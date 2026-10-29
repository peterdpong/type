//! Markdown block structures and extensions.
use crate::projection::Face;
use pulldown_cmark::{Alignment, Event, LinkType, Options, Parser, Tag, TagEnd};
use std::ops::Range;

pub(crate) fn options() -> Options {
    Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_MATH
}

#[derive(Clone, Debug)]
pub(crate) struct Cell {
    pub table: usize,
    pub row: usize,
    pub column: usize,
    pub columns: usize,
    pub missing: bool,
    pub alignment: Alignment,
}
#[derive(Clone, Debug)]
pub(crate) struct Table {
    pub source: Range<usize>,
    pub alignments: Vec<Alignment>,
    pub rows: Vec<Range<usize>>,
}
#[derive(Clone, Debug)]
pub(crate) struct Picture {
    pub source: Range<usize>,
    pub destination: String,
    pub title: String,
}
#[derive(Clone, Debug)]
pub(crate) struct Link {
    pub source: Range<usize>,
    pub destination: String,
    pub target: Option<usize>,
}
#[derive(Clone, Debug)]
pub(crate) struct Replacement {
    pub source: Range<usize>,
    pub text: String,
}
#[derive(Clone, Debug)]
pub(crate) struct Part {
    pub source: Range<usize>,
    pub cell: Option<Cell>,
    pub picture: Option<usize>,
}
#[derive(Default)]
pub(crate) struct Extensions {
    pub inline_styles: Vec<Range<usize>>,
    pub tables: Vec<Table>,
    pub pictures: Vec<Picture>,
    pub links: Vec<Link>,
    pub replacements: Vec<Replacement>,
    pub math: Vec<Range<usize>>,
    pub headings: Vec<(Range<usize>, usize)>,
    pub quotes: Vec<Range<usize>>,
    pub footnotes: Vec<(Range<usize>, usize, usize)>,
    pub skipped: Vec<Range<usize>>,
    pub metadata: Vec<Range<usize>>,
    cells: Vec<Part>,
}

impl Extensions {
    pub fn new(
        source: &str,
        hidden: &mut [bool],
        faces: &mut [Face],
        markers: &mut Vec<Range<usize>>,
    ) -> Self {
        let mut out = Self::default();
        let parser = Parser::new_ext(source, options());
        for (_, definition) in parser.reference_definitions().iter() {
            out.skipped.push(definition.span.clone());
            out.metadata.push(definition.span.clone());
        }
        let events = crate::inline::events(source, options()).collect::<Vec<_>>();
        let mut notes = Vec::<String>::new();
        for (event, _) in &events {
            if let Event::FootnoteReference(label) = event
                && !notes.iter().any(|note| note == label.as_ref())
            {
                notes.push(label.to_string());
            }
        }
        let mut eligible = vec![false; source.len()];
        let mut html = Vec::<(String, Range<usize>)>::new();
        let mut current_table = None;
        let mut row = 0;
        let mut column = 0;
        let mut code = false;
        for (event, range) in events {
            let slice = &source[range.clone()];
            match event {
                Event::Start(Tag::CodeBlock(_)) => code = true,
                Event::End(TagEnd::CodeBlock) => code = false,
                Event::Text(_) if !code => eligible[range].fill(true),
                Event::Start(Tag::Table(alignments)) => {
                    current_table = Some(out.tables.len());
                    out.tables.push(Table {
                        source: range,
                        alignments,
                        rows: Vec::new(),
                    });
                    row = 0;
                }
                Event::End(TagEnd::Table) => current_table = None,
                Event::Start(Tag::TableHead | Tag::TableRow) => {
                    column = 0;
                    out.tables[current_table.unwrap()].rows.push(range);
                }
                Event::End(TagEnd::TableHead | TagEnd::TableRow) => row += 1,
                Event::Start(Tag::TableCell) => {
                    let table = current_table.unwrap();
                    let alignment = out.tables[table].alignments[column];
                    let row_range = &out.tables[table].rows[row];
                    let row_end = row_range.start
                        + source[row_range.clone()]
                            .trim_end_matches(['\r', '\n'])
                            .len();
                    let missing = range.is_empty() && range.start >= row_end;
                    let content = if missing {
                        let row_range = &out.tables[table].rows[row];
                        let end = row_range.start
                            + source[row_range.clone()]
                                .trim_end_matches(['\r', '\n'])
                                .len();
                        end..end
                    } else {
                        let start = range.start + slice.len() - slice.trim_start().len();
                        let end = range.end - slice.len() + slice.trim_end().len();
                        hidden[range.start..start].fill(true);
                        hidden[end.max(start)..range.end].fill(true);
                        start..end.max(start)
                    };
                    if row == 0 {
                        for face in &mut faces[range.clone()] {
                            face.bold = true;
                        }
                    }
                    out.cells.push(Part {
                        source: content,
                        cell: Some(Cell {
                            table,
                            row,
                            column,
                            columns: out.tables[table].alignments.len(),
                            missing,
                            alignment,
                        }),
                        picture: None,
                    });
                    column += 1;
                }
                Event::Start(Tag::Heading { level, .. }) => {
                    // Heading formatting requires a space after the hashes.
                    if slice
                        .trim_end_matches(['\n', '\r'])
                        .trim_start()
                        .bytes()
                        .all(|byte| byte == b'#')
                    {
                        continue;
                    }
                    out.headings.push((range.clone(), level as usize));
                    if !slice.trim_start().starts_with('#')
                        && let Some((offset, underline)) =
                            slice.trim_end_matches(['\n', '\r']).rsplit_once('\n')
                    {
                        let line = underline.trim();
                        if !line.is_empty()
                            && (line.bytes().all(|b| b == b'=') || line.bytes().all(|b| b == b'-'))
                        {
                            out.skipped.push(range.start + offset.len() + 1..range.end);
                        }
                    }
                }
                Event::Start(Tag::BlockQuote(_)) => out.quotes.push(range),
                Event::Start(Tag::Link {
                    dest_url,
                    link_type,
                    ..
                }) => {
                    let mut range = range;
                    if link_type == LinkType::Collapsed && source[range.end..].starts_with("[]") {
                        range.end += 2;
                    }
                    hide_label(source, &range, false, hidden, markers);
                    for face in &mut faces[range.clone()] {
                        face.link = true;
                    }
                    out.links.push(Link {
                        source: range,
                        destination: dest_url.to_string(),
                        target: None,
                    });
                }
                Event::Start(Tag::Image {
                    dest_url, title, ..
                }) => {
                    hide_label(source, &range, true, hidden, markers);
                    out.pictures.push(Picture {
                        source: range,
                        destination: dest_url.to_string(),
                        title: title.to_string(),
                    });
                }
                Event::Start(Tag::Strikethrough)
                    if !slice.starts_with("~~") && subscript_opener(source, range.start) =>
                {
                    for face in &mut faces[range] {
                        face.script = -1;
                    }
                }
                Event::Start(Tag::Superscript) => {
                    out.inline_styles.push(range.clone());
                    pair(hidden, markers, range.clone(), 1);
                    for face in &mut faces[range] {
                        face.script = 1;
                    }
                }
                Event::FootnoteReference(label) => {
                    let number = notes
                        .iter()
                        .position(|note| note == label.as_ref())
                        .unwrap()
                        + 1;
                    hidden[range.clone()].fill(true);
                    for face in &mut faces[range.clone()] {
                        face.script = 1;
                        face.link = true;
                    }
                    out.replacements.push(Replacement {
                        source: range.clone(),
                        text: number.to_string(),
                    });
                    out.links.push(Link {
                        source: range,
                        destination: format!("footnote:{label}"),
                        target: None,
                    });
                }
                Event::Start(Tag::FootnoteDefinition(label)) => {
                    let number = match notes.iter().position(|note| note == label.as_ref()) {
                        Some(index) => index + 1,
                        None => {
                            notes.push(label.to_string());
                            notes.len()
                        }
                    };
                    if let Some(end) = slice.find("]:") {
                        let end = range.start + end + 2;
                        let end = end
                            + source[end..]
                                .bytes()
                                .take_while(|b| matches!(b, b' ' | b'\t'))
                                .count();
                        hidden[range.start..end].fill(true);
                        out.footnotes.push((range.clone(), number, end));
                    }
                    for link in &mut out.links {
                        if link.destination == format!("footnote:{label}") {
                            link.target = Some(range.start);
                        }
                    }
                }
                Event::HardBreak => hidden[range].fill(true),
                Event::InlineHtml(tag) | Event::Html(tag) if !code => {
                    let tag = tag.trim();
                    if tag.starts_with("<!--")
                        && let Some(end) = source[range.start..].find("-->")
                    {
                        let comment = range.start..range.start + end + 3;
                        hidden[comment.clone()].fill(true);
                        out.metadata.push(comment.clone());
                        out.skipped.push(comment);
                    } else if tag == "<mark>" || tag == "<kbd>" {
                        html.push((tag.to_string(), range));
                    } else if tag == "</mark>" || tag == "</kbd>" {
                        let open = tag.replacen('/', "", 1);
                        if let Some(index) = html.iter().rposition(|(tag, _)| *tag == open) {
                            let (_, start) = html.remove(index);
                            hidden[start.clone()].fill(true);
                            hidden[range.clone()].fill(true);
                            for face in &mut faces[start.end..range.start] {
                                if open == "<mark>" {
                                    face.highlight = true;
                                } else {
                                    face.keyboard = true;
                                }
                            }
                            markers.extend([start, range]);
                        }
                    }
                }
                Event::InlineMath(_) | Event::DisplayMath(_) => {
                    let display = slice.starts_with("$$");
                    let count = if display { 2 } else { 1 };
                    pair(hidden, markers, range.clone(), count);
                    if display {
                        out.math.push(range.clone());
                        if slice.starts_with("$$\n") || slice.starts_with("$$\r\n") {
                            out.skipped.push(range.start..range.start + count);
                        }
                        if source[..range.end - count].ends_with('\n') {
                            out.skipped.push(range.end - count..range.end);
                        }
                    }
                    math(
                        source,
                        range.start + count..range.end - count,
                        hidden,
                        faces,
                        markers,
                        &mut out.replacements,
                    );
                }
                _ => {}
            }
        }
        for link in &mut out.links {
            if let Some(label) = link.destination.strip_prefix("footnote:")
                && let Some(number) = notes.iter().position(|note| note == label).map(|i| i + 1)
            {
                link.target = out
                    .footnotes
                    .iter()
                    .find(|(_, n, _)| *n == number)
                    .map(|(_, _, content)| *content);
            }
        }
        for table in &out.tables {
            let start = table.rows[0].end;
            let end = table.rows.get(1).map_or(table.source.end, |r| r.start);
            out.skipped.push(start..end);
        }
        // Intraword ~ is subscript; standalone ~ and all ~~ delimit strikethrough.
        for delimiter in ["==", "~", "^"] {
            let mut from = 0;
            while let Some(relative) = source[from..].find(delimiter) {
                let start = from + relative;
                from = start + delimiter.len();
                if !eligible[start]
                    || escaped(source, start)
                    || source[from..].starts_with(char::is_whitespace)
                    || (delimiter == "~"
                        && (!subscript_opener(source, start) || source[from..].starts_with('~')))
                {
                    continue;
                }
                let rest = source[from..].split('\n').next().unwrap_or("");
                if let Some(end) = rest.find(delimiter) {
                    let end = from + end;
                    if end == from
                        || !eligible[end]
                        || escaped(source, end)
                        || source[..end].ends_with(char::is_whitespace)
                        || (delimiter != "==" && source[from..end].contains(char::is_whitespace))
                    {
                        continue;
                    }
                    out.inline_styles.push(start..end + delimiter.len());
                    pair(
                        hidden,
                        markers,
                        start..end + delimiter.len(),
                        delimiter.len(),
                    );
                    for face in &mut faces[start..end] {
                        if delimiter == "==" {
                            face.highlight = true;
                        } else {
                            face.script = if delimiter == "~" { -1 } else { 1 };
                            face.strike = false;
                        }
                    }
                    from = end + delimiter.len();
                }
            }
        }
        out.replacements.sort_by_key(|r| r.source.start);
        out
    }
    pub fn skip_line(&self, range: &Range<usize>) -> bool {
        self.skipped
            .iter()
            .any(|r| r.start <= range.start && range.end <= r.end && range.start < r.end)
    }
    pub fn parts(&self, range: Range<usize>) -> Vec<Part> {
        let cells = self
            .cells
            .iter()
            .filter(|p| range.start <= p.source.start && p.source.end <= range.end)
            .cloned()
            .collect::<Vec<_>>();
        if !cells.is_empty() {
            return cells;
        }
        let mut parts = Vec::new();
        let mut start = range.start;
        for (index, picture) in self
            .pictures
            .iter()
            .enumerate()
            .filter(|(_, p)| range.start <= p.source.start && p.source.end <= range.end)
        {
            if start < picture.source.start {
                parts.push(Part {
                    source: start..picture.source.start,
                    cell: None,
                    picture: None,
                });
            }
            parts.push(Part {
                source: picture.source.clone(),
                cell: None,
                picture: Some(index),
            });
            start = picture.source.end;
        }
        if start < range.end || parts.is_empty() {
            parts.push(Part {
                source: start..range.end,
                cell: None,
                picture: None,
            });
        }
        parts
    }
}

pub(crate) fn subscript_opener(source: &str, start: usize) -> bool {
    source[..start]
        .chars()
        .next_back()
        .is_some_and(char::is_alphanumeric)
}
fn escaped(source: &str, at: usize) -> bool {
    source[..at]
        .bytes()
        .rev()
        .take_while(|b| *b == b'\\')
        .count()
        % 2
        == 1
}
fn pair(hidden: &mut [bool], markers: &mut Vec<Range<usize>>, range: Range<usize>, count: usize) {
    for r in [
        range.start..range.start + count,
        range.end - count..range.end,
    ] {
        hidden[r.clone()].fill(true);
        markers.push(r);
    }
}
fn hide_label(
    source: &str,
    range: &Range<usize>,
    image: bool,
    hidden: &mut [bool],
    markers: &mut Vec<Range<usize>>,
) {
    let slice = &source[range.clone()];
    if slice.starts_with('<') {
        pair(hidden, markers, range.clone(), 1);
        return;
    }
    let opening = if image { 2 } else { 1 };
    let mut depth = 1;
    for (i, ch) in slice[opening..].char_indices() {
        let at = range.start + opening + i;
        if escaped(source, at) {
            continue;
        }
        if ch == '[' {
            depth += 1;
        }
        if ch == ']' {
            depth -= 1;
        }
        if depth == 0 {
            for r in [range.start..range.start + opening, at..range.end] {
                hidden[r.clone()].fill(true);
                markers.push(r);
            }
            break;
        }
    }
}

/// Render basic TeX notation, preserving unknown commands as text.
fn math(
    source: &str,
    range: Range<usize>,
    hidden: &mut [bool],
    faces: &mut [Face],
    markers: &mut Vec<Range<usize>>,
    replacements: &mut Vec<Replacement>,
) {
    for (offset, ch) in source[range.clone()].char_indices() {
        for face in &mut faces[range.start + offset..range.start + offset + ch.len_utf8()] {
            face.italic = ch.is_alphabetic();
        }
    }
    let mut i = range.start;
    while i < range.end {
        let ch = source[i..].chars().next().unwrap();
        if matches!(ch, '^' | '_') {
            let start = i + 1;
            let (start, end, next) = if source[start..range.end].starts_with('{') {
                if let Some(end) = source[start + 1..range.end].find('}') {
                    let end = start + 1 + end;
                    hidden[start] = true;
                    hidden[end] = true;
                    (start + 1, end, end + 1)
                } else {
                    i += 1;
                    continue;
                }
            } else if let Some(next) = source[start..range.end].chars().next() {
                (start, start + next.len_utf8(), start + next.len_utf8())
            } else {
                i += 1;
                continue;
            };
            hidden[i] = true;
            markers.push(i..start);
            for face in &mut faces[start..end] {
                face.script = if ch == '^' { 1 } else { -1 };
            }
            if end < next {
                markers.push(end..next);
            }
            i = start;
        } else if ch == '\\' {
            let end = i
                + 1
                + source[i + 1..range.end]
                    .bytes()
                    .take_while(u8::is_ascii_alphabetic)
                    .count();
            let symbol = match &source[i + 1..end] {
                "alpha" => "α",
                "beta" => "β",
                "gamma" => "γ",
                "delta" => "δ",
                "theta" => "θ",
                "lambda" => "λ",
                "mu" => "μ",
                "pi" => "π",
                "sigma" => "σ",
                "phi" => "φ",
                "omega" => "ω",
                "Delta" => "Δ",
                "Sigma" => "Σ",
                "Omega" => "Ω",
                "times" => "×",
                "cdot" => "·",
                "pm" => "±",
                "leq" | "le" => "≤",
                "geq" | "ge" => "≥",
                "neq" | "ne" => "≠",
                "infty" => "∞",
                "to" | "rightarrow" => "→",
                "sum" => "∑",
                "int" => "∫",
                _ => "",
            };
            if !symbol.is_empty() {
                faces[i].italic = symbol.chars().next().is_some_and(char::is_alphabetic);
                hidden[i..end].fill(true);
                replacements.push(Replacement {
                    source: i..end,
                    text: symbol.to_string(),
                });
            }
            i = end.max(i + 1);
        } else {
            i += ch.len_utf8();
        }
    }
}
