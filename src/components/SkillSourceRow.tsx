import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import { ExternalLink, Pencil, Check, X } from "lucide-react";
import { getSkillSource, setSkillSource } from "../lib/tauri";
import { getErrorMessage } from "../lib/error";
import { sourceSiteName } from "../utils";
import { cn } from "../utils";

/**
 * Where a skill came from. Most skills carry no source in their own files —
 * they are hand-written or came from a private channel — so this shows whatever
 * the user recorded and lets them record it.
 *
 * The link and the note are separate on purpose: only a URL can be opened, so a
 * note like "自己写的" must never be dressed up as something clickable.
 */
export function SkillSourceRow({
  skillId,
  fallbackUrl,
  className,
}: {
  skillId: string;
  /** A source already known from the skill's own file, used when the user has
   *  not recorded one. Shown as a link, not as an editable value. */
  fallbackUrl?: string | null;
  className?: string;
}) {
  const { t } = useTranslation();
  const [url, setUrl] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [editing, setEditing] = useState(false);
  const [draftUrl, setDraftUrl] = useState("");
  const [draftNote, setDraftNote] = useState("");
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let active = true;
    setLoaded(false);
    getSkillSource(skillId)
      .then((s) => {
        if (!active) return;
        setUrl(s?.url ?? null);
        setNote(s?.note ?? null);
        setLoaded(true);
      })
      .catch(() => {
        if (active) setLoaded(true);
      });
    return () => {
      active = false;
    };
  }, [skillId]);

  const beginEdit = () => {
    setDraftUrl(url ?? "");
    setDraftNote(note ?? "");
    setEditing(true);
  };

  const save = async () => {
    setSaving(true);
    try {
      await setSkillSource(skillId, draftUrl.trim(), draftNote.trim());
      setUrl(draftUrl.trim() || null);
      setNote(draftNote.trim() || null);
      setEditing(false);
    } catch (e) {
      toast.error(getErrorMessage(e, t("common.error")));
    } finally {
      setSaving(false);
    }
  };

  if (!loaded) return null;

  // What to open: the user's own link first, then one already known from the
  // skill's file. A note alone is never clickable.
  const effectiveUrl = url ?? fallbackUrl ?? null;

  if (editing) {
    return (
      <div
        className={cn(
          "flex flex-col gap-1.5 rounded-lg border border-accent-border bg-surface p-3 shadow-sm",
          className
        )}
      >
        <label className="text-[12px] font-medium text-secondary">
          {t("skillSource.urlLabel")}
        </label>
        <input
          value={draftUrl}
          onChange={(e) => setDraftUrl(e.target.value)}
          placeholder={t("skillSource.urlPlaceholder")}
          className="rounded border border-border-subtle bg-bg-secondary px-2 py-1.5 text-[12px] text-primary outline-none focus-visible:ring-2 focus-visible:ring-border"
        />
        <label className="mt-1 text-[12px] font-medium text-secondary">
          {t("skillSource.noteLabel")}
        </label>
        <input
          value={draftNote}
          onChange={(e) => setDraftNote(e.target.value)}
          placeholder={t("skillSource.notePlaceholder")}
          className="rounded border border-border-subtle bg-bg-secondary px-2 py-1.5 text-[12px] text-primary outline-none focus-visible:ring-2 focus-visible:ring-border"
        />
        <div className="mt-1.5 flex items-center gap-2">
          <button
            type="button"
            onClick={() => void save()}
            disabled={saving}
            className="inline-flex items-center gap-1 rounded-md bg-accent px-3 py-1.5 text-[12px] font-medium text-white outline-none transition-opacity hover:opacity-90 disabled:opacity-50"
          >
            <Check className="h-3 w-3" />
            {t("common.save")}
          </button>
          <button
            type="button"
            onClick={() => setEditing(false)}
            className="inline-flex items-center gap-1 rounded-md px-3 py-1.5 text-[12px] text-muted outline-none transition-colors hover:bg-surface-hover hover:text-secondary"
          >
            <X className="h-3 w-3" />
            {t("common.cancel")}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className={cn("flex flex-wrap items-center gap-2", className)}>
      <span className="text-[12px] font-medium text-muted">
        {t("skillSource.label")}
      </span>
      {effectiveUrl ? (
        <button
          type="button"
          onClick={() => void openUrl(effectiveUrl).catch(() => {})}
          title={effectiveUrl}
          className={cn(
            "inline-flex items-center gap-1.5 rounded-full border border-accent-border bg-accent-bg",
            "px-3 py-1 text-[12px] font-semibold text-accent outline-none transition-colors",
            "hover:bg-accent/20 focus-visible:ring-2 focus-visible:ring-border"
          )}
        >
          <ExternalLink className="h-3.5 w-3.5" />
          {sourceSiteName(effectiveUrl)}
        </button>
      ) : (
        <span className="rounded-full border border-dashed border-border-subtle px-3 py-1 text-[12px] text-faint">
          {t("skillSource.none")}
        </span>
      )}
      {note ? (
        <span className="text-[12px] italic text-secondary">{note}</span>
      ) : null}
      <button
        type="button"
        onClick={beginEdit}
        className="inline-flex items-center gap-1 rounded-md px-2 py-1 text-[12px] text-muted outline-none transition-colors hover:bg-surface-hover hover:text-secondary focus-visible:ring-2 focus-visible:ring-border"
      >
        <Pencil className="h-3 w-3" />
        {t("skillSource.edit")}
      </button>
    </div>
  );
}
