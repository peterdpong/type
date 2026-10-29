mod files;
mod native_header;
mod notice;
mod workspace;

use gpui::{prelude::*, *};
use std::path::PathBuf;
use workspace::Workspace;

actions!(
    type_app,
    [
        New,
        Open,
        Save,
        SaveAs,
        Rename,
        Close,
        CloseWindow,
        Quit,
        NextTab,
        PreviousTab,
        SwitchNextTab,
        SwitchPreviousTab
    ]
);

fn init(cx: &mut App) {
    markdown_core::init(cx);
    let command = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.bind_keys([
        KeyBinding::new("f2", Rename, None),
        KeyBinding::new(&format!("{command}-n"), New, None),
        KeyBinding::new(&format!("{command}-t"), New, None),
        KeyBinding::new(&format!("{command}-o"), Open, None),
        KeyBinding::new(&format!("{command}-s"), Save, None),
        KeyBinding::new(&format!("{command}-shift-s"), SaveAs, None),
        KeyBinding::new(&format!("{command}-w"), Close, None),
        KeyBinding::new(&format!("{command}-shift-w"), CloseWindow, None),
        KeyBinding::new(&format!("{command}-q"), Quit, None),
        KeyBinding::new("ctrl-tab", SwitchNextTab, None),
        KeyBinding::new("ctrl-shift-tab", SwitchPreviousTab, None),
        KeyBinding::new(&format!("{command}-shift-]"), NextTab, None),
        KeyBinding::new(&format!("{command}-shift-["), PreviousTab, None),
    ]);
    cx.set_menus(vec![
        Menu {
            name: "Type".into(),
            items: vec![MenuItem::action("Quit Type", Quit)],
        },
        Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action("New Tab", New),
                MenuItem::action("Open…", Open),
                MenuItem::separator(),
                MenuItem::action("Save", Save),
                MenuItem::action("Save As…", SaveAs),
                MenuItem::action("Rename…", Rename),
                MenuItem::separator(),
                MenuItem::action("Close Tab", Close),
                MenuItem::action("Close Window", CloseWindow),
            ],
        },
        Menu {
            name: "Edit".into(),
            items: vec![
                MenuItem::action("Undo", markdown_core::Undo),
                MenuItem::action("Redo", markdown_core::Redo),
                MenuItem::separator(),
                MenuItem::action("Cut", markdown_core::Cut),
                MenuItem::action("Copy", markdown_core::Copy),
                MenuItem::action("Paste", markdown_core::Paste),
                MenuItem::action("Select All", markdown_core::SelectAll),
                MenuItem::separator(),
                MenuItem::action("Find…", markdown_core::Find),
                MenuItem::action("Find Next", markdown_core::FindNext),
                MenuItem::action("Find Previous", markdown_core::FindPrevious),
            ],
        },
        Menu {
            name: "Format".into(),
            items: vec![
                MenuItem::action("Bold", markdown_core::Bold),
                MenuItem::action("Italic", markdown_core::Italic),
                MenuItem::action("Inline Code", markdown_core::InlineCode),
                MenuItem::separator(),
                MenuItem::action("Heading 1", markdown_core::Heading1),
                MenuItem::action("Heading 2", markdown_core::Heading2),
                MenuItem::action("Heading 3", markdown_core::Heading3),
                MenuItem::action("Heading 4", markdown_core::Heading4),
                MenuItem::action("Heading 5", markdown_core::Heading5),
                MenuItem::action("Heading 6", markdown_core::Heading6),
                MenuItem::separator(),
                MenuItem::action("Bullet List", markdown_core::BulletList),
                MenuItem::action("Numbered List", markdown_core::OrderedList),
                MenuItem::action("Checklist", markdown_core::TaskList),
                MenuItem::action("Toggle Checkbox", markdown_core::ToggleTask),
                MenuItem::action("Quote", markdown_core::Quote),
            ],
        },
        Menu {
            name: "Window".into(),
            items: vec![
                MenuItem::action("Next Tab", NextTab),
                MenuItem::action("Previous Tab", PreviousTab),
            ],
        },
    ]);
}

fn main() {
    let paths = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    Application::new().run(move |cx: &mut App| {
        init(cx);
        let bounds = Bounds::centered(None, size(px(960.), px(780.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(560.), px(400.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Type".into()),
                    appears_transparent: cfg!(target_os = "macos"),
                    traffic_light_position: cfg!(target_os = "macos").then_some(point(
                        px(14.),
                        px((markdown_core::appearance::TOP_GUTTER - 14.) / 2.),
                    )),
                }),
                ..Default::default()
            },
            move |window, cx| {
                let view = cx.new(|cx| Workspace::new(window, cx));
                if !paths.is_empty() {
                    view.update(cx, |this, cx| this.open_paths(paths, window, cx));
                }
                view
            },
        )
        .expect("Could not open Type window");
        cx.activate(true);
    });
}
