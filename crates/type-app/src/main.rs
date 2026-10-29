use gpui::{prelude::*, *};
use markdown_core::{MarkdownEditor, appearance};
actions!(type_app, [Quit]);
struct Workspace {
    input: Entity<MarkdownEditor>,
}
impl Render for Workspace {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(appearance::BACKGROUND))
            .font_family(appearance::FONT_FAMILY)
            .on_action(|_: &Quit, _, cx| cx.quit())
            .child(self.input.clone())
    }
}
fn main() {
    Application::new().run(|cx: &mut App| {
        markdown_core::init(cx);
        cx.bind_keys([KeyBinding::new(
            if cfg!(target_os = "macos") {
                "cmd-q"
            } else {
                "ctrl-q"
            },
            Quit,
            None,
        )]);
        cx.set_menus(vec![Menu {
            name: "Type".into(),
            items: vec![MenuItem::action("Quit Type", Quit)],
        }]);
        let bounds = Bounds::centered(None, size(px(960.), px(780.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Type".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let input = cx.new(|cx| MarkdownEditor::new("", cx));
                window.focus(&input.focus_handle(cx));
                cx.new(|_| Workspace { input })
            },
        )
        .expect("Could not open Type window");
        cx.activate(true);
    });
}
