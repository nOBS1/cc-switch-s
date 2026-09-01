import type { AppId } from "@/lib/api/types";

export type PrivateForkManagementAppId = Exclude<
  AppId,
  "claude" | "claude-desktop"
>;

/**
 * Official Claude Code and Claude Desktop are deliberately read-only in this
 * private fork. They are shared with the official CC Switch installation, so
 * exposing either entry here would let the two products overwrite the same
 * live configuration. The isolated Claude Code (Cometix) entry remains
 * manageable because it writes only to its own configuration directory.
 */
export const isManagementApp = (
  app: AppId,
): app is PrivateForkManagementAppId =>
  app !== "claude" && app !== "claude-desktop";

export const filterManagementApps = <T extends AppId>(
  apps: readonly T[],
): Array<Extract<T, PrivateForkManagementAppId>> =>
  apps.filter((app): app is Extract<T, PrivateForkManagementAppId> =>
    isManagementApp(app),
  );
