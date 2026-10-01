import { useEffect, useRef, useState } from "react";
import { BookOpen, Languages, Package, RefreshCw, Search, Sparkles, Store } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useApp } from "../context/AppContext";
import * as api from "../lib/tauri";

/**
 * Setting key persisted through the existing get_settings/set_settings
 * commands. Any non-empty value means "the user dismissed the first-run
 * guide with 「下次不再显示」 checked", so the auto-open never fires again.
 * No new database table — this is a plain settings row.
 */
const ONBOARDING_DISMISSED_KEY = "onboarding_dismissed";

const CAPABILITY_ICONS = [Search, RefreshCw, Languages] as const;
const TERM_ICONS = [BookOpen, Package, Store] as const;
const CAPABILITY_KEYS = ["inventory", "sync", "translate"] as const;
const TERM_KEYS = ["skill", "plugin", "marketplace"] as const;
const AGENT_KEYS = ["claudeCode", "codex", "deepseekHarness"] as const;

/**
 * Beginner's guide (新手说明): what this app does, the skill/plugin/
 * marketplace vocabulary, and how the three agents differ. Opens by itself
 * on first launch (unless previously dismissed), and can always be re-opened
 * from Settings — there the "don't show again" checkbox is hidden, because a
 * deliberate re-open must not silently flip the auto-open flag off.
 */
export function OnboardingDialog() {
  const { t } = useTranslation();
  const { onboardingOpen, onboardingShowCheckbox, openOnboarding, closeOnboarding } = useApp();
  const [dontShowAgain, setDontShowAgain] = useState(false);
  const [saving, setSaving] = useState(false);
  // First-launch gate: runs exactly once (StrictMode mounts effects twice),
  // same shape as FirstRunRestoreDialog's one-shot check.
  const gateRanRef = useRef(false);

  useEffect(() => {
    if (gateRanRef.current) return;
    gateRanRef.current = true;
    void (async () => {
      const dismissed = await api.getSettings(ONBOARDING_DISMISSED_KEY).catch(() => null);
      if (dismissed) return;
      openOnboarding(true);
    })();
    // openOnboarding is a stable context callback; the gate must run only once.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Reset the checkbox each time the dialog opens so a checked state from a
  // previous auto-open never leaks into a later Settings re-open.
  useEffect(() => {
    if (onboardingOpen) setDontShowAgain(false);
  }, [onboardingOpen]);

  if (!onboardingOpen) return null;

  const handleDismiss = async () => {
    if (saving) return;
    setSaving(true);
    closeOnboarding();
    // Persist after closing so the dialog never lingers on a slow write.
    if (dontShowAgain) {
      await api.setSettings(ONBOARDING_DISMISSED_KEY, "true").catch(() => {});
    }
    setSaving(false);
  };

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center">
      <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" />
      {/* Same --app-scale compensation as ConfirmDialog: the text-size setting
          applies `zoom` to <html>, and zoom does not scale vh, so a bare 85vh
          renders taller than the viewport on the largest text size. */}
      <div className="relative z-10 flex max-h-[calc(85vh/var(--app-scale))] w-full max-w-xl flex-col rounded-xl border border-border bg-surface shadow-2xl">
        {/* Header */}
        <div className="shrink-0 border-b border-border-subtle px-5 pt-5 pb-4">
          <div className="flex items-start gap-3">
            <div className="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-border-subtle bg-bg-secondary">
              <Sparkles className="h-5 w-5 text-accent" />
            </div>
            <div className="min-w-0">
              <h2 className="text-[15px] font-semibold text-primary">{t("onboarding.title")}</h2>
              <p className="mt-1 text-[13px] leading-5 text-muted">{t("onboarding.subtitle")}</p>
            </div>
          </div>
        </div>

        {/* Scrollable body */}
        <div className="min-h-0 flex-1 space-y-5 overflow-y-auto px-5 py-4">
          {/* What this app does */}
          <section>
            <h3 className="text-[13px] font-semibold text-secondary">{t("onboarding.whatTitle")}</h3>
            <p className="mt-1.5 text-[13px] leading-5 text-muted">{t("onboarding.whatIntro")}</p>
            <div className="mt-2 space-y-1.5">
              {CAPABILITY_KEYS.map((key, index) => {
                const Icon = CAPABILITY_ICONS[index];
                return (
                  <div
                    key={key}
                    className="flex items-start gap-2.5 rounded-lg border border-border-subtle bg-bg-secondary px-3 py-2"
                  >
                    <Icon className="mt-0.5 h-3.5 w-3.5 shrink-0 text-accent" />
                    <p className="min-w-0 text-[13px] leading-5 text-tertiary">
                      <span className="font-medium text-secondary">
                        {t(`onboarding.what.${key}.label`)}
                      </span>
                      {" · "}
                      {t(`onboarding.what.${key}.description`)}
                    </p>
                  </div>
                );
              })}
            </div>
          </section>

          {/* The three words */}
          <section>
            <h3 className="text-[13px] font-semibold text-secondary">{t("onboarding.termsTitle")}</h3>
            <div className="mt-2 space-y-1.5">
              {TERM_KEYS.map((key, index) => {
                const Icon = TERM_ICONS[index];
                return (
                  <div
                    key={key}
                    className="flex items-start gap-2.5 rounded-lg border border-border-subtle bg-bg-secondary px-3 py-2"
                  >
                    <Icon className="mt-0.5 h-3.5 w-3.5 shrink-0 text-accent" />
                    <p className="min-w-0 text-[13px] leading-5 text-tertiary">
                      <span className="font-medium text-secondary">
                        {t(`onboarding.terms.${key}.term`)}
                      </span>
                      {" · "}
                      {t(`onboarding.terms.${key}.description`)}
                    </p>
                  </div>
                );
              })}
            </div>
            <p className="mt-2 text-[12px] leading-5 text-faint">{t("onboarding.termsNote")}</p>
          </section>

          {/* How the three agents differ */}
          <section>
            <h3 className="text-[13px] font-semibold text-secondary">{t("onboarding.agentsTitle")}</h3>
            <div className="mt-2 space-y-1.5">
              {AGENT_KEYS.map((key) => (
                <div
                  key={key}
                  className="rounded-lg border border-border-subtle bg-bg-secondary px-3 py-2"
                >
                  <p className="text-[13px] font-medium text-secondary">
                    {t(`onboarding.agents.${key}.name`)}
                  </p>
                  <p className="mt-0.5 text-[13px] leading-5 text-tertiary">
                    {t(`onboarding.agents.${key}.description`)}
                  </p>
                </div>
              ))}
            </div>
          </section>
        </div>

        {/* Footer */}
        <div className="shrink-0 border-t border-border-subtle px-5 py-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            {onboardingShowCheckbox ? (
              <label className="flex cursor-pointer select-none items-center gap-2">
                <input
                  type="checkbox"
                  checked={dontShowAgain}
                  onChange={(e) => setDontShowAgain(e.target.checked)}
                  className="h-3.5 w-3.5 accent-[var(--color-accent)]"
                />
                <span className="text-[13px] text-muted">{t("onboarding.dontShowAgain")}</span>
              </label>
            ) : (
              <span />
            )}
            <button
              type="button"
              onClick={handleDismiss}
              disabled={saving}
              className="inline-flex items-center gap-1.5 rounded-lg border border-accent-border bg-accent-dark px-3 py-1.5 text-[13px] font-medium text-white transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50 outline-none"
            >
              {t("onboarding.gotIt")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
