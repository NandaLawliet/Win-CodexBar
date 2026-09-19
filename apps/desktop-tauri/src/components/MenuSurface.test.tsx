import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  getLocaleStrings: vi.fn(),
  setUiLanguage: vi.fn(),
}));

const eventMocks = vi.hoisted(() => ({
  listen: vi.fn(),
}));

vi.mock("../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/tauri")>()),
  ...tauriMocks,
}));
vi.mock("@tauri-apps/api/event", () => eventMocks);

import { LocaleProvider } from "../i18n/LocaleProvider";
import { buildBundle } from "../test/localeHarness";
import MenuSurface, {
  MenuSummary,
  type MenuSurfaceAction,
  type MenuFooterRow,
} from "./MenuSurface";

function renderSummary(total: number) {
  return render(
    <LocaleProvider>
      <MenuSummary
        total={total}
        errorCount={0}
        isRefreshing={false}
        lastRefresh={null}
      />
    </LocaleProvider>,
  );
}

function renderSurface(props: {
  isRefreshing?: boolean;
  onRefresh?: () => void;
  actions?: MenuSurfaceAction[];
  footerRows?: MenuFooterRow[];
}) {
  const onRefresh = props.onRefresh ?? vi.fn();
  return {
    onRefresh,
    ...render(
      <LocaleProvider>
        <MenuSurface
          variant="tray"
          onRefresh={onRefresh}
          isRefreshing={props.isRefreshing ?? false}
          actions={props.actions ?? []}
          footerRows={props.footerRows ?? []}
        >
          <div data-testid="provider-content">Provider Card Content</div>
        </MenuSurface>
      </LocaleProvider>,
    ),
  };
}

describe("MenuSummary", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle({ SummaryProvidersLabel: "providers" }),
    );
    eventMocks.listen.mockResolvedValue(() => {});
  });

  it("uses a singular provider label for one provider", async () => {
    renderSummary(1);

    expect(await screen.findByText("1 provider")).toBeInTheDocument();
  });

  it("keeps the plural provider label for multiple providers", async () => {
    renderSummary(2);

    expect(await screen.findByText("2 providers")).toBeInTheDocument();
  });
});

describe("MenuSurface layout and refresh placement", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle({ ActionRefreshAll: "Refresh All", ActionRefresh: "Refresh" }),
    );
    eventMocks.listen.mockResolvedValue(() => {});
  });

  it("renders Refresh All before Agent Usage in DOM order", async () => {
    const { container } = renderSurface({});

    const refreshBtn = await screen.findByRole("button", { name: /Refresh All/i });
    const sectionTitle = screen.getByRole("heading", { name: "Agent Usage" });

    expect(refreshBtn).toBeInTheDocument();
    expect(sectionTitle).toBeInTheDocument();

    expect(
      refreshBtn.compareDocumentPosition(sectionTitle) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();

    const topRow = container.querySelector(".menu-surface__top-row");
    expect(topRow).toBeInTheDocument();
    expect(topRow?.contains(refreshBtn)).toBe(true);
    expect(topRow?.nextElementSibling).toBe(sectionTitle);
  });

  it("triggers onRefresh callback when Refresh All button is clicked", async () => {
    const onRefresh = vi.fn();
    renderSurface({ onRefresh });

    const refreshBtn = await screen.findByRole("button", { name: /Refresh All/i });
    fireEvent.click(refreshBtn);

    expect(onRefresh).toHaveBeenCalledTimes(1);
  });

  it("disables Refresh All button and animates icon while isRefreshing is true", async () => {
    const { container } = renderSurface({ isRefreshing: true });

    const refreshBtn = await screen.findByRole("button", { name: /Refresh All/i });
    expect(refreshBtn).toBeDisabled();
    expect(refreshBtn).toHaveAttribute("aria-busy", "true");

    const icon = container.querySelector(".menu-surface__refresh-icon");
    expect(icon).toHaveClass("spin");
  });

  it("renders top actions alongside Refresh All without duplicating refresh in footer", async () => {
    const onAction = vi.fn();
    const actions: MenuSurfaceAction[] = [
      { icon: "⇲", title: "Open in Window", onClick: onAction },
    ];
    const footerRows: MenuFooterRow[] = [
      { id: "settings", icon: "⚙", label: "Settings", onClick: vi.fn() },
      { id: "quit", icon: "✕", label: "Quit", onClick: vi.fn() },
    ];

    const { container } = renderSurface({ actions, footerRows });

    expect(await screen.findByTitle("Open in Window")).toBeInTheDocument();
    const footer = container.querySelector(".menu-surface__footer");
    expect(footer).toBeInTheDocument();
    expect(footer?.textContent).toContain("Settings");
    expect(footer?.textContent).toContain("Quit");
    expect(footer?.textContent).not.toContain("Refresh");
  });
});
