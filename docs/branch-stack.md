# Branch stack

| Branch                                                                                           | Base                         | Scope                                                                                     |
| ------------------------------------------------------------------------------------------------ | ---------------------------- | ----------------------------------------------------------------------------------------- |
| [core/gpui-bootstrap](https://github.com/peterdpong/type/tree/core/gpui-bootstrap)               | `main`                       | Rust workspace, native GPUI text input, system fonts, formatting tools                    |
| [editor/markdown-core](https://github.com/peterdpong/type/tree/editor/markdown-core)             | `core/gpui-bootstrap`        | Multiline WYSIWYG editing, headings, inline styles, lists, code, Unicode input and undo   |
| [ui/editor-tools](https://github.com/peterdpong/type/tree/ui/editor-tools)                       | `editor/markdown-core`       | Selection toolbelt, collapsible outline, find dropdown and document previews              |
| [editor/markdown-extensions](https://github.com/peterdpong/type/tree/editor/markdown-extensions) | `ui/editor-tools`            | Tables, images, references, math and other Markdown extensions; caret and selection fixes |
| [app/files](https://github.com/peterdpong/type/tree/app/files)                                   | `editor/markdown-extensions` | Native file menu, open/save, safe writes, unsaved-change prompts and notifications        |
| [ui/file-tabs](https://github.com/peterdpong/type/tree/ui/file-tabs)                             | `app/files`                  | Independent file tabs, overflow, multi-file lifecycle and Control-Tab previews            |
| [ui/tab-renaming](https://github.com/peterdpong/type/tree/ui/tab-renaming)                       | `ui/file-tabs`               | Inline tab naming and safe filesystem renames                                             |
| [ui/tab-motion](https://github.com/peterdpong/type/tree/ui/tab-motion)                           | `ui/tab-renaming`            | Animated compact switcher, tab animation/reordering and native macOS drag handling        |

Review each branch against its base and merge in the listed order.

Run the complete app:

```sh
git switch ui/tab-motion
cargo run -p type-app
```

## Optional tooling

| Branch                                                                           | Base                  | Scope              |
| -------------------------------------------------------------------------------- | --------------------- | ------------------ |
| [chore/macos-bundle](https://github.com/peterdpong/type/tree/chore/macos-bundle) | `core/gpui-bootstrap` | macOS app bundling |
