// 前端统一使用 AppId 作为应用标识（与后端命令参数 `app` 一致）
export type AppId =
  | "claude"
  | "claude-desktop"
  | "codex"
  | "gemini"
  | "grokbuild"
  | "opencode"
  | "openclaw"
  | "hermes";

// 顶部应用切换器还包含共享 Claude 配置域的虚拟客户端入口。
// 后端仍只接收 AppId；调用 API 前必须通过 toBackendAppId 收敛。
export type UiAppId = AppId | "claude-cometix";

export function toBackendAppId(appId: UiAppId): AppId {
  return appId === "claude-cometix" ? "claude" : appId;
}
