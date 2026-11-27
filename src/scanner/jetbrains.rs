use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::model::{AgentKind, AgentProjectEntry};
use crate::scanner::AgentScanner;
use crate::scanner::paths::{
    app_config_dir, decode_file_uri, file_mtime_secs, home_dir, normalize_path,
    normalize_unix_timestamp,
};

const DEFAULT_RECENT_FILES: &[&str] = &["recentProjects.xml"];
const RIDER_RECENT_FILES: &[&str] = &["recentSolutions.xml", "recentProjects.xml"];

/// Unified scanner for JetBrains family IDEs (IntelliJ IDEA, PyCharm, WebStorm, GoLand,
/// CLion, PhpStorm, Rider, RustRover, RubyMine).
pub struct JetBrainsFamilyScanner {
    agent: AgentKind,
    dir_prefixes: &'static [&'static str],
    recent_files: &'static [&'static str],
}

impl JetBrainsFamilyScanner {
    pub fn intellij_idea() -> Self {
        Self {
            agent: AgentKind::IntelliJIdea,
            dir_prefixes: &["IntelliJIdea", "IdeaIC", "IdeaEdu"],
            recent_files: DEFAULT_RECENT_FILES,
        }
    }

    pub fn pycharm() -> Self {
        Self {
            agent: AgentKind::PyCharm,
            dir_prefixes: &["PyCharm", "PyCharmCE", "PyCharmEdu"],
            recent_files: DEFAULT_RECENT_FILES,
        }
    }

    pub fn webstorm() -> Self {
        Self {
            agent: AgentKind::WebStorm,
            dir_prefixes: &["WebStorm"],
            recent_files: DEFAULT_RECENT_FILES,
        }
    }

    pub fn goland() -> Self {
        Self {
            agent: AgentKind::GoLand,
            dir_prefixes: &["GoLand"],
            recent_files: DEFAULT_RECENT_FILES,
        }
    }

    pub fn clion() -> Self {
        Self {
            agent: AgentKind::CLion,
            dir_prefixes: &["CLion"],
            recent_files: DEFAULT_RECENT_FILES,
        }
    }

    pub fn phpstorm() -> Self {
        Self {
            agent: AgentKind::PhpStorm,
            dir_prefixes: &["PhpStorm"],
            recent_files: DEFAULT_RECENT_FILES,
        }
    }

    pub fn rider() -> Self {
        Self {
            agent: AgentKind::Rider,
            dir_prefixes: &["Rider"],
            recent_files: RIDER_RECENT_FILES,
        }
    }

    pub fn rustrover() -> Self {
        Self {
            agent: AgentKind::RustRover,
            dir_prefixes: &["RustRover"],
            recent_files: DEFAULT_RECENT_FILES,
        }
    }

    pub fn rubymine() -> Self {
        Self {
            agent: AgentKind::RubyMine,
            dir_prefixes: &["RubyMine"],
            recent_files: DEFAULT_RECENT_FILES,
        }
    }
}

impl AgentScanner for JetBrainsFamilyScanner {
    fn agent(&self) -> AgentKind {
        self.agent
    }

    fn scan(&self) -> Result<Vec<AgentProjectEntry>> {
        let Some(jb_root) = app_config_dir("JetBrains") else {
            return Ok(Vec::new());
        };
        if !jb_root.is_dir() {
            return Ok(Vec::new());
        }

        let home = home_dir();
        let mut by_path: HashMap<PathBuf, AgentProjectEntry> = HashMap::new();
        scan_jetbrains_root(
            &jb_root,
            home.as_deref(),
            self.agent,
            self.dir_prefixes,
            self.recent_files,
            &mut by_path,
        );
        Ok(by_path.into_values().collect())
    }
}

