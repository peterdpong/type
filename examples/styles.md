# Markdown style playground

A document for checking typography, spacing, selection, caret movement, and Markdown editing. Everything below is editable. The final section includes syntax the editor does not fully render yet.

## Heading hierarchy

Compare the six levels below. Higher levels should stand out through size, weight, and spacing; body text should remain visually distinct.

# H1 — A document title

Body text beneath the largest heading.

## H2 — A major section

Body text beneath a section heading.

### H3 — A subsection

Body text beneath a subsection heading.

#### H4 — A specific topic

Body text beneath a smaller heading.

##### H5 — Supporting detail

Body text beneath a compact heading.

###### H6 — A quiet label

Body text beneath the smallest, muted heading.

### A heading with **bold**, *italic*, and `inline code`

The heading style should combine with the inline styles.

## A longer heading that should wrap naturally in a narrow window while keeping a readable line height and a correctly sized caret

Resize the window to compare wrapped headings with wrapped body text.

### A heading followed immediately by body text
This paragraph has no empty source line above it. Press Enter at the end of the heading and try typing a new paragraph.

## Paragraphs and wrapping

This is a short paragraph.

This paragraph is intentionally longer. It should wrap cleanly within the writing column, with comfortable line spacing and no horizontal scrolling. Place the caret near the end of a visual row and use the arrow keys to move across the wrap. Select part of the paragraph across two or more rows, then resize the window and check that the selection still follows the same text.

These two source lines are adjacent.
There is no empty source line between them.

There is one empty source line before this paragraph.


There are two empty source lines before this paragraph.

## Inline styles

Plain text before **bold text** and plain text after it.

Plain text before __bold using underscores__ and plain text after it.

Plain text before *italic text* and plain text after it.

Plain text before _italic using underscores_ and plain text after it.

Plain text before ***bold and italic*** and plain text after it.

Plain text before ~~strikethrough~~ and plain text after it.

Single tilde form: ~strikethrough~, ~a~, and ~é🦀~ should keep every character visible.

Escaped punctuation: \*literal asterisks\*, \_literal underscores\_, and \# a literal hash.

Type `**text**` one character at a time, then Undo to reject its styling and restore all the visible asterisks. Redo should restore bold. The literal choice should survive saving and reopening.

Nested emphasis: **bold with an *italic phrase* inside**, followed by ordinary text.

Combined emphasis: ~~**bold and struck through**~~, then plain text.

Mixed styles: **bold**, *italic*, `inline code`, and ~~removed text~~ in one sentence.

Adjacent styles: **bold***italic*`code` followed by normal text.

## Inline code

Before `inline code` after.

Use `let answer = 42;` inside a normal sentence.

Paths and punctuation: `/Users/example/notes.md`, `foo_bar`, `a ** b`, and `# literal hashes`.

Double backticks allow a backtick inside code: ``use `code` here``.

An inline code span can be long enough to wrap across several visual rows: `this_is_a_long_identifier_with_many_parts_and_more_parts_to_exercise_wrapping_in_a_narrow_editor_window_without_losing_the_inline_background_or_selection_highlight`.

Place the caret just before or after an inline code span. Left and Right should enter and leave its formatting without inserting extra characters.

## Links

