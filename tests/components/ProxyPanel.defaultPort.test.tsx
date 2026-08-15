import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@/lib/query/proxy", () => ({
  useProxyStatusQuery: () => ({ data: { running: false } }),
  useProxyTakeoverStatus: () => ({ data: {} }),
  useSetProxyTakeoverForApp: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useGlobalProxyConfig: () => ({ data: undefined }),
  useUpdateGlobalProxyConfig: () => ({
    mutateAsync: vi.fn(),
    isPending: false,
  }),
}));

vi.mock("@/lib/query/failover", () => ({
  useFailoverQueue: () => ({ data: [] }),
  useProviderHealth: () => ({ data: undefined }),
}));

vi.mock("@/components/providers/ProviderHealthBadge", () => ({
  ProviderHealthBadge: () => null,
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (_key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? _key,
  }),
}));

import { ProxyPanel } from "@/components/proxy/ProxyPanel";
import { DEFAULT_PROXY_PORT } from "@/types/proxy";

describe("ProxyPanel default port", () => {
  it("shows the fork-specific port before persisted settings load", () => {
    render(
      <ProxyPanel
        enableLocalProxy={false}
        onEnableLocalProxyChange={vi.fn()}
        onToggleProxy={vi.fn()}
        isProxyPending={false}
      />,
    );

    expect(DEFAULT_PROXY_PORT).toBe(15731);
    expect(screen.getByLabelText("监听端口")).toHaveValue(15731);
    expect(screen.getByLabelText("监听端口")).toHaveAttribute(
      "placeholder",
      "15731",
    );
  });
});
