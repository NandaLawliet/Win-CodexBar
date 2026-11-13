import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useProviders } from "../hooks/useProviders";
import type { RateWindowSnapshot } from "../types/bridge";
import {
  formatResetCountdown,
  formatUsagePercent,
  quickPeekProviders,
} from "./quickPeekFormat";
import "./QuickPeek.css";

function UsageLane({
  label,
  window,
  now,
}: {
  label: "S" | "W";
  window: RateWindowSnapshot | null;
  now: Date;
}) {
  const percentage = window ? formatUsagePercent(window.usedPercent) : "—";
  const countdown = window ? formatResetCountdown(window, now) : "—";
  return (
    <div className="quick-peek__lane">
      {label} {percentage} • {countdown}
    </div>
  );
}

/**
 * Deliberately recreates the compact Windows tray-tooltip layout.
 * No cards, graphs, progress bars, accounts, or fresh provider fetches.
 */
export default function QuickPeek() {
  const { providers, hasLoadedCache } = useProviders({ refreshOnMount: false });
  const [now, setNow] = useState(() => new Date());
  const visible = quickPeekProviders(providers);

  useEffect(() => {
    if (hasLoadedCache) {
      void invoke("reveal_quick_peek_window").catch(() => {});
    }
  }, [hasLoadedCache]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        void invoke("dismiss_quick_peek_window").catch(() => {});
      }
    };
    window.addEventListener("keydown", onKeyDown);
    const interval = window.setInterval(() => setNow(new Date()), 30_000);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.clearInterval(interval);
    };
  }, []);

  return (
    <main className="quick-peek" aria-label="Claude and Codex usage quick peek">
      {visible.length === 0 ? (
        <div className="quick-peek__empty">No cached usage</div>
      ) : (
        visible.map((provider) => (
          <section className="quick-peek__provider" key={provider.providerId}>
            <div className="quick-peek__name">{provider.displayName}</div>
            <UsageLane label="S" window={provider.error ? null : provider.primary} now={now} />
            <UsageLane label="W" window={provider.error ? null : provider.secondary} now={now} />
          </section>
        ))
      )}
    </main>
  );
}
