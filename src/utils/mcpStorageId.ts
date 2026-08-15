import { decodeBase64Utf8 } from "@/lib/utils/base64";

const CLAUDE_SCOPE_PREFIXES = [
  "cc-switch-scope:v1:claude-cometix:",
  "cc-switch-scope:v1:claude:",
] as const;

/** Return the user/live MCP id while keeping scoped ids opaque to the UI. */
export function getMcpLiveId(storageId: string): string {
  for (const prefix of CLAUDE_SCOPE_PREFIXES) {
    if (!storageId.startsWith(prefix)) continue;
    const encoded = storageId.slice(prefix.length);
    const decoded = decodeBase64Utf8(encoded);
    return decoded || storageId;
  }
  return storageId;
}
