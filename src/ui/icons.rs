use std::borrow::Cow;

use gpui_kit::assets::icon_assets;
use gpui_kit::component::{Icon, IconNamed};
use gpui_kit::{App, AssetSource, Result, SharedString, Styled};

use crate::app_icon::{APP_ICON_DARK_PNG, APP_ICON_LIGHT_PNG};
use crate::model::AgentKind;
use crate::ui::theme::agent_accent;

icon_assets!(ExtraLucideAssets, [Pencil]);

pub const APP_ICON_LIGHT_ASSET_PATH: &str = "icons/app/app-icon-light.png";
pub const APP_ICON_DARK_ASSET_PATH: &str = "icons/app/app-icon-dark.png";

/// Returns the bundled asset path of the application icon for the given theme mode.
pub const fn app_icon_asset_path(is_dark: bool) -> &'static str {
    if is_dark {
        APP_ICON_DARK_ASSET_PATH
    } else {
        APP_ICON_LIGHT_ASSET_PATH
    }
}

const APP_ICON_ASSETS: &[(&str, &[u8])] = &[
    (APP_ICON_LIGHT_ASSET_PATH, APP_ICON_LIGHT_PNG),
    (APP_ICON_DARK_ASSET_PATH, APP_ICON_DARK_PNG),
];

const AGENT_ICON_ASSETS: &[(&str, &[u8])] = &[
    (
        "icons/agents/claude-code.svg",
        include_bytes!("../../assets/icons/agents/claude-code.svg"),
    ),
    (
        "icons/agents/codex.svg",
        include_bytes!("../../assets/icons/agents/codex.svg"),
    ),
    (
        "icons/agents/gemini-cli.svg",
        include_bytes!("../../assets/icons/agents/gemini-cli.svg"),
    ),
    (
        "icons/agents/opencode.svg",
        include_bytes!("../../assets/icons/agents/opencode.svg"),
    ),
    (
        "icons/agents/openchamber.svg",
        include_bytes!("../../assets/icons/agents/openchamber.svg"),
    ),
    (
        "icons/agents/zed.svg",
        include_bytes!("../../assets/icons/agents/zed.svg"),
    ),
    (
        "icons/agents/cursor.svg",
        include_bytes!("../../assets/icons/agents/cursor.svg"),
    ),
    (
        "icons/agents/windsurf.svg",
        include_bytes!("../../assets/icons/agents/windsurf.svg"),
    ),
    (
        "icons/agents/vscode.svg",
        include_bytes!("../../assets/icons/agents/vscode.svg"),
    ),
    (
        "icons/agents/trae.svg",
        include_bytes!("../../assets/icons/agents/trae.svg"),
    ),
    (
        "icons/agents/kiro.svg",
        include_bytes!("../../assets/icons/agents/kiro.svg"),
    ),
    (
        "icons/agents/antigravity.svg",
        include_bytes!("../../assets/icons/agents/antigravity.svg"),
    ),
    (
        "icons/agents/cline.svg",
        include_bytes!("../../assets/icons/agents/cline.svg"),
    ),
    (
        "icons/agents/roo-code.svg",
        include_bytes!("../../assets/icons/agents/roo-code.svg"),
    ),
    (
        "icons/agents/intellij-idea.svg",
        include_bytes!("../../assets/icons/agents/intellij-idea.svg"),
    ),
    (
        "icons/agents/pycharm.svg",
        include_bytes!("../../assets/icons/agents/pycharm.svg"),
    ),
    (
        "icons/agents/webstorm.svg",
        include_bytes!("../../assets/icons/agents/webstorm.svg"),
    ),
    (
        "icons/agents/goland.svg",
        include_bytes!("../../assets/icons/agents/goland.svg"),
    ),
    (
        "icons/agents/clion.svg",
        include_bytes!("../../assets/icons/agents/clion.svg"),
    ),
    (
        "icons/agents/phpstorm.svg",
        include_bytes!("../../assets/icons/agents/phpstorm.svg"),
    ),
    (
        "icons/agents/rider.svg",
        include_bytes!("../../assets/icons/agents/rider.svg"),
    ),
    (
        "icons/agents/rustrover.svg",
        include_bytes!("../../assets/icons/agents/rustrover.svg"),
    ),
    (
        "icons/agents/rubymine.svg",
        include_bytes!("../../assets/icons/agents/rubymine.svg"),
    ),
];

/// Composite asset source serving bundled agent brand icons alongside default GPUI Kit icons.
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = APP_ICON_ASSETS.iter().find(|(p, _)| *p == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if let Some((_, bytes)) = AGENT_ICON_ASSETS.iter().find(|(p, _)| *p == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if let Some(bytes) = ExtraLucideAssets.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut items = gpui_kit::assets::Assets.list(path)?;
        items.extend(ExtraLucideAssets.list(path)?);
        items.extend(
            APP_ICON_ASSETS
                .iter()
                .chain(AGENT_ICON_ASSETS.iter())
                .filter(|(p, _)| p.starts_with(path))
                .map(|(p, _)| SharedString::from(*p)),
        );
        Ok(items)
    }
}

/// Maps each supported [`AgentKind`] to its bundled SVG asset path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentIcon(pub AgentKind);

