import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { Languages, Loader2 } from "lucide-react";
import {
  getTranslationSettings,
  getTranslationStatus,
  translateSkills,
  type TranslationSettings,
  type TranslationStatus,
} from "../lib/tauri";
import { getErrorMessage } from "../lib/error";

/**
 * Shared one-click translation control. Scope is always GLOBAL — clicking it
 * anywhere translates everything still untranslated (plugin skills, official
 * skills, plugin-group descriptions, and every installed+enabled agent's local
 * skills). The caller's `onDone` is invoked after a successful run so the page
 * can refresh its own list and show the freshly-cached Chinese rows.
 *
 * Renders three states, mirroring the original PluginSkillsSection control:
 *   ① not configured → "Set up translation" (navigates to /settings)
 *   ② pending > 0     → "Translate" + pending badge
 *   ③ all translated  → "All translated" (disabled confirmation)
 * plus an in-flight progress state driven by the `translation-progress` event.
 */
export function TranslateButton({ onDone }: { onDone?: () => void | Promise<void> }) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [trStatus, setTrStatus] = useState<TranslationStatus | null>(null);
  const [trSettings, setTrSettings] = useState<TranslationSettings | null>(null);
  const [translating, setTranslating] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);

  // Fetch translation status + settings once on mount.
  useEffect(() => {
    getTranslationStatus()
      .then(setTrStatus)
      .catch(() => {});
    getTranslationSettings()
      .then(setTrSettings)
      .catch(() => {});
  }, []);

  // Listen for batch progress while a translation is in flight; unlisten on cleanup.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let active = true;
    listen<{ done: number; total: number }>("translation-progress", (event) => {
      if (!active) return;
      setProgress({ done: event.payload.done, total: event.payload.total });
    })
      .then((fn) => {
        if (!active) {
          fn();
          return;
        }
        unlisten = fn;
      })
      .catch(() => {});
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  const handleTranslate = async () => {
    if (translating) return;
    setTranslating(true);
    setProgress(null);
    try {
      const report = await translateSkills();
      if (report.translated > 0 && report.failed_batches === 0) {
        toast.success(
          t("translation.resultSummary", {
            ok: report.translated,
            failed: report.failed_batches,
          })
        );
      } else if (report.failed_batches > 0) {
        toast.error(
          t("translation.resultSummary", {
            ok: report.translated,
            failed: report.failed_batches,
          })
        );
      } else {
        toast.info(t("translation.noPending"));
      }
      // Refresh the bilingual rows and the pending counter.
      await Promise.all([
        getTranslationStatus().then(setTrStatus).catch(() => {}),
        onDone ? Promise.resolve(onDone()).catch(() => {}) : Promise.resolve(),
      ]);
    } catch (e) {
      toast.error(getErrorMessage(e, t("common.error")));
    } finally {
      setTranslating(false);
      setProgress(null);
    }
  };

  // ① Not configured → prompt the user to set up the endpoint.
  if (trSettings && (trSettings.has_key === false || trSettings.model === "")) {
    return (
      <button
        type="button"
        onClick={() => navigate("/settings")}
        className="inline-flex items-center gap-1.5 rounded-md border border-border-subtle bg-bg-secondary px-2.5 py-1 text-[12px] text-secondary transition-colors hover:bg-surface-hover outline-none focus-visible:ring-2 focus-visible:ring-border"
      >
        <Languages className="h-3.5 w-3.5" />
        {t("translation.goToSettings")}
      </button>
    );
  }

  // ② In flight → show progress driven by the `translation-progress` event.
  if (translating) {
    const done = progress?.done ?? 0;
    const total = progress?.total ?? 0;
    return (
      <span className="inline-flex items-center gap-1.5 rounded-md border border-border-subtle bg-bg-secondary px-2.5 py-1 text-[12px] text-muted">
        <Loader2 className="h-3.5 w-3.5 animate-spin" />
        {done === 0
          ? t("translation.translatingPreparing", { total })
          : t("translation.translating", { done, total })}
      </span>
    );
  }

  // ③ Pending → translate + badge.
  if (trStatus && trStatus.pending > 0) {
    return (
      <button
        type="button"
        onClick={() => void handleTranslate()}
        className="inline-flex items-center gap-1.5 rounded-md border border-border-subtle bg-bg-secondary px-2.5 py-1 text-[12px] text-secondary transition-colors hover:bg-surface-hover outline-none focus-visible:ring-2 focus-visible:ring-border"
      >
        <Languages className="h-3.5 w-3.5" />
        {t("translation.translate")}
        <span className="rounded-full bg-accent-bg px-1.5 py-0.5 text-[11px] font-medium text-accent">
          {t("translation.pendingBadge", { count: trStatus.pending })}
        </span>
      </button>
    );
  }

  // ④ All translated → disabled confirmation.
  if (trStatus && trStatus.total > 0 && trStatus.pending === 0) {
    return (
      <span className="inline-flex items-center gap-1.5 rounded-md border border-border-subtle bg-bg-secondary px-2.5 py-1 text-[12px] text-muted">
        <Languages className="h-3.5 w-3.5" />
        {t("translation.translatedAll")}
      </span>
    );
  }

  return null;
}
