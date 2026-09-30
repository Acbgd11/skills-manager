import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ChevronDown,
  ChevronRight,
  ExternalLink,
  Loader2,
  Package,
  ShieldCheck,
} from "lucide-react";
import { cn } from "../utils";
import { DetailSheet } from "./DetailSheet";
import { SkillMarkdown } from "./SkillMarkdown";
import {
  getClaudePluginSkills,
  getPluginSkillDocument,
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
  name: string;
  content: string | null;
  error: boolean;
}

const BADGE_READONLY = "bg-amber-500/10 text-amber-700 dark:text-amber-300";

export function PluginSkillsSection({ agentKey }: PluginSkillsSectionProps) {
  const { t } = useTranslation();
  const [data, setData] = useState<PluginSkillsDto | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showOfficial, setShowOfficial] = useState(true);
  const [showPlugins, setShowPlugins] = useState(true);
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const [doc, setDoc] = useState<DocState>({
    open: false,
    name: "",
    content: null,
    error: false,
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
        setError(getErrorMessage(e, t("pluginSkills.documentError")));
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

  const groups = data.groups;
  const official = data.official;
  const hasAnything = groups.length > 0 || official.length > 0;

  const openDoc = (entry: PluginSkillEntry) => {
    setDoc({ open: true, name: entry.name, content: null, error: false });
    getPluginSkillDocument(entry.relative_path)
      .then((res) => {
        setDoc((prev) =>
          prev.open && prev.name === entry.name
            ? { ...prev, content: res.content }
            : prev
        );
      })
      .catch(() => {
        setDoc((prev) =>
          prev.open && prev.name === entry.name
            ? { ...prev, error: true }
            : prev
        );
      });
  };

  const closeDoc = () =>
    setDoc({ open: false, name: "", content: null, error: false });

  const toggleGroup = (key: string) =>
    setCollapsed((prev) => ({ ...prev, [key]: !prev[key] }));

  const renderOfficialRow = (entry: PluginSkillEntry) => (
    <button
      key={`official:${entry.relative_path}`}
      type="button"
      onClick={() => openDoc(entry)}
      className="flex w-full items-start gap-2 rounded-md px-3 py-2 text-left transition-colors hover:bg-surface-hover outline-none focus-visible:ring-2 focus-visible:ring-border"
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
      </span>
    </button>
  );

  const renderPluginRow = (entry: PluginSkillEntry) => (
    <button
      key={`plugin:${entry.relative_path}`}
      type="button"
      onClick={() => openDoc(entry)}
      className="flex w-full items-start gap-2 rounded-md px-3 py-2 pl-9 text-left transition-colors hover:bg-surface-hover outline-none focus-visible:ring-2 focus-visible:ring-border"
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
      </span>
    </button>
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
      <h2 className="flex items-center gap-2 text-[14px] font-semibold text-primary">
        <Package className="h-4 w-4 text-muted" />
        {t("pluginSkills.title")}
        {hasAnything ? null : null}
      </h2>

      {!hasAnything ? (
        <p className="px-1 text-[13px] text-muted">{t("pluginSkills.empty")}</p>
      ) : (
        <div className="flex flex-col gap-3">
          {/* Official skills block */}
          {official.length > 0 ? (
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
                  <span className="app-badge">{official.length}</span>
                </button>
              </div>
              {showOfficial ? (
                <div className="flex flex-col pb-1">
                  {official.map(renderOfficialRow)}
                </div>
              ) : null}
            </div>
          ) : null}

          {/* Plugin skills block */}
          {groups.length > 0 ? (
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
                  <span className="app-badge">{groups.length}</span>
                </button>
              </div>
              {showPlugins ? (
                <div className="flex flex-col gap-1 pb-1">
                  {groups.map((group) => {
                    const gkey = `${group.marketplace}/${group.plugin}`;
                    const isCollapsed = collapsed[gkey] ?? false;
                    const repoUrl = group.repository ?? group.homepage;
                    return (
                      <div key={gkey} className="px-1">
                        <div className="flex items-start gap-2 rounded-md px-2 py-1.5">
                          <button
                            type="button"
                            onClick={() => toggleGroup(gkey)}
                            className="mt-0.5 inline-flex shrink-0 items-center text-muted outline-none focus-visible:ring-2 focus-visible:ring-border rounded"
                          >
                            {isCollapsed ? (
                              <ChevronRight className="h-3.5 w-3.5" />
                            ) : (
                              <ChevronDown className="h-3.5 w-3.5" />
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
                          ) : null}
                        </div>
                        {!isCollapsed && group.skills.length > 0 ? (
                          <div className="flex flex-col pb-1">
                            {group.skills.map(renderPluginRow)}
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
