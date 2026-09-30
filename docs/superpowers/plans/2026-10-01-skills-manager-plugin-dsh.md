# Skills Manager 子项目甲 实施计划（插件识别 + DSH + 来源分类 + 现成元数据）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Skills Manager 认识 DeepSeek Harness、只读展示 Claude 插件与官方技能（带来源/版本/安装日期/仓库链接），并按「官方 / 插件 / 个人」三档区分。

**Architecture:** 在现有 Rust 核心上做最小延伸：①给适配器层加一个「应用专属数据根」例外（DSH 走 `DSH_HOME`）；②新增独立只读扫描模块 `plugin_scanner`，实时读 Claude 配置目录下 `plugins/`，不写数据库、不碰用户数据；③新增两个 Tauri 命令供前端调用；④前端新增一个自包含组件 `PluginSkillsSection`，挂在 Claude Code 的智能体页里，复用现有 `DetailSheet` + `SkillMarkdown` 展示内容。

**Tech Stack:** Rust 1.98（crate `app_lib`，代码在 `src-tauri/`）、Tauri 2、React 19 + TypeScript + Vite + Tailwind（代码在 `src/`）、react-i18next（三语：en / zh / zh-TW）。

**Spec:** `docs/superpowers/specs/2026-10-01-skills-manager-plugin-dsh-design.md`（随仓库入库）

## Global Constraints

- 上游基线：`xingkongliang/skills-manager` v1.40.2（MIT）；工作分支 `v1-plugin-dsh`；`main` 保持与上游一致。
- **只读铁律**：任何代码路径都不得写入 `plugins/` 目录下的任何内容；插件与官方技能不可删除/编辑/同步/部署。
- 一律使用 `PathBuf::join` 组合路径，禁止字符串拼接路径（Windows 分隔符）。
- 新增 UI 文案必须三语齐全：`src/i18n/en.json`、`src/i18n/zh.json`、`src/i18n/zh-TW.json`。
- Rust `rust-version = "1.77.2"`（Cargo.toml），不得引入需要更高版本的依赖；**不新增任何 Rust/npm 依赖**。
- 复用现有工具，不重复造：frontmatter 解析用 `core::skill_metadata::{parse_skill_md, is_valid_skill_dir}`；URL 打开用前端 `@tauri-apps/plugin-opener` 的 `openUrl`（见 `src/views/Backup.tsx:23`）；文档面板用 `src/components/DetailSheet.tsx` + `src/components/SkillMarkdown.tsx`。
- 每个任务结束必须 commit；提交信息用英文、`feat:` / `test:` 前缀。

## Review Focus

以下失败模式规格书未逐条明说，但任何一个崩了都会让用户踩坑；每条都已在下面对应任务的测试里钉死：

1. `CLAUDE_CONFIG_DIR` 未设置 / 指向不存在目录 → 插件区显示空态，绝不崩溃（Task 4 测试）。
2. `plugins/installed_plugins.json` 缺失、非 JSON、或缺 `installedAt`/`lastUpdated` 字段 → 对应元数据留空，列表照常显示（Task 3 测试）。
3. 同一插件存在多个版本目录（如 `5.0.7` 与 `6.4.1`）→ 只取最高版本，不重复列出（Task 3 测试）。
4. 文档请求携带 `..` 或指向 `plugins/` 之外的路径 → 必须拒绝（Task 4 测试）。
5. 插件目录里无 `SKILL.md` 的子目录 → 跳过而非报错（Task 3 测试）。
6. `SKILL.md` 无法读取（权限/编码）→ 该技能跳过或描述留空，整体扫描不失败（Task 3 测试）。

---

### Task 1: 环境关 — fork、克隆、基线编译、装上跑起来

**Files:**
- 创建（克隆所得）：`D:\CloudMusic\skills-manager\`（整个仓库）
- 读取：`src-tauri/tauri.conf.json`（记录 productName / identifier）、`package.json`

**Interfaces:**
- Consumes: 无
- Produces: 可编译运行的工作副本；后续所有任务在此目录内操作；分支 `v1-plugin-dsh`

- [ ] **Step 1: 打开浏览器让用户点 Fork（一键）**

Run:
```bash
powershell -NoProfile -Command "Start-Process 'https://github.com/xingkongliang/skills-manager/fork'"
```
让用户在页面中点绿色 **Create fork**（约 3 秒）；然后确认 `git@github.com:Acbgd11/skills-manager.git` 存在（可在浏览器地址栏看到）。若用户已有点击过，跳过等待。

- [ ] **Step 2: 克隆自己的 fork 并接上上游**

Run:
```bash
git clone git@github.com:Acbgd11/skills-manager.git /d/CloudMusic/skills-manager
cd /d/CloudMusic/skills-manager
git remote add upstream https://github.com/xingkongliang/skills-manager.git
git remote -v
```
Expected: `origin` 指向 Acbgd11/skills-manager（SSH），`upstream` 指向原作者。SSH 走 `C:\Users\hp\.ssh\config` 里已配好的代理通道。

- [ ] **Step 3: 建工作分支**

Run:
```bash
cd /d/CloudMusic/skills-manager
git checkout -b v1-plugin-dsh
```

- [ ] **Step 4: 装前端依赖并读构建配置**

Run:
```bash
cd /d/CloudMusic/skills-manager
npm ci
cat src-tauri/tauri.conf.json | head -30
```
Expected: `npm ci` 成功；记下 productName 与 identifier（预期为官方同名，安装时覆盖用户已装的 v1.40.2 —— 这是预期行为，回退方案=重装官方安装包）。

- [ ] **Step 5: 基线编译（最险的一步，先零改动跑通）**

Run:
```bash
cd /d/CloudMusic/skills-manager
npm run tauri:build
```
Expected: 首次编译 10-30 分钟，产出安装包（`src-tauri/target/release/bundle/`）。

**若卡在下载（crates.io 慢/超时）**，写 `C:\Users\hp\.cargo\config.toml`（可回退）：
```toml
[source.crates-io]
replace-with = "rsproxy-sparse"

