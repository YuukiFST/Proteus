//! Proteus desktop entry point (T0 surface, PRD §6/§7).
//!
//! UI glue only: opens the gpui window, instantiates `gpui-component`, and
//! mounts the app shell. Every tool is one view in `views/`, all business
//! logic lives in `proteus-core` (PRD §6 hard separation: this crate must be
//! the ONLY place gpui/gpui-component appear).

mod views;

use gpui::*;

fn main() {
    let app = Application::new();

    app.run(move |cx| {
        // Required before any gpui-component feature is used.
        gpui_component::init(cx);

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1200.), px(800.)), cx)),
            titlebar: Some(TitlebarOptions {
                title: Some("Proteus".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| views::AppView::new(window, cx));
                // The first level on the window must be a gpui-component Root.
                cx.new(|cx| gpui_component::Root::new(view, window, cx))
            })?;

            Ok::<_, anyhow::Error>(())
        })
        .detach();
    });
}

#[cfg(test)]
mod tests {
    use crate::views::Tool;

    /// T0 smoke test (PRD §7 tier table): every PRD §9 tool has exactly one
    /// view registered with a unique title, so the sidebar can reach all 19.
    #[test]
    fn every_prd_tool_has_a_unique_view() {
        let mut titles = Vec::new();
        for tool in Tool::ALL {
            assert!(!tool.title().is_empty(), "every tool needs a title");
            assert!(!tool.group().is_empty(), "every tool needs a group");
            titles.push(tool.title().to_string());
        }
        let mut sorted = titles.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), titles.len(), "titles must be unique");
        assert_eq!(Tool::ALL.len(), 19, "PRD §9 lists 13 PDF + 6 image tools");
    }
}