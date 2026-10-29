# Extended Markdown

## Tables

| Style | Sample | Alignment |
| :--- | :---: | ---: |
| Bold | **Text** | 1 |
| Code | `value` | 2 |

Use Tab and Shift-Tab to move between cells. Enter moves to the next row; Tab from the last cell adds a row. Command-Enter leaves the table.

## Images

![A local image](./assets/sample.svg "Local SVG image")

![Placeholder image; no image asset is supplied](./placeholder.png)

## Reference links

A [reference-style link][example-reference]. Command-click a link to open it.

[example-reference]: https://example.com "Reference title"

## Setext headings

An underlined level-one heading
==============================

An underlined level-two heading
------------------------------

## Nested quotes

> Outer quotation
> > Nested quotation
> > > One more level
> Back to the outer quotation

## Breaks and autolinks

This line ends with two spaces.  
This line follows a Markdown hard break.

This line uses a backslash.\
This line follows another hard break.

An autolink: <https://example.com>.

## HTML and extensions

<mark>HTML highlighting</mark> and <kbd>⌘S</kbd>.

<!-- An HTML comment used to check source preservation. -->

A footnote reference[^note], ==highlight syntax==, H~2~O, and x^2^.

Standalone ~strikethrough~ and ~~double-tilde strikethrough~~ still work.

[^note]: A footnote definition. Command-click its reference to jump here.

## Math

Inline math: $E = mc^2$.

$$
a^2 + b^2 = c^2
$$

Basic symbols and subscripts: $\alpha + x_{12} \leq \pi$.

## Source preservation

All definitions, comments, delimiters, destinations, and formula source remain intact when saved.
