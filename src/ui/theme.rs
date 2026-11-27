use std::rc::Rc;

use gpui_kit::component::{ActiveTheme, ColorName, Theme};
use gpui_kit::{App, Hsla, SharedString, px};

use crate::config::AppConfig;
use crate::model::AgentKind;

/// 应用界面字号和字体，并在切换浅色或深色主题时保留它们。
pub fn apply_ui_typography(config: &AppConfig, cx: &mut App) {
    let size = f32::from(config.effective_ui_font_size());
    let family = config
        .ui_font_family
        .as_ref()
        .filter(|family| cx.text_system().all_font_names().contains(family))
        .map(|family| SharedString::from(family.clone()))
        .unwrap_or_else(|| ".SystemUIFont".into());

    Theme::update(cx, |theme| {
        for appearance in [&mut theme.light_theme, &mut theme.dark_theme] {
            let appearance = Rc::make_mut(appearance);
            appearance.font_size = Some(size);
            appearance.font_family = Some(family.clone());
        }
        theme.font_size = px(size);
        theme.font_family = family;
    });
}

/// Maps each supported AI agent to a semantic GPUI Kit color scale.
pub fn agent_color_name(agent: AgentKind) -> ColorName {
    match agent {
        AgentKind::ClaudeCode => ColorName::Orange,
        AgentKind::Codex => ColorName::Emerald,
        AgentKind::GeminiCli => ColorName::Blue,
        AgentKind::OpenCode => ColorName::Violet,
        AgentKind::OpenChamber => ColorName::Fuchsia,
        AgentKind::Zed => ColorName::Sky,
        AgentKind::Cursor => ColorName::Cyan,
        AgentKind::Windsurf => ColorName::Teal,
        AgentKind::VsCode => ColorName::Indigo,
        AgentKind::Trae => ColorName::Pink,
        AgentKind::Kiro => ColorName::Amber,
        AgentKind::Antigravity => ColorName::Purple,
        AgentKind::Cline => ColorName::Yellow,
        AgentKind::RooCode => ColorName::Rose,
        AgentKind::IntelliJIdea => ColorName::Rose,
        AgentKind::PyCharm => ColorName::Emerald,
        AgentKind::WebStorm => ColorName::Cyan,
        AgentKind::GoLand => ColorName::Purple,
        AgentKind::CLion => ColorName::Teal,
        AgentKind::PhpStorm => ColorName::Violet,
        AgentKind::Rider => ColorName::Fuchsia,
        AgentKind::RustRover => ColorName::Orange,
        AgentKind::RubyMine => ColorName::Pink,
    }
}

/// Returns a theme-aware accent color for an AI agent.
pub fn agent_accent(agent: AgentKind, cx: &App) -> Hsla {
    let color = agent_color_name(agent);
    if cx.theme().is_dark() {
        color.scale(400)
    } else {
        color.scale(600)
    }
}

/// Returns a theme-aware heatmap cell color for intensity levels 0..=4.
pub fn heat_level_color(level: u8, cx: &App) -> Hsla {
    let is_dark = cx.theme().is_dark();
    match level {
        0 => cx.theme().muted,
        1 => {
            if is_dark {
                ColorName::Blue.scale(900)
            } else {
                ColorName::Blue.scale(200)
            }
        }
        2 => {
            if is_dark {
                ColorName::Blue.scale(700)
            } else {
                ColorName::Blue.scale(400)
            }
        }
        3 => {
            if is_dark {
                ColorName::Blue.scale(500)
            } else {
                ColorName::Blue.scale(500)
            }
        }
        _ => {
            if is_dark {
                ColorName::Blue.scale(400)
            } else {
                ColorName::Blue.scale(600)
            }
        }
    }
}
