import type { ProviderCatalogEntry, ProviderUsageSnapshot } from "../types/bridge";
import { providerPlaceholder } from "./trayProviders";

export function orderProviderSnapshots(
  providers: ProviderUsageSnapshot[],
  catalog: ProviderCatalogEntry[],
  enabledProviderIds: string[],
  providerOrder: string[] = [],
): ProviderUsageSnapshot[] {
  const order = new Map<string, number>();
  const orderedIds = providerOrder.length > 0
    ? providerOrder
    : catalog.map((provider) => provider.id);
  for (const [index, providerId] of orderedIds.entries()) {
    order.set(providerId, index);
  }
  for (const [index, providerId] of enabledProviderIds.entries()) {
    if (!order.has(providerId)) {
      order.set(providerId, orderedIds.length + index);
    }
  }

  // The tray must represent configuration, not only whichever provider
  // snapshots happened to arrive first. A newly-enabled/slow provider (for
  // example Antigravity local/LSP) can otherwise disappear from the custom
  // overview until its provider-updated event reaches this webview.
  const catalogNames = new Map(
    catalog.map((provider) => [provider.id, provider.displayName]),
  );
  const hydrated = [...providers];
  const present = new Set(hydrated.map((provider) => provider.providerId));
  for (const providerId of enabledProviderIds) {
    if (present.has(providerId)) continue;
    hydrated.push(
      providerPlaceholder(
        providerId,
        catalogNames.get(providerId) ?? providerId,
      ),
    );
    present.add(providerId);
  }

  return hydrated.sort((a, b) => {
    const aOrder = order.get(a.providerId);
    const bOrder = order.get(b.providerId);
    if (aOrder != null && bOrder != null && aOrder !== bOrder) return aOrder - bOrder;
    if (aOrder != null) return -1;
    if (bOrder != null) return 1;
    return a.displayName.localeCompare(b.displayName);
  });
}
