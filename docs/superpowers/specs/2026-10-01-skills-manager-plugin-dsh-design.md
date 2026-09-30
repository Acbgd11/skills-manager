# Skills Manager 改造设计稿 — 子项目甲（插件识别 + DSH + 来源分类）

日期：2026-10-01
状态：待用户审阅
上游：`xingkongliang/skills-manager` v1.40.2（MIT）
本机工作副本（计划）：`D:\CloudMusic\skills-manager`，改动分支 `v1-plugin-dsh`

---

## 1. 背景与目标

用户日常用两套智能体：Claude Code 桌面端（20 个插件、插件内 156 个 skill）与 DeepSeek Harness（3 个 skill）。已装 Skills Manager v1.40.2 用于管理 skill，但：

1. 不认识 DeepSeek Harness；
2. 看不到插件里带的 skill（156 个，占用户技能的绝大多数）；
3. 所有 skill 混在一起，分不清「官方自带」和「额外安装」。

本子项目目标：让 Skills Manager ①认 DSH ②看见插件技能（只读）③按来源分三档 ④顺手带上现成的元数据（安装时间、来源链接、已装版本号）。

**明确不做**（留给后续子项目）：
- UI 风格重做（子项目乙）
- MCP + AI 增强（子项目丙）：双语标题、一句中文解释、更新日志总结；以及原有的"让智能体指挥本软件"
- 联网信息（子项目丁）：上游新版检查、"客观口碑"（⭐星数 / 最后更新日 / 维护者）+「去搜评价」按钮
- 逐技能的"网友评论"：**不做**（此数据不存在；以客观口碑替代，见 3.6）
- 插件的删改/同步（只读展示；插件归 Claude 自己的系统管理）
- 不动原有的同步、备份、Git 逻辑

## 2. 验收标准

| # | 检查项 | 通过标准 |
|---|---|---|
| 1 | 零改动可编译 | 未改一行代码，能编译出安装包并正常启动（"环境关"） |
| 2 | 插件技能 | Claude Code 页出现全部插件技能（本机现有 156 个），每条带「市场 / 插件 / 版本」来源信息，可搜索、可看内容，**不可删改同步** |
| 3 | DSH | 智能体列表出现 "DeepSeek Harness"；`DSH_HOME` 环境变量存在时以其为准，否则回退 `~/.dsh`；其 `skills/` 下 3 个技能出现且可正常同步 |
| 4 | 三档来源 | 「官方」（docx 等，来自 Anthropic 官方技能市场）／「插件」（含第三方标注，如 superpowers）／「个人」（skills 目录自有技能）标签正确 |
| 5 | 老功能不坏 | 技能库、同步、更新检查、备份照常工作 |
| 6 | 元数据（现成数据） | 每个插件技能可见：**安装时间（精确到年月日）、来源链接、已装版本号**；数值与 `installed_plugins.json` / `plugin.json` 一致，链接可点击跳转 |

## 3. 技术设计

### 3.1 环境与工程

- 工具链：Rust（rustup，MSVC target）+ VS 2022 Build Tools（VCTools 工作负载）+ Node/pnpm。下载编译期间走 Clash 代理（127.0.0.1:7897）。
- 网络：`C:\Users\hp\.ssh\config` 增加 github.com 走 `connect.exe` 代理通道（已加，实测认证成功）。
- 仓库：GitHub fork（Acbgd11），本地克隆至 `D:\CloudMusic\skills-manager`；`main` 与上游保持一致（保留合并上游更新的通道），改动全部在 `v1-plugin-dsh` 分支。
- 交付：编译出 NSIS 安装包给用户安装试用；满意后推送分支到 fork。回退方案：重装官方 v1.40.2 安装包（已在用户 Downloads）。

### 3.2 改动一：DSH 一等公民适配器

在 `src-tauri/src/core/tool_adapters.rs` 的 `default_tool_adapters()` 新增：

- `key: "deepseek_harness"`，`display_name: "DeepSeek Harness"`，分类 Coding
- **技能根目录来自环境变量 `DSH_HOME`**（本机 = `D:\DSH-Official\DSH-Home`），技能目录 = `$DSH_HOME/skills`；变量缺失时回退 `~/.dsh/skills`
- 需要给 `ToolAdapter` 增加一个小字段（如 `home_env_var: Option<String>`）并在路径解析（`candidate_paths`）中支持：override > 环境变量根目录 + 相对路径 > 家目录 + 相对路径
- "已安装"判定：`$DSH_HOME`（或 `~/.dsh`）存在即视为已安装
- DSH 的 npm 插件目录（cordis 体系）**不扫**——它们不是 skill 容器

### 3.3 改动二：插件技能扫描（只读）

**扫描目标**：`<Claude 配置目录>/plugins/cache/<市场>/<插件>/<版本>/skills/<技能>/SKILL.md`
（本机配置目录 = `E:\Claude-Code\Config`，取自 `CLAUDE_CONFIG_DIR`；未设置时用 `~/.claude`）

