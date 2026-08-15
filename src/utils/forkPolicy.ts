import type { AppId } from "@/lib/api/types";

/**
 * Claude Desktop is deliberately read-only in this private fork.  It is a
 * machine-wide application shared with the official CC Switch installation,
 * so exposing it here would let the two products overwrite the same live
 * configuration.
 */
export const isManagementApp = (app: AppId): boolean =>
  app !== "claude-desktop";

export const filterManagementApps = (apps: readonly AppId[]): AppId[] =>
  apps.filter(isManagementApp);
