import { useEffect, useState, type ReactNode } from "react";
import type { MenuFooterRow } from "./MenuSurface";

const STORAGE_KEY = "codexbar.trayFooterVisibility.v1";
const DEFAULT_VISIBILITY = { zoom: true, settings: true, about: true, quit: true };
type VisibilityKey = keyof typeof DEFAULT_VISIBILITY;

function readVisibility(): typeof DEFAULT_VISIBILITY {
  const visibility = { ...DEFAULT_VISIBILITY };
  try {
    const stored: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null");
    if (stored && typeof stored === "object" && !Array.isArray(stored)) {
      for (const key of Object.keys(visibility) as VisibilityKey[]) {
        const value = (stored as Record<string, unknown>)[key];
        if (typeof value === "boolean") visibility[key] = value;
      }
    }
  } catch {
    // Storage may be unavailable in a webview; the footer must remain usable.
  }
  return visibility;
}

function EyeIcon({ open }: { open: boolean }) {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12Z" stroke="currentColor" strokeWidth="1.7" />
      <circle cx="12" cy="12" r="3" stroke="currentColor" strokeWidth="1.7" />
      {!open && <path d="m3 3 18 18" stroke="currentColor" strokeWidth="2" />}
    </svg>
  );
}

export default function TrayFooter({
  zoom,
  zoomLabel,
  rows,
  onLayoutChange,
}: {
  zoom: ReactNode;
  zoomLabel: string;
  rows: MenuFooterRow[];
  onLayoutChange: () => void;
}) {
  const [visibility, setVisibility] = useState(readVisibility);
  const [editing, setEditing] = useState(false);
  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(visibility));
    } catch {
      // A blocked write must not prevent hiding or recovering controls.
    }
    onLayoutChange();
  }, [visibility, editing, onLayoutChange]);

  const toggle = (key: VisibilityKey, label: string) => (
    <button
      type="button"
      className="tray-footer__eye"
      aria-label={`${visibility[key] ? "Hide" : "Show"} ${label}`}
      title={`${visibility[key] ? "Hide" : "Show"} ${label}`}
      aria-pressed={visibility[key]}
      onClick={() => setVisibility((current) => ({ ...current, [key]: !current[key] }))}
    >
      <EyeIcon open={visibility[key]} />
    </button>
  );

  return (
    <>
      <button
        type="button"
        className="tray-footer__eye tray-footer__editor"
        aria-label="Edit footer visibility"
        title="Edit footer visibility"
        aria-pressed={editing}
        onClick={() => setEditing((current) => !current)}
      >
        <EyeIcon open />
      </button>
      {(editing || visibility.zoom) && (
        editing ? (
          <div className={`tray-footer__entry${visibility.zoom ? "" : " tray-footer__entry--hidden"}`}>
            {zoom}
            {toggle("zoom", zoomLabel)}
          </div>
        ) : zoom
      )}
      {rows.map((row) => {
        const key = row.id as VisibilityKey;
        const hideable = Object.prototype.hasOwnProperty.call(DEFAULT_VISIBILITY, key);
        if (hideable && !editing && !visibility[key]) return null;
        const action = (
          <button
            type="button"
            className="menu-surface__footer-row"
            onClick={row.onClick}
          >
            {row.icon && <span className="menu-surface__footer-icon" aria-hidden>{row.icon}</span>}
            <span>{row.label}</span>
            {row.shortcut && <span className="menu-surface__footer-shortcut">{row.shortcut}</span>}
          </button>
        );
        return (
          <div
            key={row.id}
            className={`tray-footer__entry${hideable && !visibility[key] ? " tray-footer__entry--hidden" : ""}${row.id === "refresh" ? " tray-footer__refresh" : ""}`}
          >
            {action}
            {editing && hideable && toggle(key, row.label)}
          </div>
        );
      })}
    </>
  );
}
