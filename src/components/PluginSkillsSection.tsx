import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import {
  ChevronDown,
  ChevronRight,
  ExternalLink,
  FolderOpen,
  Loader2,
  Package,
  Search,
  ShieldCheck,
} from "lucide-react";
import { cn, sourceSiteName } from "../utils";
import { DetailSheet } from "./DetailSheet";
import { SkillMarkdown } from "./SkillMarkdown";
import {
  getClaudePluginSkills,
  getPluginSkillDocument,
  revealPluginSkillFolder,
  type PluginSkillEntry,
  type PluginSkillGroup,
  type PluginSkillsDto,
} from "../lib/tauri";
import { getErrorMessage } from "../lib/error";

interface PluginSkillsSectionProps {
  agentKey: string;
}

interface DocState {
  open: boolean;
  path: string;
  name: string;
  content: string | null;
  error: boolean;
  /** GitHub URL of the plugin this skill ships with, when the plugin has one. */
  repoUrl: string | null;
}

const BADGE_READONLY = "bg-amber-500/10 text-amber-700 dark:text-amber-300";

export function PluginSkillsSection({ agentKey }: PluginSkillsSectionProps) {
  const { t } = useTranslation();
  const [data, setData] = useState<PluginSkillsDto | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showOfficial, setShowOfficial] = useState(true);
  const [showPlugins, setShowPlugins] = useState(true);
  // Plugin groups default to collapsed (empty = all collapsed); user expands on click.
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [query, setQuery] = useState("");
  const [doc, setDoc] = useState<DocState>({
    open: false,
    path: "",
    name: "",
    content: null,
    error: false,
    repoUrl: null,
  });

  const requestRef = useRef(0);
  const load = useCallback(async () => {
    const requestId = ++requestRef.current;
    setLoading(true);
    setError(null);
    try {
      const dto = await getClaudePluginSkills();
      if (requestRef.current === requestId) setData(dto);
    } catch (e) {
      if (requestRef.current === requestId) {
        setError(getErrorMessage(e, t("pluginSkills.loadError")));
      }
    } finally {
      if (requestRef.current === requestId) setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    if (agentKey !== "claude_code") return;
    void load();
    return () => {
      requestRef.current += 1;
    };
  }, [agentKey, load]);

  // The page-level translate button reloads its own lists; this section holds
  // separate data, so it needs the same cue to pick up fresh Chinese rows.
  useEffect(() => {
    if (agentKey !== "claude_code") return;
    const onUpdated = () => void load();
    window.addEventListener("translations-updated", onUpdated);
    return () => window.removeEventListener("translations-updated", onUpdated);
  }, [agentKey, load]);

  // F4: filter by query across name, description, and plugin (group name).
  // The translated Chinese name/description are matched too — they are what the
  // card actually shows once translated, so searching for them has to work.
  // When a query is active, matching entries auto-expand their containing group.
  /// Official skills all ship from one marketplace repo, so they share a link.
  const officialRepoUrl = data?.official_repository ?? null;

  const normalizedQuery = query.trim().toLowerCase();
  const matches = useCallback(
    (entry: PluginSkillEntry, group?: PluginSkillGroup) => {
      if (!normalizedQuery) return true;
      const haystack = [
        entry.name,
        entry.description ?? "",
        entry.zh_name ?? "",
        entry.zh_description ?? "",
        group?.plugin ?? "",
        group?.zh_name ?? "",
        group?.zh_description ?? "",
      ];
      return haystack.some((field) => field.toLowerCase().includes(normalizedQuery));
    },
    [normalizedQuery]
  );

  const filteredOfficial = useMemo(() => {
    if (!data) return [];
    return data.official.filter((e) => matches(e));
  }, [data, matches]);

  const filteredGroups = useMemo(() => {
    if (!data) return [];
    return data.groups
      .map((g) => ({
        ...g,
        skills: g.skills.filter((e) => matches(e, g)),
      }))
      .filter(
        (g) =>
          g.skills.length > 0 ||
          g.plugin.toLowerCase().includes(normalizedQuery) ||
          (g.zh_name ?? "").toLowerCase().includes(normalizedQuery) ||
          (g.zh_description ?? "").toLowerCase().includes(normalizedQuery)
      );
  }, [data, matches, normalizedQuery]);

  // When query is non-empty, auto-expand all groups that have matching skills.
  const effectiveExpanded = useMemo(() => {
    if (!normalizedQuery) return expanded;
    // During search, force-expand every group (even ones the user collapsed).
    const allExpanded: Record<string, boolean> = {};
    for (const g of data?.groups ?? []) {
      allExpanded[`${g.marketplace}/${g.plugin}`] = true;
    }
    return allExpanded;
  }, [expanded, normalizedQuery, data]);

  if (agentKey !== "claude_code") return null;

  if (loading) {
    return (
      <div className="flex items-center gap-2 px-4 py-3 text-[13px] text-muted">
        <Loader2 className="h-3.5 w-3.5 animate-spin" />
        {t("pluginSkills.loading")}
      </div>
    );
  }

  if (error) {
    return (
      <div className="px-4 py-3 text-[13px] text-muted">{error}</div>
    );
  }

  if (!data) return null;

  const hasAnything = filteredGroups.length > 0 || filteredOfficial.length > 0;

  const openDoc = (entry: PluginSkillEntry, repoUrl: string | null = null) => {
    setDoc({
      open: true,
      path: entry.relative_path,
      name: entry.name,
      content: null,
      error: false,
      repoUrl,
    });
    getPluginSkillDocument(entry.relative_path)
      .then((res) => {
        setDoc((prev) =>
          prev.open && prev.path === entry.relative_path
            ? { ...prev, content: res.content }
            : prev
        );
      })
      .catch(() => {
        setDoc((prev) =>
          prev.open && prev.path === entry.relative_path
            ? { ...prev, error: true }
            : prev
        );
      });
  };

  const closeDoc = () =>
    setDoc({ open: false, path: "", name: "", content: null, error: false, repoUrl: null });

  const toggleGroup = (key: string) =>
    setExpanded((prev) => ({ ...prev, [key]: !prev[key] }));

  // Rows are divs (not nested buttons) so the reveal action can live inside.
  const revealFolderButton = (entry: PluginSkillEntry) => (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation();
        revealPluginSkillFolder(entry.relative_path).catch((err: unknown) => {
          toast.error(getErrorMessage(err, t("common.error")));
        });
      }}
      className="mt-0.5 inline-flex h-6 w-6 shrink-0 items-center justify-center rounded text-muted outline-none transition-colors hover:bg-surface-hover hover:text-secondary focus-visible:ring-2 focus-visible:ring-border"
      title={t("common.openFolder")}
      aria-label={t("common.openFolder")}
    >
      <FolderOpen className="h-3.5 w-3.5" />
    </button>
  );

  /** GitHub link, or a plain "no GitHub" note when there is none to open. */
  const RepoLinkCell = ({ repoUrl }: { repoUrl: string | null }) =>
    repoUrl ? (
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          void openUrl(repoUrl).catch(() => {});
        }}
        className="mt-0.5 inline-flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-[12px] text-muted outline-none transition-colors hover:bg-surface-hover hover:text-secondary focus-visible:ring-2 focus-visible:ring-border"
        title={repoUrl}
        aria-label={t("pluginSkills.openRepo")}
      >
        <ExternalLink className="h-3 w-3" />
        GitHub
      </button>
    ) : (
      <span
        className="mt-0.5 shrink-0 px-1.5 py-0.5 text-[12px] text-faint"
        title={t("pluginSkills.noRepoHint")}
      >
        {t("pluginSkills.noRepo")}
      </span>
    );

  const renderOfficialRow = (entry: PluginSkillEntry, repoUrl: string | null) => (
    <div
      key={`official:${entry.relative_path}`}
      role="button"
      tabIndex={0}
      onClick={() => openDoc(entry, repoUrl)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          openDoc(entry, repoUrl);
        }
      }}
      className="flex w-full cursor-pointer items-start gap-2 rounded-md px-3 py-2 text-left transition-colors hover:bg-surface-hover outline-none focus-visible:ring-2 focus-visible:ring-border"
    >
      <ShieldCheck className="mt-0.5 h-3.5 w-3.5 shrink-0 text-muted" />
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[13px] font-medium text-secondary">
          {entry.name}
        </span>
        {entry.description ? (
          <span className="block truncate text-[12px] text-muted">
            {entry.description}
          </span>
        ) : null}
        {entry.zh_name ? (
          <div className="mt-0.5 text-[12px] text-secondary">
            {entry.zh_name}
            {entry.zh_description ? (
              <span className="text-muted"> — {entry.zh_description}</span>
            ) : null}
          </div>
        ) : null}
      </span>
      {revealFolderButton(entry)}
      <RepoLinkCell repoUrl={repoUrl} />
    </div>
  );

  // A skill inside a plugin has no repository of its own — it ships with the
  // plugin, so it links to the same repo / shows the same "no GitHub" note.
  const renderPluginRow = (entry: PluginSkillEntry, repoUrl: string | null) => (
    <div
      key={`plugin:${entry.relative_path}`}
      role="button"
      tabIndex={0}
      onClick={() => openDoc(entry, repoUrl)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          openDoc(entry, repoUrl);
        }
      }}
      className="flex w-full cursor-pointer items-start gap-2 rounded-md px-3 py-2 pl-9 text-left transition-colors hover:bg-surface-hover outline-none focus-visible:ring-2 focus-visible:ring-border"
    >
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[13px] font-medium text-secondary">
          {entry.name}
        </span>
        {entry.description ? (
          <span className="block truncate text-[12px] text-muted">
            {entry.description}
          </span>
        ) : null}
        {entry.zh_name ? (
          <div className="mt-0.5 text-[12px] text-secondary">
            {entry.zh_name}
            {entry.zh_description ? (
              <span className="text-muted"> — {entry.zh_description}</span>
            ) : null}
          </div>
        ) : null}
      </span>
      {revealFolderButton(entry)}
      <RepoLinkCell repoUrl={repoUrl} />
    </div>
  );

  const metaLine = (group: PluginSkillGroup) => {
    const parts: string[] = [];
    if (group.marketplace) {
      parts.push(`${t("pluginSkills.marketplace")}: ${group.marketplace}`);
    }
    if (group.version) {
      parts.push(`${t("pluginSkills.version")}: ${group.version}`);
    }
    if (group.installed_at) {
      parts.push(`${t("pluginSkills.installedAt")} ${group.installed_at}`);
    }
    if (group.last_updated) {
      parts.push(`${t("pluginSkills.updatedAt")} ${group.last_updated}`);
    }
    return parts.join(" · ");
  };

  return (
    <section className="mt-4 flex flex-col gap-3">
      <div className="flex items-center justify-between gap-2">
        <h2 className="flex items-center gap-2 text-[14px] font-semibold text-primary">
          <Package className="h-4 w-4 text-muted" />
          {t("pluginSkills.title")}
        </h2>
        {/* Translation is a page-level action: WorkspaceView's toolbar owns the
            single button. A second copy here sat next to it and let both start
            the same job. */}
      </div>

      {!hasAnything && !normalizedQuery ? (
        <p className="px-1 text-[13px] text-muted">{t("pluginSkills.empty")}</p>
      ) : (
        <div className="flex flex-col gap-3">
          {/* Search input */}
          <div className="flex items-center gap-1.5 rounded-lg border border-border-subtle bg-background px-2.5 py-1.5">
            <Search className="h-3.5 w-3.5 shrink-0 text-muted" />
            <input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t("pluginSkills.searchPlaceholder")}
              className="min-w-0 flex-1 bg-transparent text-[13px] text-secondary outline-none placeholder:text-faint"
            />
          </div>

          {/* Official skills block */}
          {filteredOfficial.length > 0 ? (
            <div className="rounded-xl border border-border-subtle bg-bg-secondary">
              <div className="flex items-center justify-between gap-2 px-3 py-2">
                <button
                  type="button"
                  onClick={() => setShowOfficial((v) => !v)}
                  className="inline-flex items-center gap-1.5 text-[13px] font-medium text-secondary outline-none focus-visible:ring-2 focus-visible:ring-border rounded"
                >
                  {showOfficial ? (
                    <ChevronDown className="h-3.5 w-3.5" />
                  ) : (
                    <ChevronRight className="h-3.5 w-3.5" />
                  )}
                  {t("pluginSkills.officialTitle")}
                  <span className="app-badge">{filteredOfficial.length}</span>
                </button>
              </div>
              {showOfficial ? (
                <div className="flex flex-col pb-1">
                  {filteredOfficial.map((entry) => renderOfficialRow(entry, officialRepoUrl))}
                </div>
              ) : null}
            </div>
          ) : null}

          {/* Plugin skills block */}
          {filteredGroups.length > 0 ? (
            <div className="rounded-xl border border-border-subtle bg-bg-secondary">
              <div className="flex items-center justify-between gap-2 px-3 py-2">
                <button
                  type="button"
                  onClick={() => setShowPlugins((v) => !v)}
                  className="inline-flex items-center gap-1.5 text-[13px] font-medium text-secondary outline-none focus-visible:ring-2 focus-visible:ring-border rounded"
                >
                  {showPlugins ? (
                    <ChevronDown className="h-3.5 w-3.5" />
                  ) : (
                    <ChevronRight className="h-3.5 w-3.5" />
                  )}
                  {t("pluginSkills.pluginsTitle")}
                  <span className="app-badge">{filteredGroups.length}</span>
                </button>
              </div>
              {showPlugins ? (
                <div className="flex flex-col gap-1 pb-1">
                  {filteredGroups.map((group) => {
                    const gkey = `${group.marketplace}/${group.plugin}`;
                    const isExpanded = effectiveExpanded[gkey] ?? false;
                    const repoUrl = group.repository ?? group.homepage;
                    return (
                      <div key={gkey} className="px-1">
                        <div className="flex items-start gap-2 rounded-md px-2 py-1.5">
                          <button
                            type="button"
                            onClick={() => toggleGroup(gkey)}
                            className="mt-0.5 inline-flex shrink-0 items-center text-muted outline-none focus-visible:ring-2 focus-visible:ring-border rounded"
                          >
                            {isExpanded ? (
                              <ChevronDown className="h-3.5 w-3.5" />
                            ) : (
                              <ChevronRight className="h-3.5 w-3.5" />
                            )}
                          </button>
                          <div className="min-w-0 flex-1">
                            <div className="flex flex-wrap items-center gap-1.5">
                              <span className="truncate text-[13px] font-medium text-secondary">
                                {group.plugin}
                              </span>
                              {group.author ? (
                                <span className="shrink-0 text-[12px] text-muted">
                                  {t("pluginSkills.author")}: {group.author}
                                </span>
                              ) : null}
                              <span
                                className={cn(
                                  "shrink-0 rounded-full px-2 py-0.5 text-[12px] font-medium",
                                  BADGE_READONLY
                                )}
                              >
                                {t("pluginSkills.readOnly")}
                              </span>
                            </div>
                            {group.description ? (
                              <p className="mt-0.5 truncate text-[12px] text-muted">
                                {group.description}
                              </p>
                            ) : null}
                            {group.zh_description ? (
                              <p className="mt-0.5 truncate text-[12px] text-secondary">
                                {group.zh_description}
                              </p>
                            ) : null}
                            <p className="mt-0.5 text-[12px] text-faint">
                              {metaLine(group)}
                            </p>
                          </div>
                          {repoUrl ? (
                            <button
                              type="button"
                              onClick={() => {
                                void openUrl(repoUrl).catch(() => {});
                              }}
                              className="shrink-0 inline-flex items-center gap-1 rounded-md px-2 py-1 text-[12px] text-muted transition-colors hover:bg-surface-hover hover:text-secondary outline-none focus-visible:ring-2 focus-visible:ring-border"
                              title={t("pluginSkills.openRepo")}
                            >
                              <ExternalLink className="h-3 w-3" />
                              {t("pluginSkills.openRepo")}
                            </button>
                          ) : (
                            <span className="shrink-0 px-2 py-1 text-[12px] text-faint">
                              {t("pluginSkills.noRepo")}
                            </span>
                          )}
                        </div>
                        {isExpanded && group.skills.length > 0 ? (
                          <div className="flex flex-col pb-1">
                            {group.skills.map((entry) => renderPluginRow(entry, repoUrl))}
                          </div>
                        ) : null}
                      </div>
                    );
                  })}
                </div>
              ) : null}
            </div>
          ) : null}
        </div>
      )}

      <DetailSheet
        open={doc.open}
        title={doc.name}
        onClose={closeDoc}
        meta={
          <div className="flex flex-wrap items-center justify-end gap-2">
            <span className="text-[12px] font-medium text-muted">
              {t("skillSource.label")}
            </span>
            {doc.repoUrl ? (
              <button
                type="button"
                onClick={() => void openUrl(doc.repoUrl!).catch(() => {})}
                title={doc.repoUrl}
                className="inline-flex items-center gap-1.5 rounded-full border border-accent-border bg-accent-bg px-3 py-1 text-[12px] font-semibold text-accent outline-none transition-colors hover:border-accent focus-visible:ring-2 focus-visible:ring-border"
              >
                <ExternalLink className="h-3.5 w-3.5" />
                {sourceSiteName(doc.repoUrl)}
              </button>
            ) : (
              <span className="rounded-full border border-dashed border-border-subtle px-3 py-1 text-[12px] text-faint">
                {t("skillSource.none")}
              </span>
            )}
          </div>
        }
      >
        {doc.error ? (
          <p className="text-[13px] text-muted">
            {t("pluginSkills.documentError")}
          </p>
        ) : doc.content === null ? (
          <div className="flex items-center gap-2 text-[13px] text-muted">
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
            {t("pluginSkills.loading")}
          </div>
        ) : (
          <SkillMarkdown content={doc.content} />
        )}
      </DetailSheet>
    </section>
  );
}
