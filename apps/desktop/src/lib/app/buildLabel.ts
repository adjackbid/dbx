export interface BuildIdentity {
  version?: string;
  commit?: string;
  buildTimeMs?: string;
}

/**
 * Human-readable identity of the running backend build, shown on the sign-in
 * screen so an operator can confirm which version a deployment actually serves.
 * Returns an empty string when the server did not report a version.
 */
export function formatBuildLabel(data: BuildIdentity | null | undefined): string {
  const version = (data?.version ?? "").trim();
  if (!version) return "";
  const parts = [`v${version}`];
  const commit = (data?.commit ?? "").trim();
  if (commit && commit !== "unknown") parts.push(commit);
  const buildTimeMs = Number((data?.buildTimeMs ?? "").trim());
  if (Number.isFinite(buildTimeMs) && buildTimeMs > 0) {
    parts.push(new Date(buildTimeMs).toISOString().slice(0, 16).replace("T", " "));
  }
  return parts.join(" · ");
}
