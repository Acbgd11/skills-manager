import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { Loader2 } from "lucide-react";
import * as api from "../lib/tauri";
import { getErrorMessage } from "../lib/error";
import { SkillMarkdown } from "./SkillMarkdown";

/**
 * The 中文 tab of a skill: the document body translated on demand.
 *
 * Shared by every skill detail surface so the behaviour cannot drift between
 * them. The backend caches by content hash, so opening the same skill again
 * costs nothing; a cached translation is loaded without spending a call.
 */
export function SkillBodyZh({
  skillId,
  content,
  onShowOriginal,
}: {
  skillId: string;
  /** The original body, which is also the cache key. Null while it loads. */
  content: string | null;
  /** Offered as a shortcut back to the original when provided. */
  onShowOriginal?: () => void;
}) {
  const { t } = useTranslation();
  const [zh, setZh] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [errorText, setErrorText] = useState<string | null>(null);
  const [chunk, setChunk] = useState<{ done: number; total: number } | null>(null);
  const requestRef = useRef(0);

  // Reset whenever a different skill (or a different body) is shown.
  useEffect(() => {
    setZh(null);
    setErrorText(null);
    setChunk(null);
    setLoading(false);
  }, [skillId, content]);

  // Long bodies translate in chunks; show which one is in flight.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let active = true;
    listen<{ done: number; total: number }>("body-translation-progress", (e) => {
      if (active) setChunk(e.payload);
    })
      .then((fn) => {
        if (active) unlisten = fn;
        else fn();
      })
      .catch(() => {});
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  // Show an already-cached translation without spending a model call.
  useEffect(() => {
    if (!content) return;
    let active = true;
    const requestId = ++requestRef.current;
    api
      .getCachedBodyTranslation(content)
      .then((cached) => {
        if (active && requestRef.current === requestId) setZh(cached);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [content]);

  const translate = useCallback(async () => {
    if (!content || loading) return;
    const requestId = ++requestRef.current;
    setLoading(true);
    setErrorText(null);
    try {
      const result = await api.translateSkillBody(content, skillId);
      if (requestRef.current !== requestId) return;
      if (result) {
        setZh(result);
      } else {
        setErrorText(t("translation.bodyNotTranslated"));
      }
    } catch (e) {
      if (requestRef.current === requestId) {
        // The backend's own wording distinguishes a cut-off reply from a
        // connection problem; the user needs that difference.
        setErrorText(getErrorMessage(e, t("translation.bodyFailed")));
      }
    } finally {
      if (requestRef.current === requestId) setLoading(false);
    }
  }, [content, skillId, loading, t]);

  if (loading) {
    return (
      <div className="mt-12 flex items-center justify-center gap-2 text-center text-[13px] text-muted">
        <Loader2 className="h-3.5 w-3.5 animate-spin" />
        {chunk && chunk.total > 1
          ? t("translation.translatingBodyChunk", {
              done: Math.min(chunk.done + 1, chunk.total),
              total: chunk.total,
            })
          : t("translation.translatingBody")}
      </div>
    );
  }

  if (zh) {
    return (
      <div>
        <div className="mb-3 flex flex-wrap items-center gap-2">
          <span className="text-[12px] text-muted">{t("translation.bodyDisclaimer")}</span>
          {onShowOriginal ? (
            <button
              type="button"
              onClick={onShowOriginal}
              className="rounded-md border border-border-subtle bg-bg-secondary px-2.5 py-1 text-[12px] text-secondary transition-colors hover:bg-surface-hover outline-none"
            >
              {t("translation.viewOriginal")}
            </button>
          ) : null}
        </div>
        <SkillMarkdown content={zh} />
      </div>
    );
  }

  return (
    <div className="mt-12 flex flex-col items-center gap-3 text-center">
      <p className="text-[13px] text-muted">
        {errorText ?? t("translation.bodyNotTranslated")}
      </p>
      <button
        type="button"
        onClick={() => void translate()}
        disabled={!content}
        className="rounded-md border border-border-subtle bg-bg-secondary px-3 py-1.5 text-[12px] text-secondary transition-colors hover:bg-surface-hover outline-none disabled:opacity-50"
      >
        {t("translation.translate")}
      </button>
    </div>
  );
}