impl AgentIcon {
    pub const fn asset_path(agent: AgentKind) -> &'static str {
        match agent {
            AgentKind::ClaudeCode => "icons/agents/claude-code.svg",
            AgentKind::Codex => "icons/agents/codex.svg",
            AgentKind::GeminiCli => "icons/agents/gemini-cli.svg",
            AgentKind::OpenCode => "icons/agents/opencode.svg",
            AgentKind::OpenChamber => "icons/agents/openchamber.svg",
            AgentKind::Zed => "icons/agents/zed.svg",
            AgentKind::Cursor => "icons/agents/cursor.svg",
            AgentKind::Windsurf => "icons/agents/windsurf.svg",
            AgentKind::VsCode => "icons/agents/vscode.svg",
            AgentKind::Trae => "icons/agents/trae.svg",
            AgentKind::Kiro => "icons/agents/kiro.svg",
            AgentKind::Antigravity => "icons/agents/antigravity.svg",
            AgentKind::Cline => "icons/agents/cline.svg",
            AgentKind::RooCode => "icons/agents/roo-code.svg",
            AgentKind::IntelliJIdea => "icons/agents/intellij-idea.svg",
            AgentKind::PyCharm => "icons/agents/pycharm.svg",
            AgentKind::WebStorm => "icons/agents/webstorm.svg",
            AgentKind::GoLand => "icons/agents/goland.svg",
            AgentKind::CLion => "icons/agents/clion.svg",
            AgentKind::PhpStorm => "icons/agents/phpstorm.svg",
            AgentKind::Rider => "icons/agents/rider.svg",
            AgentKind::RustRover => "icons/agents/rustrover.svg",
            AgentKind::RubyMine => "icons/agents/rubymine.svg",
        }
    }
}

impl IconNamed for AgentIcon {
    fn path(self) -> SharedString {
        Self::asset_path(self.0).into()
    }
}

/// Builds a theme-tinted [`Icon`] for the given AI agent.
pub fn agent_icon(agent: AgentKind, cx: &App) -> Icon {
    Icon::new(AgentIcon(agent)).text_color(agent_accent(agent, cx))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_AGENTS: [AgentKind; 23] = [
        AgentKind::ClaudeCode,
        AgentKind::Codex,
        AgentKind::GeminiCli,
        AgentKind::OpenCode,
        AgentKind::OpenChamber,
        AgentKind::Zed,
        AgentKind::Cursor,
        AgentKind::Windsurf,
        AgentKind::VsCode,
        AgentKind::Trae,
        AgentKind::Kiro,
        AgentKind::Antigravity,
        AgentKind::Cline,
        AgentKind::RooCode,
        AgentKind::IntelliJIdea,
        AgentKind::PyCharm,
        AgentKind::WebStorm,
        AgentKind::GoLand,
        AgentKind::CLion,
        AgentKind::PhpStorm,
        AgentKind::Rider,
        AgentKind::RustRover,
        AgentKind::RubyMine,
    ];

    #[test]
    fn all_agent_icons_are_embedded_and_rasterizable_by_gpui() {
        use std::sync::Arc;
        use gpui_kit::{DevicePixels, Size, SvgRenderer, SvgSize};

        let renderer = SvgRenderer::new(Arc::new(AppAssets));
        let target_size = SvgSize::Size(Size::new(DevicePixels(32), DevicePixels(32)));

        for agent in ALL_AGENTS {
            let path = AgentIcon::asset_path(agent);
            let data = AppAssets
                .load(path)
                .expect("asset load should not fail")
                .expect("agent icon asset should exist");
            let parsed = renderer
                .parse_svg(&data)
                .unwrap_or_else(|err| panic!("failed to parse {path}: {err}"));
            let image = renderer
                .render_parsed(&parsed, target_size)
                .unwrap_or_else(|err| panic!("failed to rasterize {path}: {err}"));
            let bytes = image.as_bytes(0).expect("rendered image frame should exist");
            let total_pixels = bytes.len() / 4;
            let visible_pixels = bytes.chunks_exact(4).filter(|px| px[3] > 0).count();
            assert!(
                visible_pixels > total_pixels / 20 && visible_pixels < total_pixels * 95 / 100,
                "{path} rendered with unexpected alpha coverage: {visible_pixels}/{total_pixels}"
            );
        }
    }

    #[test]
    fn app_assets_resolves_light_and_dark_app_icons() {
        for is_dark in [false, true] {
            let path = app_icon_asset_path(is_dark);
            let data = AppAssets
                .load(path)
                .expect("app icon lookup should succeed")
                .expect("app icon asset should exist");
            assert!(data.starts_with(b"\x89PNG\r\n\x1a\n"));
        }
    }

    #[test]
    fn app_assets_falls_back_to_default_kit_icons() {
        let icon = AppAssets
            .load("icons/bot.svg")
            .expect("default asset lookup should succeed");
        assert!(icon.is_some(), "default GPUI Kit icon should still resolve");

        let pencil = AppAssets
            .load(gpui_kit::assets::IconName::Pencil.path().as_ref())
            .expect("extra Lucide icon lookup should succeed");
        assert!(pencil.is_some(), "embedded Pencil icon should resolve");
    }
}