- 只扫 **已安装**（cache）；不扫 `plugins/marketplaces/` 里的待装品
- 同一插件存在多个版本目录时取最高版本
- 产出一条技能记录时附带来源元数据：`{ kind: "plugin", marketplace, plugin, version }`
- **现成元数据直接读取（不做 AI 生成）**：
  - 安装时间 / 最后更新：`<配置目录>/plugins/installed_plugins.json` 的 `installedAt` / `lastUpdated`，界面展示到**年月日**
  - 来源链接：插件目录内 `.claude-plugin/plugin.json` 的 `homepage` / `repository`
  - 已装版本：`installed_plugins.json` 的 `version`（缺失时回退取 cache 目录名）
- **实时读取，不入库**：仿照现有「关联工作区」的做法（`read_linked_workspace_skills` 每实时读盘），插件技能只在其所属智能体页面实时展示；因此插件升级换版本目录后自动跟随，无需迁移数据
- 只读：不能部署/删除/同步/编辑
- 容错：目录不存在或结构不符 → 返回空列表，不报错、不崩溃

### 3.4 改动三：来源三档分类

| 档位 | 判定规则（按来源目录） | 可管理性 |
|---|---|---|
| 官方 | Claude 官方技能市场根目录 `plugins/marketplaces/anthropic-agent-skills/skills/*`（docx/pdf 等所在，已核实） | 只读展示（同插件技能） |
| 插件 | 3.3 扫描所得；界面可展开看市场与作者（Anthropic 官方出品 / 第三方） | 只读 |
| 个人 | 各智能体 skills 目录中不属上述两类的技能（现有行为） | 可管理（维持现状） |

界面：在技能列表加「来源」筛选/分组与标签；不改整体布局风格。

> 实现前先核实一次：「官方」档的来源目录需与桌面端实际加载的 `anthropic-skills:` 技能清单逐一比对（以桌面端实际加载为准；若存在出入，以实际加载的来源目录为判定规则并同步更新本节）。

### 3.5 数据结构影响

- 复用现有 `ToolAdapter`（+1 字段）与模拟「关联工作区」的实时读取通道，**尽量不新增数据表**；如现有技能记录结构需要来源字段，仅加只读字段，不动原字段语义
- 现有测试 `claude_code_does_not_scan_plugin_marketplaces_by_default` 的预期行为将被**有意改变**（替换为新行为的测试），在实现说明中明示

### 3.6 数据来源原则与后续子项目边界

界面信息原则：**要么是可核实的事实（可溯源），要么是明确标注"AI 生成"的解释**；不引入来源不明的信息（"网友评论"因无数据源且质量不可控，不做）。

后续子项目（本稿仅登记边界，不在本次范围）：

- **丙（MCP + AI）**：MCP 服务器让智能体读取/控制本软件；双语标题、一句中文解释、更新日志总结——由 AI 读内容生成后写回本地缓存（按技能内容哈希缓存，内容变化才重新生成）；软件本身不内置 AI、不需配 key
- **丁（联网信息）**：上游新版检查；客观口碑（GitHub ⭐星数 / 最后更新日 / 维护者）+「去搜评价」按钮
- **乙（UI 换脸）**：统一呈现上述全部信息

## 4. 测试计划

自动化（Rust 单测，跟随现有风格）：

1. 插件扫描：在临时目录构造 `cache/<市场>/<插件>/<版本>/skills/<技能>/SKILL.md` 结构 → 扫出技能且来源元数据正确；多版本取最高；空目录不崩
2. DSH 适配器：设 `DSH_HOME` 时路径正确；未设时回退 `~/.dsh/skills`；目录存在判定"已安装"
3. 来源分类：三类来源各归其档；官方目录与插件目录互不混淆
4. 元数据读取：从构造的 `installed_plugins.json` 正确取出安装时间/版本；`plugin.json` 缺 `homepage`/`repository` 字段时不崩、链接为空

手工验收（用户执行，见第 2 节表格）：安装新构建 → 对照 5 项逐一确认。

## 5. 风险与退路

| 风险 | 应对 |
|---|---|
| 首次编译环境失败（最险） | 第一步先零改动编译验证（"环境关"），失败先修环境再谈改动 |
| 上游作者快速更新 | `main` 保真、改动隔离于分支，可随时合并上游；改动集中在少量文件 |
| 插件目录结构未来变化 | 扫描器容错为空列表；结构变化时单独修扫描器 |
| 用户数据受损 | 全部改动不写用户数据；插件技能只读；退路 = 重装官方 v1.40.2 |

## 6. 交付物

1. 可安装的 Windows 安装包（本次改造版）
2. 代码：`v1-plugin-dsh` 分支推送至用户 fork
3. 本设计稿与实施计划随仓库入库（克隆后移入 `docs/superpowers/specs/`）