# Lists and checkboxes

Try Tab, Shift+Tab, Enter, and Backspace at the start of an item. All changes can be undone. Click a checkbox or use ⌘Enter to change its status.

## Nested bullets

- Plan the editor
  - Writing experience
    - Clear typography
    - A calm layout
  - Keyboard navigation
    - Move into and out of formatting
- Build the application
  - Open and save Markdown
  - Keep independent tabs

## Numbered steps

1. Choose a project
   1. Give it a name
   1. Pick a folder
      - Keep related files together
      - Use a descriptive filename
1. Write a draft
   - Start with an outline
   - Expand each section with enough text to check that wrapped lines stay aligned with the item rather than its bullet or number.
1. Review and save

## Interactive checklist

- [ ] Write the first draft
  - [x] Add a title
  - [ ] Add **bold**, *italic*, and `inline code`
  - [ ] Review the nested lists
- [x] Keep the original Markdown editable
- [ ] Finish the document

## Other marker styles

* Asterisk parent
  + Plus-sign child
    - [X] Completed task using uppercase X

1) Parenthesis numbering
   1) Nested step
   2) Another nested step
2) Finish

## Continuations

- A longer item with a second source line.
  This line should align with the first line’s text.
  - A child underneath the continuation
- Press Shift+Enter to continue within this item.

## Literal Markdown in code

```markdown
- [ ] This is code, not a clickable checkbox.
  1. These markers should remain visible.
```
