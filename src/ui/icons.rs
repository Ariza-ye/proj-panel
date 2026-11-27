use std::borrow::Cow;

use gpui_kit::assets::icon_assets;
use gpui_kit::component::{Icon, IconName, IconNamed};
use gpui_kit::{App, AssetSource, IntoElement, ParentElement, Result, SharedString, Styled, div, px, rgb, svg};

use crate::launcher::LauncherKind;
use crate::model::AgentKind;
use crate::ui::theme::agent_accent;

icon_assets!(ExtraLucideAssets, [Pencil, ExternalLink, LogOut]);

pub const APP_ICON_GLYPH_ASSET_PATH: &str = "icons/app/app-icon-glyph.svg";

const APP_ICON_ASSETS: &[(&str, &[u8])] = &[(
    APP_ICON_GLYPH_ASSET_PATH,
    include_bytes!("../../assets/icons/app/app-icon-glyph.svg"),
)];

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

/// Builds an [`Icon`] for an external launcher, reusing linked AI agent brand icons when available.
pub fn launcher_icon(kind: LauncherKind, cx: &App) -> Icon {
    match kind.linked_agent() {
        Some(agent) => agent_icon(agent, cx),
        None => Icon::new(IconName::SquareTerminal),
    }
}

/// Renders the vector ProjPanel brand icon used in window and popover headers.
pub fn render_app_brand_icon(is_dark: bool) -> impl IntoElement {
    let (bg_color, border_color, glyph_color) = if is_dark {
        (rgb(0x161f32), rgb(0x2d3f60), rgb(0x3b8eff))
    } else {
        (rgb(0xf8fafd), rgb(0xd8e2f0), rgb(0x0b63f6))
    };

    div()
        .size(px(20.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.0))
        .bg(bg_color)
        .border_1()
        .border_color(border_color)
        .child(
            svg()
                .path(APP_ICON_GLYPH_ASSET_PATH)
                .size(px(15.0))
                .text_color(glyph_color),
        )
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
    fn app_assets_resolves_and_rasterizes_app_icon_glyph() {
        use std::sync::Arc;
        use gpui_kit::{DevicePixels, Size, SvgRenderer, SvgSize};

        let data = AppAssets
            .load(APP_ICON_GLYPH_ASSET_PATH)
            .expect("app icon lookup should succeed")
            .expect("app icon glyph asset should exist");
        let renderer = SvgRenderer::new(Arc::new(AppAssets));
        let parsed = renderer
            .parse_svg(&data)
            .expect("app icon glyph SVG should parse");
        let image = renderer
            .render_parsed(
                &parsed,
                SvgSize::Size(Size::new(DevicePixels(32), DevicePixels(32))),
            )
            .expect("app icon glyph SVG should rasterize");
        let bytes = image.as_bytes(0).expect("rendered image frame should exist");
        let total_pixels = bytes.len() / 4;
        let visible_pixels = bytes.chunks_exact(4).filter(|px| px[3] > 0).count();
        assert!(
            visible_pixels > total_pixels / 20 && visible_pixels < total_pixels * 95 / 100,
            "app icon glyph rendered with unexpected alpha coverage: {visible_pixels}/{total_pixels}"
        );
    }

    #[test]
    fn app_assets_falls_back_to_default_kit_icons() {
        let icon = AppAssets
            .load("icons/bot.svg")
            .expect("default asset lookup should succeed");
        assert!(icon.is_some(), "default GPUI Kit icon should still resolve");

        for extra in [
            gpui_kit::assets::IconName::Pencil,
            gpui_kit::assets::IconName::ExternalLink,
            gpui_kit::assets::IconName::LogOut,
        ] {
            let asset = AppAssets
                .load(extra.path().as_ref())
                .expect("extra Lucide icon lookup should succeed");
            assert!(asset.is_some(), "embedded icon {:?} should resolve", extra);
        }
    }
}
