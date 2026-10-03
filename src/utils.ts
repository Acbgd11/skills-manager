import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
    return twMerge(clsx(inputs));
}

/** Shorten the user's home directory to `~` for display. Windows paths also
 *  get their separators unified: agent dirs are joined from `/`-separated
 *  relative paths, which reads as `~\.workbuddy/skills` otherwise (#495). */
/** A hostname's human name, so a link says where it goes rather than showing a
 *  bare URL. Falls back to the hostname itself for anything unrecognised. */
export function sourceSiteName(url: string): string {
    let host: string;
    try {
        host = new URL(url).hostname.replace(/^www\./, "").toLowerCase();
    } catch {
        return url;
    }
    const known: Record<string, string> = {
        "github.com": "GitHub",
        "gitee.com": "Gitee",
        "gitlab.com": "GitLab",
        "clawhub.ai": "ClawHub",
        "clawhub.com": "ClawHub",
        "xiaping.coze.com": "Coze",
        "coze.com": "Coze",
        "skills.sh": "skills.sh",
    };
    return known[host] ?? host;
}

/** The GitHub URL a skill can be visited at, or null when it has none.
 *
 * Only `git`/`skillssh` skills carry a remote; `local`/`import` skills store a
 * filesystem path in `source_ref`, which is not something to open in a browser.
 * A `git` ref is used as-is when it is already a URL, otherwise the familiar
 * `owner/repo` shorthand is expanded. */
export function skillGithubUrl(skill: {
    source_type: string;
    source_ref: string | null;
}): string | null {
    if (skill.source_type !== "git" && skill.source_type !== "skillssh") return null;
    const raw = skill.source_ref?.trim();
    if (!raw) return null;
    if (/^https?:\/\//i.test(raw)) return raw;
    if (/^[\w.-]+\/[\w.-]+$/.test(raw)) return `https://github.com/${raw}`;
    return null;
}

export function compactHomePath(path: string) {
    const display = /^[A-Za-z]:\\/.test(path) ? path.replace(/\//g, "\\") : path;
    return display
        .replace(/^\/Users\/[^/]+/, "~")
        .replace(/^\/home\/[^/]+/, "~")
        .replace(/^[A-Za-z]:\\Users\\[^\\]+/, "~");
}