[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"
```
**若报 openssl/perl 相关错误**（`vendored-openssl` 需要 Perl）：
```bash
winget install --id StrawberryPerl.StrawberryPerl --accept-source-agreements --accept-package-agreements
```
装完重跑 Step 5。

- [ ] **Step 6: 安装并运行基线版**

Run: 双击安装包安装（静默：直接运行安装器 `/S`），然后：
```bash
powershell -NoProfile -Command "Start-Process 'C:\Users\hp\skills-manager\skills-manager.exe'"
```
Expected: 窗口正常打开；用户的技能库数据（`C:\Users\hp\.skills-manager`）不受影响。

- [ ] **Step 7: 记录基线并提交分支起点**

Run:
```bash
cd /d/CloudMusic/skills-manager
git status
git log --oneline -1
```
Expected: 干净工作区；在 `v1-plugin-dsh` 分支上，与上游 v1.40.2 同一提交。

---

### Task 2: DSH 适配器 — 尊重 DSH_HOME（修改既有条目）

**Files:**
- Modify: `src-tauri/src/core/tool_adapters.rs`（**供应商已内置** `deepseek_harness` 条目，位于 :805-830，配套测试 :1118-1136；本任务修改它们，**不新增条目**——新增会产生重复 key）

**Interfaces:**
- Consumes: 无
- Produces:
  - `ToolAdapter::root_override(key: &str) -> Option<PathBuf>`（私有；`"deepseek_harness"` → 环境变量 `DSH_HOME` 非空时取其值，否则回退 `~/.dsh`）
  - `ToolAdapter::base_dir(&self) -> PathBuf`（`root_override` 命中则用其值，否则 `home()`）
  - 行为变更（既有适配器）：`relative_skills_dir` → `"skills"`、`relative_detect_dir` → `"skills"`、`project_relative_skills_dir` → `Some(".dsh/skills")`、`additional_scan_dirs` 保持 `[".agents/skills"]`（仍按家目录解析）

**设计说明（计划修正，2026-10-01 裁定）：** 上游 v1.34.0（commit `5bc948a`）早已内置该适配器，但上游注释明说「适配器不读环境变量」，故本机 `DSH_HOME=D:\DSH-Official\DSH-Home` 时检测不到——这正是用户问题 #1 的根因。本任务 = 给既有条目加环境变量根支持（spec §3.2），**不新增重复条目**。

- [ ] **Step 1: 修改既有测试（TDD 起点）**

在 `tool_adapters.rs` 的既有测试 `deepseek_harness_deploys_to_its_own_home_and_discovers_the_shared_root`（约 :1118）中：

```rust
        assert_eq!(adapter.relative_skills_dir, "skills");
        assert_eq!(adapter.relative_detect_dir, "skills");
```

并追加两个新测试（注意：tests 模块需要 `use std::path::PathBuf;`，若缺请补上）：

```rust
    #[test]
    fn dsh_adapter_resolves_env_home_and_fallback() {
        let adapter = default_tool_adapters()
            .into_iter()
            .find(|a| a.key == "deepseek_harness")
            .expect("deepseek_harness adapter should exist");

        std::env::set_var("DSH_HOME", "D:\\dsh-test-home");
        assert_eq!(
            adapter.skills_dir(),
            PathBuf::from("D:\\dsh-test-home").join("skills")
        );

        std::env::remove_var("DSH_HOME");
        assert_eq!(
            adapter.skills_dir(),
            ToolAdapter::home().join(".dsh").join("skills")
        );
    }

    #[test]
    fn dsh_shared_agents_root_stays_home_relative() {
        let adapter = default_tool_adapters()
            .into_iter()
            .find(|a| a.key == "deepseek_harness")
            .expect("deepseek_harness adapter should exist");
        assert_eq!(
            adapter.project_relative_skills_dir(),
            ".dsh/skills",
            "project roots keep the dotted .dsh layout"
        );
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cd /d/CloudMusic/skills-manager/src-tauri && cargo test deepseek_harness`
Expected: FAIL（现有断言 `.dsh/skills` 对新值 `"skills"`；`skills_dir()` 尚不认 DSH_HOME）

- [ ] **Step 3: 实现**

在 `impl ToolAdapter` 内新增（并让 `skills_dir()`/`is_installed()` 走 `base_dir`）：

```rust
    /// 应用专属数据根：DSH 用 `$DSH_HOME`，未设置时回退 `~/.dsh`（spec §3.2）。
    /// 仅作用于主 skills 根与安装检测；`additional_scan_dirs`（共享 `~/.agents` 根）
    /// 保持家目录解析，不受此覆盖影响。
    fn root_override(key: &str) -> Option<PathBuf> {
        match key {
            "deepseek_harness" => {
                if let Ok(value) = std::env::var("DSH_HOME") {
                    let trimmed = value.trim();
                    if !trimmed.is_empty() {
                        return Some(PathBuf::from(trimmed));
                    }
                }
                Some(Self::home().join(".dsh"))
            }
            _ => None,
        }
    }

    fn base_dir(&self) -> PathBuf {
        Self::root_override(&self.key).unwrap_or_else(Self::home)
    }

    /// 相对路径的候选解析（主根）。`additional_scan_dirs` 请用 `home_candidate_paths`。
    fn candidate_paths(&self, relative: &str) -> Vec<PathBuf> {
        let mut candidates = vec![self.base_dir().join(relative)];

        if let Some(suffix) = relative.strip_prefix(".config/") {
            if let Some(config_dir) = dirs::config_dir() {
                let config_path = config_dir.join(suffix);
                if !candidates.contains(&config_path) {
                    candidates.push(config_path);
                }
            }
        }

        candidates
    }

    /// 家目录基准的候选解析（`additional_scan_dirs` 专用，保持上游语义）。
    fn home_candidate_paths(relative: &str) -> Vec<PathBuf> {
        let mut candidates = vec![Self::home().join(relative)];

        if let Some(suffix) = relative.strip_prefix(".config/") {
            if let Some(config_dir) = dirs::config_dir() {
                let config_path = config_dir.join(suffix);
                if !candidates.contains(&config_path) {
                    candidates.push(config_path);
                }
            }
        }

        candidates
    }
```

调用点调整（以 `grep -n candidate_paths src-tauri/src/core/tool_adapters.rs` 为准）：
- `skills_dir()` → `self.candidate_paths(...)`
- `is_installed()` → `self.candidate_paths(...)`
- `additional_existing_scan_dirs()` → `Self::home_candidate_paths(rel)`

修改既有 `deepseek_harness` 条目（:805-830）：`relative_skills_dir: "skills".into()`（原 `.dsh/skills`）、`relative_detect_dir: "skills".into()`（原 `.dsh`）、`project_relative_skills_dir: Some(".dsh/skills".into())`（原 None；钉住项目根，避免退化成裸 `skills`）、`additional_scan_dirs: vec![".agents/skills".into()]` 不变。同步更新该条目上方注释：说明主根现由 `$DSH_HOME`（回退 `~/.dsh`）解析。

- [ ] **Step 4: 运行测试确认通过**

Run: `cd /d/CloudMusic/skills-manager/src-tauri && cargo test deepseek_harness && cargo test`
Expected: 新测试 PASS；全量 `cargo test` 无回归（基线：500 passed / 2 failed / 6 ignored，那 2 个失败是 Windows 符号链接特权 1314 的既有环境问题，不算回归）。

- [ ] **Step 5: Commit**

```bash
cd /d/CloudMusic/skills-manager
git add src-tauri/src/core/tool_adapters.rs
git commit -m "feat: resolve the DeepSeek Harness skills root from DSH_HOME

Upstream adapters read no env vars, so a DSH_HOME-based install is
never detected. The adapter keeps its dotted project root
(<project>/.dsh/skills) and the shared ~/.agents discovery root;
only the global skills root now follows DSH_HOME (~/.dsh fallback)."
```
---

### Task 3: 插件技能扫描模块（只读）

**Files:**
- Create: `src-tauri/src/core/plugin_scanner.rs`
- Modify: `src-tauri/src/core/mod.rs`（加 `pub mod plugin_scanner;`）
- Test: 同文件底部 `#[cfg(test)] mod tests`（用 `tempfile`，已是依赖）

**Interfaces:**
- Consumes: `crate::core::skill_metadata::{parse_skill_md, is_valid_skill_dir}`
- Produces（Task 4/5 依赖，签名固定）:
  - `pub struct PluginSkillEntry { pub name: String, pub description: Option<String>, pub relative_path: String }`
  - `pub struct PluginSkillGroup { pub marketplace: String, pub plugin: String, pub version: String, pub installed_at: Option<String>, pub last_updated: Option<String>, pub homepage: Option<String>, pub repository: Option<String>, pub author: Option<String>, pub description: Option<String>, pub skills: Vec<PluginSkillEntry> }`
  - `pub fn claude_config_dir() -> PathBuf`
  - `pub fn scan_plugin_skills(config_dir: &Path) -> Vec<PluginSkillGroup>`
  - `pub fn scan_official_skills(config_dir: &Path) -> Vec<PluginSkillEntry>`

**设计说明（与规格书一处差异，更优）：** 插件扫描走独立只读模块，**不改动** `claude_code` 适配器与其既有测试 `claude_code_does_not_scan_plugin_marketplaces_by_default` —— 该测试保持原样通过（适配器本身依旧不扫插件目录，扫描由上层独立完成，互不侵入）。

- [ ] **Step 1: 写失败测试**

在新建的 `plugin_scanner.rs` 里先写模块骨架 + 测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_skill(dir: &std::path::Path, name: &str, description: &str) {
        let skill_dir = dir.join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n\nBody\n"),
        )
        .unwrap();
    }

    #[test]
    fn scans_plugin_skills_with_metadata_and_picks_newest_version() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let market = root.join("plugins/cache/claude-plugins-official");
        let old = market.join("superpowers").join("5.0.7");
        let new = market.join("superpowers").join("6.4.1");
        write_skill(&old.join("skills"), "old-skill", "old");
        write_skill(&new.join("skills"), "brainstorming", "Design first");
        fs::create_dir_all(new.join(".claude-plugin")).unwrap();
        fs::write(
            new.join(".claude-plugin/plugin.json"),
            r#"{"name":"superpowers","description":"Core skills","author":{"name":"Jesse"},"homepage":"https://github.com/obra/superpowers","repository":"https://github.com/obra/superpowers"}"#,
        )
        .unwrap();
        fs::create_dir_all(root.join("plugins")).unwrap();
        fs::write(
            root.join("plugins/installed_plugins.json"),
            r#"{"version":2,"plugins":{"superpowers@claude-plugins-official":[{"scope":"user","version":"6.4.1","installedAt":"2026-09-26T18:36:13.904Z","lastUpdated":"2026-09-27T01:02:03.000Z"}]}}"#,
        )
        .unwrap();

        let groups = scan_plugin_skills(root);
        assert_eq!(groups.len(), 1);
        let g = &groups[0];
        assert_eq!(g.plugin, "superpowers");
        assert_eq!(g.version, "6.4.1");
        assert_eq!(g.installed_at.as_deref(), Some("2026-09-26"));
        assert_eq!(g.last_updated.as_deref(), Some("2026-09-27"));
        assert_eq!(g.repository.as_deref(), Some("https://github.com/obra/superpowers"));
        assert_eq!(g.skills.len(), 1, "only newest version dir is scanned");
        assert_eq!(g.skills[0].name, "brainstorming");
    }

    #[test]
    fn tolerates_missing_installed_plugins_json() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_skill(&root.join("plugins/cache/mkt/plug/1.0.0/skills"), "s", "d");
        let groups = scan_plugin_skills(root);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].installed_at, None);
        assert_eq!(groups[0].version, "1.0.0");
    }

    #[test]
    fn skips_dirs_without_skill_md_and_missing_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("plugins/cache/mkt/plug/1.0.0/skills/not-a-skill")).unwrap();
        assert!(scan_plugin_skills(root).is_empty());
        assert!(scan_plugin_skills(&root.join("does-not-exist")).is_empty());
        assert!(scan_official_skills(&root.join("does-not-exist")).is_empty());
    }

    #[test]
    fn scans_official_skills_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_skill(&root.join("plugins/marketplaces/anthropic-agent-skills/skills"), "docx", "Word files");
        let official = scan_official_skills(root);
        assert_eq!(official.len(), 1);
        assert_eq!(official[0].name, "docx");
    }

    #[test]
    fn tolerates_malformed_installed_plugins_json() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_skill(&root.join("plugins/cache/mkt/plug/1.0.0/skills"), "s", "d");
        fs::create_dir_all(root.join("plugins")).unwrap();
        fs::write(root.join("plugins/installed_plugins.json"), "{not json").unwrap();
        let groups = scan_plugin_skills(root);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].installed_at, None);
    }

    #[test]
    fn unreadable_skill_md_does_not_break_scan() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let skill_dir = root.join("plugins/cache/mkt/plug/1.0.0/skills/broken");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), [0xff, 0xfe, 0x00, 0x01]).unwrap();
        let groups = scan_plugin_skills(root);
        assert_eq!(groups.len(), 1, "scan continues");
        assert_eq!(groups[0].skills.len(), 1);
        assert_eq!(groups[0].skills[0].description, None);
    }

    #[test]
    fn claude_config_dir_prefers_env_var() {
        std::env::set_var("CLAUDE_CONFIG_DIR", "D:\\claude-cfg-test");
        assert_eq!(claude_config_dir(), PathBuf::from("D:\\claude-cfg-test"));
        std::env::remove_var("CLAUDE_CONFIG_DIR");
        assert!(claude_config_dir().ends_with(".claude"));
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cd /d/CloudMusic/skills-manager/src-tauri && cargo test plugin_scanner`
Expected: FAIL（模块/函数不存在）

- [ ] **Step 3: 实现 `plugin_scanner.rs`**

```rust
//! Read-only discovery of Claude Code plugin skills and official skills.
//! Nothing in this module writes to the user's Claude configuration.

use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::core::skill_metadata::{is_valid_skill_dir, parse_skill_md};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PluginSkillEntry {
    pub name: String,
    pub description: Option<String>,
    pub relative_path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PluginSkillGroup {
    pub marketplace: String,
    pub plugin: String,
    pub version: String,
    pub installed_at: Option<String>,
    pub last_updated: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub skills: Vec<PluginSkillEntry>,
}

#[derive(Debug, Clone, Default)]
struct InstalledPluginInfo {
    version: Option<String>,
    installed_at: Option<String>,
    last_updated: Option<String>,
}

pub fn claude_config_dir() -> PathBuf {
    if let Ok(value) = std::env::var("CLAUDE_CONFIG_DIR") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
}

/// "2026-09-26T18:36:13.904Z" -> "2026-09-26"
fn iso_date(value: Option<&str>) -> Option<String> {
    let s = value?.trim();
    if s.len() >= 10 && s.as_bytes()[4] == b'-' && s.as_bytes()[7] == b'-' {
        Some(s[..10].to_string())
    } else {
        None
    }
}

fn read_installed_plugins(config_dir: &Path) -> HashMap<String, InstalledPluginInfo> {
    let path = config_dir.join("plugins").join("installed_plugins.json");
    let Ok(text) = fs::read_to_string(&path) else {
        return HashMap::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return HashMap::new();
    };
    let mut map = HashMap::new();
    let Some(plugins) = json.get("plugins").and_then(|v| v.as_object()) else {
        return map;
    };
    for (id, entries) in plugins {
        let Some(first) = entries.as_array().and_then(|a| a.first()) else {
            continue;
        };
        map.insert(
            id.clone(),
            InstalledPluginInfo {
                version: first.get("version").and_then(|v| v.as_str()).map(str::to_string),
                installed_at: iso_date(first.get("installedAt").and_then(|v| v.as_str())),
                last_updated: iso_date(first.get("lastUpdated").and_then(|v| v.as_str())),
            },
        );
    }
    map
}

/// Highest semver version directory inside a plugin dir; falls back to name order.
fn newest_version_dir(plugin_dir: &Path) -> Option<PathBuf> {
    let mut dirs: Vec<(Option<semver::Version>, PathBuf)> = Vec::new();
    for entry in fs::read_dir(plugin_dir).ok()? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        dirs.push((semver::Version::parse(&name).ok(), path));
    }
    dirs.sort_by(|a, b| match (&a.0, &b.0) {
        (Some(x), Some(y)) => x.cmp(y),
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (None, None) => a.1.cmp(&b.1),
    });
    dirs.pop().map(|(_, p)| p)
}

fn read_plugin_manifest(version_dir: &Path) -> (Option<String>, Option<String>, Option<String>, Option<String>) {
    let path = version_dir.join(".claude-plugin").join("plugin.json");
    let Ok(text) = fs::read_to_string(&path) else {
        return (None, None, None, None);
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return (None, None, None, None);
    };
    let s = |key: &str| json.get(key).and_then(|v| v.as_str()).map(str::to_string);
    let author = json
        .get("author")
        .and_then(|a| a.get("name").and_then(|v| v.as_str()).or_else(|| a.as_str()))
        .map(str::to_string);
    (s("description"), s("homepage"), s("repository"), author)
}

fn skills_under(skills_root: &Path, relative_prefix: &Path) -> Vec<PluginSkillEntry> {
    let mut skills = Vec::new();
    let Ok(entries) = fs::read_dir(skills_root) else {
        return skills;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || !is_valid_skill_dir(&path) {
            continue;
        }
        let meta = parse_skill_md(&path);
        let Some(relative) = relative_prefix
            .join(entry.file_name())
            .to_str()
            .map(|s| s.replace('\\', "/"))
        else {
            continue;
        };
        skills.push(PluginSkillEntry {
            name: meta
                .name
                .clone()
                .unwrap_or_else(|| entry.file_name().to_string_lossy().to_string()),
            description: meta.description.clone(),
            relative_path: relative,
        });
    }
    skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    skills
}

pub fn scan_plugin_skills(config_dir: &Path) -> Vec<PluginSkillGroup> {
    let installed = read_installed_plugins(config_dir);
    let cache_root = config_dir.join("plugins").join("cache");
    let mut groups = Vec::new();
    let Ok(markets) = fs::read_dir(&cache_root) else {
        return groups;
    };
    for market in markets.flatten() {
        let market_name = market.file_name().to_string_lossy().to_string();
        if !market.path().is_dir() || market_name.starts_with('.') {
            continue;
        }
        let Ok(plugins) = fs::read_dir(market.path()) else { continue };
        for plugin in plugins.flatten() {
            let plugin_name = plugin.file_name().to_string_lossy().to_string();
            if !plugin.path().is_dir() || plugin_name.starts_with('.') {
                continue;
            }
            let Some(version_dir) = newest_version_dir(&plugin.path()) else {
                continue;
            };
            let version = version_dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let info = installed.get(&format!("{plugin_name}@{market_name}"));
            let (description, homepage, repository, author) = read_plugin_manifest(&version_dir);
            let relative_prefix = Path::new("cache")
                .join(&market_name)
                .join(&plugin_name)
                .join(&version)
                .join("skills");
            let skills = skills_under(&version_dir.join("skills"), &relative_prefix);
            if skills.is_empty() {
                continue;
            }
            groups.push(PluginSkillGroup {
                marketplace: market_name.clone(),
                plugin: plugin_name,
                version: info
                    .and_then(|i| i.version.clone())
                    .unwrap_or(version),
                installed_at: info.and_then(|i| i.installed_at.clone()),
                last_updated: info.and_then(|i| i.last_updated.clone()),
                homepage,
                repository,
                author,
                description,
                skills,
            });
        }
    }
    groups.sort_by(|a, b| a.plugin.to_lowercase().cmp(&b.plugin.to_lowercase()));
    groups
}

pub fn scan_official_skills(config_dir: &Path) -> Vec<PluginSkillEntry> {
    let root = config_dir
        .join("plugins")
        .join("marketplaces")
        .join("anthropic-agent-skills")
        .join("skills");
    skills_under(&root, Path::new("marketplaces/anthropic-agent-skills/skills"))
}
```

In `src-tauri/src/core/mod.rs` add: `pub mod plugin_scanner;`

- [ ] **Step 4: 运行测试确认通过**

Run: `cd /d/CloudMusic/skills-manager/src-tauri && cargo test plugin_scanner`
Expected: 4 tests PASS

- [ ] **Step 5: Commit**

```bash
cd /d/CloudMusic/skills-manager
git add src-tauri/src/core/plugin_scanner.rs src-tauri/src/core/mod.rs
git commit -m "feat: add read-only plugin and official skill scanner"
```

---

### Task 4: Tauri 命令（列表 + 文档）

**Files:**
- Create: `src-tauri/src/commands/plugins.rs`
- Modify: `src-tauri/src/commands/mod.rs`（加 `pub mod plugins;`）
- Modify: `src-tauri/src/lib.rs:993` 附近的 `invoke_handler` 注册两个新命令
- Test: `commands/plugins.rs` 底部 tests（路径校验）

**Interfaces:**
- Consumes: Task 3 的 `plugin_scanner::{claude_config_dir, scan_plugin_skills, scan_official_skills, PluginSkillGroup, PluginSkillEntry}`；`core::project_scanner::ProjectSkillDocumentDto`
- Produces:
  - `get_claude_plugin_skills() -> PluginSkillsDto { groups: Vec<PluginSkillGroup>, official: Vec<PluginSkillEntry>, config_dir: String }`
  - `get_plugin_skill_document(relative_path: String) -> ProjectSkillDocumentDto`
  - 前端命令名：`"get_claude_plugin_skills"`、`"get_plugin_skill_document"`

- [ ] **Step 1: 写失败测试（路径校验）**

```rust
    #[test]
    fn rejects_paths_outside_plugins_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("plugins/cache/m/p/1.0.0/skills/s")).unwrap();
        assert!(resolve_skill_dir(root, "../outside").is_err());
        assert!(resolve_skill_dir(root, "cache/m/p/1.0.0/skills/s").is_ok());
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cd /d/CloudMusic/skills-manager/src-tauri && cargo test rejects_paths_outside_plugins_root`
Expected: FAIL（模块不存在）

- [ ] **Step 3: 实现 `commands/plugins.rs`**

```rust
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::command;