pub(crate) fn scan_jetbrains_root(
    jb_root: &Path,
    home: Option<&Path>,
    agent: AgentKind,
    dir_prefixes: &[&str],
    recent_files: &[&str],
    by_path: &mut HashMap<PathBuf, AgentProjectEntry>,
) {
    let Ok(entries) = fs::read_dir(jb_root) else {
        return;
    };

    for entry in entries.flatten() {
        let product_dir = entry.path();
        if !product_dir.is_dir() {
            continue;
        }
        let Some(dir_name) = product_dir.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !matches_product_dir(dir_name, dir_prefixes) {
            continue;
        }

        let options_dir = product_dir.join("options");
        if !options_dir.is_dir() {
            continue;
        }

        for file_name in recent_files {
            let xml_path = options_dir.join(file_name);
            if !xml_path.is_file() {
                continue;
            }
            let Ok(raw_xml) = fs::read_to_string(&xml_path) else {
                continue;
            };
            let fallback_ts = file_mtime_secs(&xml_path);
            let source = format!("{dir_name}/{file_name}");
            let parsed = parse_recent_projects_xml(&raw_xml);

            for rec in parsed {
                let Some(path) = resolve_jetbrains_path(&rec.raw_path, home) else {
                    continue;
                };
                let last_active = rec.last_active.or(fallback_ts);
                let idea_name = read_idea_dot_name(&path);
                let display_name = idea_name.or(rec.display_name);

                by_path
                    .entry(path.clone())
                    .and_modify(|existing| {
                        let is_newer = last_active > existing.last_active;
                        if is_newer {
                            existing.last_active = last_active;
                            existing.source = source.clone();
                        }
                        if display_name.is_some() && (existing.display_name.is_none() || is_newer) {
                            existing.display_name = display_name.clone();
                        }
                    })
                    .or_insert_with(|| AgentProjectEntry {
                        agent,
                        path,
                        display_name,
                        last_active,
                        session_count: None,
                        source: source.clone(),
                    });
            }
        }
    }
}

