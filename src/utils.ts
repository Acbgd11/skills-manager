import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
    return twMerge(clsx(inputs));
}

/** Shorten the user's home directory to `~` for display. Windows paths also
 *  get their separators unified: agent dirs are joined from `/`-separated
 *  relative paths, which reads as `~\.workbuddy/skills` otherwise (#495). */
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