use crate::commands::projects::ProjectSkillDocumentDto;
use crate::core::error::AppError;
use crate::core::plugin_scanner;

#[derive(Debug, Serialize)]
pub struct PluginSkillsDto {
    pub groups: Vec<plugin_scanner::PluginSkillGroup>,
    pub official: Vec<plugin_scanner::PluginSkillEntry>,
    pub config_dir: String,
}

/// Resolve a plugin-relative skill path, refusing anything outside `<config>/plugins`.
fn resolve_skill_dir(config_dir: &Path, relative_path: &str) -> Result<PathBuf, AppError> {
    if relative_path.contains("..") {
        return Err(AppError::invalid_input("Invalid skill path"));
    }
    let plugins_root = config_dir.join("plugins");
    let full = plugins_root.join(relative_path);
    let canon_root = std::fs::canonicalize(&plugins_root)
        .map_err(|_| AppError::invalid_input("Plugins directory not found"))?;
    let canon_full = std::fs::canonicalize(&full)
        .map_err(|_| AppError::invalid_input("Skill directory not found"))?;
    if !canon_full.starts_with(&canon_root) {
        return Err(AppError::invalid_input("Path outside plugins directory"));
    }
    Ok(canon_full)
}

#[command]
pub async fn get_claude_plugin_skills() -> Result<PluginSkillsDto, AppError> {
    tauri::async_runtime::spawn_blocking(|| {
        let config_dir = plugin_scanner::claude_config_dir();
        let groups = plugin_scanner::scan_plugin_skills(&config_dir);
        let official = plugin_scanner::scan_official_skills(&config_dir);
        Ok(PluginSkillsDto {
            groups,
            official,
            config_dir: config_dir.to_string_lossy().to_string(),
        })
    })
    .await?
}

