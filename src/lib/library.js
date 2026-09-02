// Small, pure JS helpers shared between App.jsx and Sidebar.jsx for the
// persistent project library. Mirrors library.rs's own `placeholder_title`
// exactly (same "underscore/hyphen to space" rule) so the frontend can
// tell "this is still the auto-generated placeholder" apart from "the user
// actually typed a real title" without needing a round-trip to Rust just
// to ask.

export function placeholderTitleFor(filename) {
  if (!filename) return "";
  const withoutExt = filename.replace(/\.[^./\\]+$/, "");
  return withoutExt.replace(/[_-]/g, " ");
}