An [example link](https://example.com) inside a paragraph.

A [link with a title](https://example.com "Example title") inside a paragraph.

A [relative link to the welcome document](./welcome.md).

A [**bold link label**](https://example.com) and an [*italic link label*](https://example.com).

These examples check label styling and preservation of the Markdown destination.

## Lists

### Bullets with different markers

- First bullet
- Second bullet with **bold text**
- Third bullet with *italic text* and `inline code`

* Asterisk bullet
* Another asterisk bullet

+ Plus-sign bullet
+ Another plus-sign bullet

### Numbered lists

1. First numbered item
2. Second numbered item with a [link](https://example.com)
3. Third numbered item with ~~removed text~~

8. A list beginning at eight
9. A single-digit marker
10. A double-digit marker
11. A longer item that should wrap onto another visual row in a narrow window while keeping its text aligned with the other items.

1) An item using a closing parenthesis
2) Another item using a closing parenthesis

### Checkboxes

- [ ] An incomplete task
- [x] A completed task
- [X] A completed task with an uppercase marker
- [ ] A task with **emphasis** and `inline code`

### Nested and mixed lists

- Parent bullet
  - Child bullet
    - Grandchild bullet
  - Sibling child with **bold** and `code`
- Another parent
  1. Numbered child
  2. Another numbered child
     - A nested bullet inside a numbered item
     - [ ] A nested task
     - [x] A completed nested task

1. First step
   - A supporting detail
   - Another supporting detail
2. Second step
   1. A numbered substep
   2. A substep with a continuation paragraph
      This source line should align with the substep text.

Click a checkbox to toggle it. Use Tab and Shift+Tab to move an item with its children. Enter continues a list; Enter on an empty item steps out one level. Shift+Enter continues within the same item.

## Quotes

> A short quotation.

> A quotation spanning several source lines.
> This line contains **bold**, *italic*, and `inline code`.
>
> This is another paragraph within the same quote.

This paragraph is outside the quote.

> This longer quotation should wrap naturally when the window is narrow. Its text, left border, inline styles, selection highlight, and caret should stay aligned across every visual row.

Place the caret on the last visual row of a quote and press Down to move into plain text beneath it.

## Horizontal rules

A rule made from hyphens:

---

A rule made from asterisks:

***

A rule made from underscores:

___

Each rule should separate the surrounding paragraphs.

## Code blocks

Language labels below exercise fence parsing; syntax coloring and dedicated language-label controls are not implemented yet.

### Plain fenced code

```
Plain code, with no language label.
**These asterisks stay literal.**
# This is not a heading or an outline entry.
[This is not a link](https://example.com).

There is an empty row above this one.
```

Plain text after the code block.

### Rust with indentation

```rust
fn main() {
    let message = "Hello, Markdown 👋";
    if !message.is_empty() {
        println!("{message}");
    }
}
```

### JSON

```json
{
  "title": "A small document",
  "saved": true,
  "tags": ["writing", "markdown", "测试"]
}
```

### Tilde fences and literal tabs

~~~text
Column one	Column two	Column three
alpha	beta	gamma

Tabs should advance to consistent four-column stops.
~~~

### Backticks inside a longer fence

````markdown
```rust
let nested_fence = true;
```
````

### A long code row

```text
const message = "This deliberately long line should exercise wrapping inside the code panel without losing the background, padding, selection highlight, or correct caret position.";
```

### Indented code

    first line of indented code
    second line of indented code
        a further level of indentation

Plain text after the indented block.

### An empty code block

```
```

Click inside the empty block and type immediately. On the final visual row of a code block, press Down to leave it and continue in plain text.

## Unicode and punctuation

Accents: café, naïve, résumé, São Paulo, Ångström.

Combining marks: café and café should look alike.

Emoji: 👋 🦀 ✨ 👨‍👩‍👧‍👦 👍🏽 🇨🇦.

Chinese: 这是一段用于测试字体和光标的文字。

Japanese: 見出し、本文、コードを確認します。

Korean: 제목과 본문을 확인합니다.

Punctuation: “curly quotes”, ‘single quotes’, apostrophes, ellipses…, en–dashes, and em—dashes.

Formatted Unicode: **café 👋**, *日本語*, `résumé_测试`, and ~~旧的文字~~.

Move Left and Right through the emoji and combining marks. Each complete character should remain intact when selected or deleted.

## Selection and editing checks

- [ ] Select a single word in plain text and inspect the floating toolbelt.
- [ ] Select across plain text, **bold text**, *italic text*, and `inline code` in this line.
- [ ] Select a long wrapped paragraph, then resize the window.
- [ ] Select several code rows, including an empty row, and check the highlight.
- [ ] Toggle bold or italic without a selection, then type.
- [ ] Move into and out of inline code with Left and Right.
- [ ] Leave a quote or code block with Down.
- [ ] Use the outline to jump between heading levels.
- [ ] Switch tabs and return; check the selection, scroll position, and undo history.
- [ ] Save an edit, undo it, and check the unsaved indicator.

## Extended Markdown syntax

These examples exercise tables, images, references, headings, quotes, breaks, inline HTML, footnotes, scripts, and basic math. The image below is intentionally missing; it should show a clear placeholder. See `extensions.md` for a local image and table editing instructions.

### Tables

| Style | Sample | Alignment |
| :--- | :---: | ---: |
| Bold | **Text** | 1 |
| Code | `value` | 2 |

### Images

![Placeholder image; no image asset is supplied](./placeholder.png)

### Reference links

A [reference-style link][example-reference].

[example-reference]: https://example.com "Reference title"

### Setext headings

An underlined level-one heading
==============================

An underlined level-two heading
------------------------------

### Nested quotes

> Outer quotation
> > Nested quotation

### Breaks and autolinks

This line ends with two spaces.  
This line follows a Markdown hard break.

An autolink: <https://example.com>.

### HTML and extensions

<mark>HTML highlighting</mark> and <kbd>⌘S</kbd>.

<!-- An HTML comment used to check source preservation. -->

A footnote reference[^note], ==highlight syntax==, H~2~O, and x^2^.

[^note]: A footnote definition.

Inline math: $E = mc^2$.

$$
a^2 + b^2 = c^2
$$

## End of playground

All examples above should remain intact in the saved Markdown source, including hidden comments and reference definitions.