#[command]
pub async fn get_plugin_skill_document(relative_path: String) -> Result<ProjectSkillDocumentDto, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let config_dir = plugin_scanner::claude_config_dir();
        let skill_dir = resolve_skill_dir(&config_dir, &relative_path)?;
        for candidate in ["SKILL.md", "skill.md", "CLAUDE.md", "README.md"] {
            let file_path = skill_dir.join(candidate);
            if file_path.is_file() {
                let content = std::fs::read_to_string(&file_path)
                    .map_err(|_| AppError::invalid_input("Skill document is not readable"))?;
                return Ok(ProjectSkillDocumentDto {
                    skill_name: relative_path.clone(),
                    filename: candidate.to_string(),
                    content,
                });
            }
        }
        Err(AppError::invalid_input("No skill document found"))
    })
    .await?
}
```

说明：`AppError::invalid_input` 已存在于 `core/error.rs:67`；`spawn_blocking(...).await?` 的写法与 `commands/agent_workspace.rs:142` 的 `get_global_local_skills` 完全一致（闭包返回 `Ok(...)`，外层 `?` 透传）。

在 `lib.rs` 的 `generate_handler!` 中「// Skills」注释块附近加：
```rust
            commands::plugins::get_claude_plugin_skills,
            commands::plugins::get_plugin_skill_document,
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd /d/CloudMusic/skills-manager/src-tauri && cargo test rejects_paths && cargo test`
Expected: 新测试 PASS，全量无回归

- [ ] **Step 5: Commit**

```bash
cd /d/CloudMusic/skills-manager
git add src-tauri/src/commands/plugins.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs
git commit -m "feat: expose plugin skill list and document commands"
```

---

### Task 5: 前端（API + 组件 + 三语文案 + 挂载）

**Files:**
- Modify: `src/lib/tauri.ts`（加类型 + 两个 API 封装）
- Create: `src/components/PluginSkillsSection.tsx`
- Modify: `src/views/WorkspaceView.tsx`（Claude Code 详情页挂载组件 + 个人档徽标）
- Modify: `src/i18n/en.json`、`src/i18n/zh.json`、`src/i18n/zh-TW.json`（新增 `pluginSkills` 命名空间）

**Interfaces:**
- Consumes: Task 4 命令 `get_claude_plugin_skills`、`get_plugin_skill_document`；`DetailSheet`、`SkillMarkdown`、`@tauri-apps/plugin-opener` 的 `openUrl`
- Produces: `<PluginSkillsSection agentKey={string} />`

- [ ] **Step 1: `src/lib/tauri.ts` 加类型与 API（追加文件末尾附近，风格照抄邻近的 `getGlobalLocalSkills`）**

```ts
// ── Plugin Skills (read-only) ──

