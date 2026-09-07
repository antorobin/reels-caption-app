import ExportButton from "./ExportButton.jsx";
import ProgressBar from "../ProgressBar.jsx";

// Fixed to the bottom-right of the whole shell, always visible -- the one
// action every session ends with, so it's never buried behind navigation.
//
// Labeled "Save" rather than "Burn & Export" -- requested directly: this
// is genuinely the app's save/checkpoint action (it also saves the
// project's full state immediately, not just the burned video -- see
// App.jsx's burnCaptions), and every prior state is still there in the
// project's processed/ folder, so clicking it again to "save" further
// progress is exactly the repeatable action a save button should be. The
// underlying prop/function names (`burnCaptions`, `burning`, ...) stay as
// they are -- they describe the actual mechanism (burning captions into a
// new video file), not what the button is called.
function BurnExportButton({
  burnCaptions,
  burning,
  burnProgress,
  burnStatus,
  disabled,
  lastBurnedPath,
  contentIdeas,
  revealLastBurned,
  currentProjectId,
}) {
  return (
    <div className="burn-export-dock">
      {burnStatus && (
        <div className="burn-export-status">
          {burnStatus}
          {lastBurnedPath && !burning && (
            <button type="button" className="link-button" onClick={revealLastBurned}>
              Reveal in folder
            </button>
          )}
        </div>
      )}
      {burning && (
        <div className="burn-export-progress">
          <ProgressBar progress={burnProgress} />
        </div>
      )}
      <div className="burn-export-buttons-row">
        <ExportButton lastBurnedPath={lastBurnedPath} contentIdeas={contentIdeas} currentProjectId={currentProjectId} />
        <button type="button" className="burn-export-button" onClick={burnCaptions} disabled={disabled || burning}>
          <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="#fff" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z" />
            <path d="M17 21v-8H7v8" />
            <path d="M7 3v5h8" />
          </svg>
          {burning ? "Saving…" : "Save"}
        </button>
      </div>
    </div>
  );
}

export default BurnExportButton;