fn matches_product_dir(dir_name: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| {
        let Some(rest) = dir_name.strip_prefix(prefix) else {
            return false;
        };
        rest.is_empty()
            || rest.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawJetBrainsProject {
    raw_path: String,
    display_name: Option<String>,
    last_active: Option<i64>,
}

struct ActiveEntry {
    raw_path: String,
    display_name: Option<String>,
    hidden: bool,
    last_active: Option<i64>,
}

/// Parses JetBrains `recentProjects.xml` / `recentSolutions.xml` documents, extracting
/// projects from `additionalInfo`, legacy `recentPaths`, and `lastOpenedProject`.
fn parse_recent_projects_xml(xml: &str) -> Vec<RawJetBrainsProject> {
    let mut results: Vec<RawJetBrainsProject> = Vec::new();
    let mut seen_keys: Vec<String> = Vec::new();
    let mut fallback_paths: Vec<String> = Vec::new();

    // Nesting depths track which `<option>` block we are currently inside.
    let mut additional_info_depth: usize = 0;
    let mut recent_paths_depth: usize = 0;
    let mut current_entry: Option<ActiveEntry> = None;

    for tag in iter_xml_tags(xml) {
        match tag.kind {
            TagKind::Close => {
                if tag.name == "option" {
                    additional_info_depth = additional_info_depth.saturating_sub(1);
                    recent_paths_depth = recent_paths_depth.saturating_sub(1);
                } else if tag.name == "entry"
                    && let Some(entry) = current_entry.take()
                    && !entry.hidden
                {
                    seen_keys.push(entry.raw_path.clone());
                    results.push(RawJetBrainsProject {
                        raw_path: entry.raw_path,
                        display_name: entry.display_name,
                        last_active: entry.last_active,
                    });
                }
            }
            TagKind::Open | TagKind::SelfClosing => {
                let is_open = tag.kind == TagKind::Open;
                if tag.name == "option" {
                    if additional_info_depth > 0 {
                        if is_open {
                            additional_info_depth += 1;
                        }
                        if let Some(entry) = current_entry.as_mut()
                            && let (Some(name), Some(val)) = (tag.attr("name"), tag.attr("value"))
                            && matches!(
                                name.as_str(),
                                "activationTimestamp" | "projectOpenTimestamp"
                            )
                            && let Ok(raw_ts) = val.trim().parse::<i64>()
                            && let Some(secs) = normalize_unix_timestamp(raw_ts)
                            && Some(secs) > entry.last_active
                        {
                            entry.last_active = Some(secs);
                        }
                    } else if recent_paths_depth > 0 {
                        if is_open {
                            recent_paths_depth += 1;
                        }
                        if tag.attr("name").is_none()
                            && let Some(val) = tag.attr("value")
                        {
                            fallback_paths.push(val);
                        }
                    } else if let Some(name) = tag.attr("name") {
                        match name.as_str() {
                            "additionalInfo" if is_open => additional_info_depth = 1,
                            "recentPaths" if is_open => recent_paths_depth = 1,
                            "lastOpenedProject" => {
                                if let Some(val) = tag.attr("value") {
                                    fallback_paths.push(val);
                                }
                            }
                            _ => {}
                        }
                    }
                } else if tag.name == "entry" && additional_info_depth > 0 {
                    if let Some(key) = tag.attr("key") {
                        if is_open {
                            current_entry = Some(ActiveEntry {
                                raw_path: key,
                                display_name: None,
                                hidden: false,
                                last_active: None,
                            });
                        } else {
                            seen_keys.push(key.clone());
                            results.push(RawJetBrainsProject {
                                raw_path: key,
                                display_name: None,
                                last_active: None,
                            });
                        }
                    }
                } else if tag.name == "RecentProjectMetaInfo"
                    && let Some(entry) = current_entry.as_mut()
                {
                    if tag
                        .attr("hidden")
                        .is_some_and(|v| v.eq_ignore_ascii_case("true"))
                    {
                        entry.hidden = true;
                    }
                    let explicit_display = tag
                        .attr("displayName")
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty());
                    let frame_name = tag
                        .attr("frameTitle")
                        .and_then(|ft| extract_name_from_frame_title(&ft));
                    if let Some(name) = explicit_display.or(frame_name) {
                        entry.display_name = Some(name);
                    }
                }
            }
        }
    }

    for path in fallback_paths {
        if !seen_keys.iter().any(|k| k == &path) {
            seen_keys.push(path.clone());
            results.push(RawJetBrainsProject {
                raw_path: path,
                display_name: None,
                last_active: None,
            });
        }
    }

    results
}

/// Reads the JetBrains custom project name from `<project>/.idea/.name`.
fn read_idea_dot_name(project_dir: &Path) -> Option<String> {
    let name_file = project_dir.join(".idea").join(".name");
    let raw = fs::read_to_string(name_file).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Extracts the project title segment from a JetBrains `frameTitle` attribute.
/// Examples:
/// - `"FE-海丝 [~/work/thxpace-admin] – vite.config.js"` -> `"FE-海丝"`
/// - `"singo – router.go"` -> `"singo"`
fn extract_name_from_frame_title(frame_title: &str) -> Option<String> {
    let trimmed = frame_title.trim();
    if trimmed.is_empty() {
        return None;
    }

    let before_path = trimmed
        .split_once(" [")
        .map(|(head, _)| head)
        .unwrap_or(trimmed);
    let before_file = before_path
        .split_once(" \u{2013} ")
        .or_else(|| before_path.split_once(" - "))
        .map(|(head, _)| head)
        .unwrap_or(before_path);

    let clean = before_file.trim();
    if clean.is_empty() {
        None
    } else {
        Some(clean.to_string())
    }
}

fn resolve_jetbrains_path(raw: &str, home: Option<&Path>) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let expanded = if let Some(rest) = trimmed.strip_prefix("$USER_HOME$") {
        let home_dir = home?;
        let home_str = home_dir.to_string_lossy();
        let home_clean = home_str.trim_end_matches(['/', '\\']);
        format!("{home_clean}{rest}")
    } else if trimmed.starts_with('$') || contains_unexpanded_macro(trimmed) {
        // Skip internal macro paths such as `$APPLICATION_CONFIG_DIR$/light-edit`.
        return None;
    } else {
        trimmed.to_string()
    };

    let candidate = if expanded.starts_with("file://") {
        decode_file_uri(&expanded)?
    } else {
        PathBuf::from(&expanded)
    };

    if !candidate.is_absolute() {
        return None;
    }

    // Rider records `.sln` / `.slnx` / `.csproj` files in recentSolutions.xml; map to parent dir.
    let project_dir = if is_solution_file_path(&candidate) || candidate.is_file() {
        candidate.parent()?.to_path_buf()
    } else {
        candidate
    };

    let norm = normalize_path(&project_dir);
    if norm.is_absolute() { Some(norm) } else { None }
}

fn contains_unexpanded_macro(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && (bytes[j].is_ascii_uppercase() || bytes[j] == b'_') {
                j += 1;
            }
            if j > start && j < bytes.len() && bytes[j] == b'$' {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn is_solution_file_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            ext.eq_ignore_ascii_case("sln")
                || ext.eq_ignore_ascii_case("slnx")
                || ext.eq_ignore_ascii_case("csproj")
                || ext.eq_ignore_ascii_case("fsproj")
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TagKind {
    Open,
    Close,
    SelfClosing,
}

struct XmlTag<'a> {
    name: &'a str,
    kind: TagKind,
    attrs_src: &'a str,
}

impl<'a> XmlTag<'a> {
    fn attr(&self, target: &str) -> Option<String> {
        let bytes = self.attrs_src.as_bytes();
        let len = bytes.len();
        let mut i = 0;

        while i < len {
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= len {
                break;
            }
            let name_start = i;
            while i < len && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' && bytes[i] != b'/'
            {
                i += 1;
            }
            let attr_name = &self.attrs_src[name_start..i];
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= len || bytes[i] != b'=' {
                continue;
            }
            i += 1; // skip '='
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= len {
                break;
            }
            let quote = bytes[i];
            if quote != b'"' && quote != b'\'' {
                continue;
            }
            i += 1;
            let val_start = i;
            while i < len && bytes[i] != quote {
                i += 1;
            }
            let raw_val = &self.attrs_src[val_start..i];
            if i < len {
                i += 1; // skip closing quote
            }
            if attr_name == target {
                return Some(unescape_xml_entities(raw_val));
            }
        }
        None
    }
}

/// Scans an XML string into a sequence of structural tags, skipping comments,
/// processing instructions, and CDATA sections.
fn iter_xml_tags(xml: &str) -> Vec<XmlTag<'_>> {
    let bytes = xml.as_bytes();
    let len = bytes.len();
    let mut tags = Vec::new();
    let mut i = 0;

    while i < len {
        let Some(rel_lt) = xml[i..].find('<') else {
            break;
        };
        let lt = i + rel_lt;
        if lt + 1 >= len {
            break;
        }

        // Skip XML comments `<!-- ... -->`.
        if xml[lt..].starts_with("<!--") {
            if let Some(end) = xml[lt + 4..].find("-->") {
                i = lt + 4 + end + 3;
                continue;
            }
            break;
        }
        // Skip CDATA `<![CDATA[ ... ]]>`.
        if xml[lt..].starts_with("<![CDATA[") {
            if let Some(end) = xml[lt + 9..].find("]]>") {
                i = lt + 9 + end + 3;
                continue;
            }
            break;
        }
        // Skip processing instructions `<? ... ?>` and declarations `<! ... >`.
        if bytes[lt + 1] == b'?' {
            if let Some(end) = xml[lt + 2..].find("?>") {
                i = lt + 2 + end + 2;
                continue;
            }
            break;
        }
        if bytes[lt + 1] == b'!' {
            if let Some(end) = xml[lt + 2..].find('>') {
                i = lt + 2 + end + 1;
                continue;
            }
            break;
        }

        // Locate closing `>` while respecting quoted attribute values.
        let mut j = lt + 1;
        let mut in_quote: Option<u8> = None;
        while j < len {
            let b = bytes[j];
            if let Some(q) = in_quote {
                if b == q {
                    in_quote = None;
                }
            } else if b == b'"' || b == b'\'' {
                in_quote = Some(b);
            } else if b == b'>' {
                break;
            }
            j += 1;
        }
        if j >= len {
            break;
        }

        let inner = xml[lt + 1..j].trim();
        i = j + 1;
        if inner.is_empty() {
            continue;
        }

        if let Some(rest) = inner.strip_prefix('/') {
            let name = rest.split_whitespace().next().unwrap_or("");
            if !name.is_empty() {
                tags.push(XmlTag {
                    name,
                    kind: TagKind::Close,
                    attrs_src: "",
                });
            }
        } else {
            let (body, kind) = if let Some(stripped) = inner.strip_suffix('/') {
                (stripped.trim_end(), TagKind::SelfClosing)
            } else {
                (inner, TagKind::Open)
            };
            let name_end = body
                .find(|c: char| c.is_ascii_whitespace())
                .unwrap_or(body.len());
            let name = &body[..name_end];
            let attrs_src = &body[name_end..];
            if !name.is_empty() {
                tags.push(XmlTag {
                    name,
                    kind,
                    attrs_src,
                });
            }
        }
    }

    tags
}

fn unescape_xml_entities(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after_amp = &rest[amp + 1..];
        if let Some(semi) = after_amp.find(';') {
            let entity = &after_amp[..semi];
            let decoded = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                    u32::from_str_radix(&entity[2..], 16)
                        .ok()
                        .and_then(char::from_u32)
                }
                _ if entity.starts_with('#') => {
                    entity[1..].parse::<u32>().ok().and_then(char::from_u32)
                }
                _ => None,
            };
            if let Some(ch) = decoded {
                out.push(ch);
                rest = &after_amp[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = after_amp;
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parses_additional_info_filters_hidden_and_light_edit() {
        let xml = r#"<application>
  <component name="RecentProjectsManager">
    <option name="additionalInfo">
      <map>
        <entry key="$USER_HOME$/work/server &amp; api">
          <value>
            <RecentProjectMetaInfo frameTitle="server">
              <option name="activationTimestamp" value="1793956453900" />
              <option name="projectOpenTimestamp" value="1793869300732" />
            </RecentProjectMetaInfo>
          </value>
        </entry>
        <entry key="$APPLICATION_CONFIG_DIR$/light-edit">
          <value>
            <RecentProjectMetaInfo hidden="true">
              <option name="activationTimestamp" value="1745745365438" />
            </RecentProjectMetaInfo>
          </value>
        </entry>
        <entry key="$USER_HOME$/work/hidden-proj">
          <value>
            <RecentProjectMetaInfo hidden="true">
              <option name="activationTimestamp" value="1745745365438" />
            </RecentProjectMetaInfo>
          </value>
        </entry>
      </map>
    </option>
    <option name="lastOpenedProject" value="$USER_HOME$/work/server &amp; api" />
    <option name="lastProjectLocation" value="$USER_HOME$/work" />
  </component>
</application>"#;

        let parsed = parse_recent_projects_xml(xml);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].raw_path, "$USER_HOME$/work/server & api");
        assert_eq!(parsed[0].last_active, Some(1_793_956_453));

        let resolved = resolve_jetbrains_path(&parsed[0].raw_path, Some(Path::new("/Users/demo")));
        assert_eq!(
            resolved,
            Some(PathBuf::from("/Users/demo/work/server & api"))
        );
        assert!(
            resolve_jetbrains_path(
                "$APPLICATION_CONFIG_DIR$/light-edit",
                Some(Path::new("/Users/demo"))
            )
            .is_none()
        );
    }

    #[test]
    fn parses_legacy_recent_paths_and_rider_solutions() {
        let xml = r#"<application>
  <component name="RecentProjectsManager">
    <option name="recentPaths">
      <list>
        <option value="$USER_HOME$/dotnet/DemoApp/DemoApp.sln" />
      </list>
    </option>
  </component>
</application>"#;

        let parsed = parse_recent_projects_xml(xml);
        assert_eq!(parsed.len(), 1);
        let resolved = resolve_jetbrains_path(&parsed[0].raw_path, Some(Path::new("/Users/demo")));
        assert_eq!(resolved, Some(PathBuf::from("/Users/demo/dotnet/DemoApp")));
    }

    #[test]
    fn scans_multiple_versions_and_excludes_non_ide_tools() {
        let tmp = tempdir().unwrap();
        let idea_old = tmp.path().join("IntelliJIdea2023.3").join("options");
        let idea_new = tmp.path().join("IntelliJIdea2024.3").join("options");
        let datagrip = tmp.path().join("DataGrip2026.1").join("options");
        fs::create_dir_all(&idea_old).unwrap();
        fs::create_dir_all(&idea_new).unwrap();
        fs::create_dir_all(&datagrip).unwrap();

        fs::write(
            idea_old.join("recentProjects.xml"),
            r#"<application><component name="RecentProjectsManager"><option name="additionalInfo"><map>
              <entry key="$USER_HOME$/repo/a"><value><RecentProjectMetaInfo>
                <option name="activationTimestamp" value="1700000100000" />
              </RecentProjectMetaInfo></value></entry>
            </map></option></component></application>"#,
        )
        .unwrap();

        fs::write(
            idea_new.join("recentProjects.xml"),
            r#"<application><component name="RecentProjectsManager"><option name="additionalInfo"><map>
              <entry key="$USER_HOME$/repo/a"><value><RecentProjectMetaInfo>
                <option name="activationTimestamp" value="1750000200000" />
              </RecentProjectMetaInfo></value></entry>
            </map></option></component></application>"#,
        )
        .unwrap();

        fs::write(
            datagrip.join("recentProjects.xml"),
            r#"<application><component name="RecentProjectsManager"><option name="additionalInfo"><map>
              <entry key="$USER_HOME$/db/only-datagrip"><value><RecentProjectMetaInfo>
                <option name="activationTimestamp" value="1790000000000" />
              </RecentProjectMetaInfo></value></entry>
            </map></option></component></application>"#,
        )
        .unwrap();

        let mut map = HashMap::new();
        scan_jetbrains_root(
            tmp.path(),
            Some(Path::new("/Users/demo")),
            AgentKind::IntelliJIdea,
            &["IntelliJIdea", "IdeaIC", "IdeaEdu"],
            DEFAULT_RECENT_FILES,
            &mut map,
        );

        assert_eq!(map.len(), 1);
        let item = map.get(&PathBuf::from("/Users/demo/repo/a")).unwrap();
        assert_eq!(item.last_active, Some(1_750_000_200));
        assert_eq!(item.source, "IntelliJIdea2024.3/recentProjects.xml");
    }

    #[test]
    fn extracts_display_name_from_idea_dot_name_and_frame_title() {
        let tmp = tempdir().unwrap();
        let proj_dir = tmp.path().join("thxpace-admin");
        fs::create_dir_all(proj_dir.join(".idea")).unwrap();
        fs::write(proj_dir.join(".idea").join(".name"), "FE-福建海丝\n").unwrap();

        let jb_root = tmp.path().join("JetBrains");
        let ws_opts = jb_root.join("WebStorm2024.3").join("options");
        fs::create_dir_all(&ws_opts).unwrap();
        let xml = format!(
            r#"<application><component name="RecentProjectsManager"><option name="additionalInfo"><map>
              <entry key="{}"><value><RecentProjectMetaInfo frameTitle="旧名称 [~/x] – main.ts">
                <option name="activationTimestamp" value="1750000200000" />
              </RecentProjectMetaInfo></value></entry>
              <entry key="/nonexistent/Saber3"><value><RecentProjectMetaInfo frameTitle="常工院 [~/Saber3] – index.vue">
                <option name="activationTimestamp" value="1750000300000" />
              </RecentProjectMetaInfo></value></entry>
            </map></option></component></application>"#,
            proj_dir.to_string_lossy()
        );
        fs::write(ws_opts.join("recentProjects.xml"), xml).unwrap();

        let mut map = HashMap::new();
        scan_jetbrains_root(
            &jb_root,
            None,
            AgentKind::WebStorm,
            &["WebStorm"],
            DEFAULT_RECENT_FILES,
            &mut map,
        );

        let local = map.get(&normalize_path(&proj_dir)).unwrap();
        assert_eq!(local.display_name.as_deref(), Some("FE-福建海丝"));

        let missing = map.get(&PathBuf::from("/nonexistent/Saber3")).unwrap();
        assert_eq!(missing.display_name.as_deref(), Some("常工院"));
    }
}