export interface PluginSkillEntry {
  name: string;
  description: string | null;
  relative_path: string;
}

export interface PluginSkillGroup {
  marketplace: string;
  plugin: string;
  version: string;
  installed_at: string | null;
  last_updated: string | null;
  homepage: string | null;
  repository: string | null;
  author: string | null;
  description: string | null;
  skills: PluginSkillEntry[];
}

export interface PluginSkillsDto {
  groups: PluginSkillGroup[];
  official: PluginSkillEntry[];
  config_dir: string;
}

export const getClaudePluginSkills = () =>
  invoke<PluginSkillsDto>("get_claude_plugin_skills");

export const getPluginSkillDocument = (relativePath: string) =>
  invoke<ProjectSkillDocument>("get_plugin_skill_document", { relativePath });
```

- [ ] **Step 2: 三语文案（三个 JSON 各加一段，键名完全相同）**

`src/i18n/zh.json`（追加到顶层）：
```json
  "pluginSkills": {
    "title": "来自插件与官方的技能",
    "officialTitle": "官方技能",
    "pluginsTitle": "插件技能",
    "personalBadge": "个人",
    "readOnly": "只读",
    "marketplace": "市场",
    "version": "版本",
    "installedAt": "安装于",
    "updatedAt": "更新于",
    "author": "作者",
    "openRepo": "打开仓库",
    "empty": "未发现插件技能",
    "loading": "正在扫描插件…",
    "documentError": "无法读取该技能文档"
  },
