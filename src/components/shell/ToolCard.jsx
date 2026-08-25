import { useState } from "react";

// A compact, contextual entry point for a secondary tool (Loudness, Music
// Ducking, Title/Hashtags/Emoji) -- collapsed by default (a one-line status
// is enough for most visits), expands in place to the tool's existing full
// panel rather than navigating away to a separate screen. Replaces the old
// long sidebar Tools list for these promoted tools.
function ToolCard({ title, status, badge, children }) {
  const [open, setOpen] = useState(false);

  return (
    <div className="tool-card">
      <button type="button" className="tool-card-header" onClick={() => setOpen((v) => !v)}>
        <span className="tool-card-title">
          {title}
          {badge && <span className="tool-card-badge">{badge}</span>}
        </span>
        <span className="tool-card-status">{status}</span>
        <svg
          width="12"
          height="12"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2.5"
          strokeLinecap="round"
          strokeLinejoin="round"
          className={open ? "tool-card-chevron open" : "tool-card-chevron"}
        >
          <polyline points="6 9 12 15 18 9" />
        </svg>
      </button>
      {open && <div className="tool-card-body">{children}</div>}
    </div>
  );
}

export default ToolCard;
