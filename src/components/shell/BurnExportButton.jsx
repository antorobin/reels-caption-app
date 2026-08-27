import ProgressBar from "../ProgressBar.jsx";
import ScheduleToInstagramButton from "./ScheduleToInstagramButton.jsx";

// Fixed to the bottom-right of the whole shell, always visible -- the one
// action every session ends with, so it's never buried behind navigation.
function BurnExportButton({ burnCaptions, burning, burnProgress, burnStatus, disabled, lastBurnedPath, contentIdeas }) {
  return (
    <div className="burn-export-dock">
      {burnStatus && <div className="burn-export-status">{burnStatus}</div>}
      {burning && (
        <div className="burn-export-progress">
          <ProgressBar progress={burnProgress} />
        </div>
      )}
      <div className="burn-export-buttons-row">
        <ScheduleToInstagramButton videoPath={lastBurnedPath} contentIdeas={contentIdeas} />
        <button type="button" className="burn-export-button" onClick={burnCaptions} disabled={disabled || burning}>
          <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="#fff" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M12 2C8 6 6 9 6 12a6 6 0 0 0 12 0c0-3-2-6-6-10z" />
          </svg>
          {burning ? "Burning…" : "Burn & Export"}
        </button>
      </div>
    </div>
  );
}

export default BurnExportButton;