```
`en.json`：`"title": "Skills from plugins & official sources"`、`"officialTitle": "Official skills"`、`"pluginsTitle": "Plugin skills"`、`"personalBadge": "Personal"`、`"readOnly": "Read-only"`、`"marketplace": "Marketplace"`、`"version": "Version"`、`"installedAt": "Installed"`、`"updatedAt": "Updated"`、`"author": "Author"`、`"openRepo": "Open repository"`、`"empty": "No plugin skills found"`、`"loading": "Scanning plugins…"`、`"documentError": "Unable to read this skill document"`。
`zh-TW.json`：与 zh.json 同义，用繁体（例：`"來自外掛與官方的技能"`、`"外掛技能"`、`"唯讀"`、`"安裝於"`、`"更新於"`、`"開啟儲存庫"`）。

- [ ] **Step 3: 新建 `src/components/PluginSkillsSection.tsx`**

结构要求（照抄项目里现有组件的写法风格：`useEffect` + `useState`、`cn`、lucide 图标、`t()`）：
- 加载：`getClaudePluginSkills()`；加载中显示 `t("pluginSkills.loading")`
- 渲染两块：
  - 官方块：标题 `officialTitle` + 数量，技能行（名字 + 描述）
  - 插件块：标题 `pluginsTitle` + 数量；每个插件一个可折叠分组：标题 `plugin`（`author` 有则显示），元信息行 = `市场 · 版本 · 安装于 YYYY-MM-DD · 更新于 YYYY-MM-DD`，右侧「只读」徽标与「打开仓库」按钮（`openUrl(group.repository ?? group.homepage)`，两者皆空则隐藏）
- 任一技能行点击 → `getPluginSkillDocument(entry.relative_path)` → 用 `DetailSheet`（`title` = 技能名）内嵌 `<SkillMarkdown content={...} />` 展示；读取失败显示 `t("pluginSkills.documentError")`
- 两处列表都为空时显示 `t("pluginSkills.empty")`
- 顶部两个筛选开关（`officialTitle` / `pluginsTitle`）分别控制对应块的显示/隐藏，默认都开；「个人」档由 WorkspaceView 现有列表承担，本组件只负责它的徽标文案

- [ ] **Step 4: 挂载到 `src/views/WorkspaceView.tsx`**

- 在 Claude Code 的智能体详情渲染分支中（锚点：包含 `t("globalWorkspace.addSkill")` 按钮的工具栏区块），其下方插入：
```tsx
{tool.key === "claude_code" && <PluginSkillsSection agentKey={tool.key} />}
```
- 同时在该页现有本地技能列表标题处加一枚「个人」徽标：使用既有徽标样式（参考同文件内 `globalWorkspace.localSkills.status.*` 的 badge 渲染）文案 `t("pluginSkills.personalBadge")`
- 顶部 import：`import { PluginSkillsSection } from "../components/PluginSkillsSection";`

- [ ] **Step 5: 类型检查 + Lint**

Run:
```bash
cd /d/CloudMusic/skills-manager
npm run build
npm run lint
```
Expected: 均无错误

- [ ] **Step 6: 手动验证（开发模式）**

Run: `cd /d/CloudMusic/skills-manager && npm run tauri:dev`
在打开的窗口里：Claude Code 页应出现「官方技能（19）」与「插件技能（156 个 / 分组显示）」；点开任一技能能读到文档；「打开仓库」能唤起浏览器；无「只读」以外任何写操作入口。
Expected: 全部符合；否则修复后重跑。

- [ ] **Step 7: Commit**

```bash
cd /d/CloudMusic/skills-manager
git add src/lib/tauri.ts src/components/PluginSkillsSection.tsx src/views/WorkspaceView.tsx src/i18n/en.json src/i18n/zh.json src/i18n/zh-TW.json
git commit -m "feat: show read-only plugin and official skills on the Claude Code page"
```

---

### Task 6: 端到端验收 + 推送

**Files:**
- 无代码改动（若验收发现问题，回到对应任务修）

**Interfaces:**
- Consumes: Task 1-5 全部产出
- Produces: 安装包 + 推送到 fork 的 `v1-plugin-dsh` 分支

- [ ] **Step 1: 出正式安装包**

Run: `cd /d/CloudMusic/skills-manager && npm run tauri:build`
Expected: `src-tauri/target/release/bundle/` 下产出安装包

- [ ] **Step 2: 给用户安装并逐条验收（规格书第 2 节 6 行表）**

由用户对照执行；关键判定：
1. 插件技能 156 个、带市场/插件/版本/安装日期/仓库链接
2. DSH 出现且带 3 个技能（desktop-todo / dev-expert / find-skills）
3. docx 等标「官方」；superpowers 等标「插件」且作者是第三方；DSH 的标「个人」
4. 技能库/同步/更新检查/备份正常
5. 旧数据（`C:\Users\hp\.skills-manager`）无损
6. 只读：插件技能无删除/编辑/同步入口

- [ ] **Step 3: 失败即回退路径确认**

若发现严重问题：重装官方 v1.40.2 安装包（用户 Downloads 里），数据不受影响；修好后再装。

- [ ] **Step 4: 推送分支到 fork**

```bash
cd /d/CloudMusic/skills-manager
git push -u origin v1-plugin-dsh
```
Expected: 推送到 `github.com/Acbgd11/skills-manager`（SSH 走已配置代理）

- [ ] **Step 5: 把规格书与计划入库**

```bash
cd /d/CloudMusic/skills-manager
mkdir -p docs/superpowers/specs docs/superpowers/plans
cp /d/CloudMusic/docs/superpowers/specs/2026-10-01-skills-manager-plugin-dsh-design.md docs/superpowers/specs/
cp /d/CloudMusic/docs/superpowers/plans/2026-10-01-skills-manager-plugin-dsh.md docs/superpowers/plans/
git add docs/superpowers
git commit -m "docs: add v1 plugin/dsh design spec and implementation plan"
git push
```