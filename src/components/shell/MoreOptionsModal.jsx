import { useState } from "react";
import CaptionStyleEditor from "../CaptionStyleEditor.jsx";
import SilenceRemovalPanel from "../SilenceRemovalPanel.jsx";
import DiarizePanel from "../tools/DiarizePanel.jsx";
import KeywordsPanel from "../tools/KeywordsPanel.jsx";
import ProsodyPanel from "../tools/ProsodyPanel.jsx";
import VoSyncPanel from "../tools/VoSyncPanel.jsx";

const TABS = [
  { id: "style", label: "Style captions" },
  { id: "silence", label: "Remove silence" },
  { id: "prosody", label: "Vocal emphasis" },
  { id: "diarize", label: "Speaker diarization" },
  { id: "vosync", label: "Voice-over sync (export)" },
  { id: "keywords", label: "Keywords & hashtags" },
];

// Everything that doesn't need to be visible by default: less-common
// refinements, tucked one click away instead of occupying permanent space
// in the main flow. Each tab slots in an existing panel unchanged.
function MoreOptionsModal({ onClose, videoPath, words, onJumpCutApplied, captionStyle, onCaptionStyleChange, ...toolProps }) {
  const [activeTab, setActiveTab] = useState("style");

  return (
    <div className="shell-modal-backdrop" onClick={onClose}>
      <div className="shell-modal more-options-modal" onClick={(e) => e.stopPropagation()}>
        <div className="shell-modal-header">
          <h2>More options</h2>
          <button type="button" className="shell-modal-close" onClick={onClose}>
            ✕
          </button>
        </div>

        <div className="more-options-tabs">
          {TABS.map((tab) => (
            <button
              key={tab.id}
              type="button"
              className={activeTab === tab.id ? "more-options-tab active" : "more-options-tab"}
              onClick={() => setActiveTab(tab.id)}
            >
              {tab.label}
            </button>
          ))}
        </div>

        <div className="more-options-body">
          {activeTab === "style" && <CaptionStyleEditor style={captionStyle} onChange={onCaptionStyleChange} />}

          {activeTab === "silence" && <SilenceRemovalPanel videoPath={videoPath} words={words} onApplied={onJumpCutApplied} />}

          {activeTab === "prosody" && (
            <ProsodyPanel
              videoPath={videoPath}
              words={words}
              prosody={toolProps.prosody}
              analyzingProsody={toolProps.analyzingProsody}
              prosodyStatus={toolProps.prosodyStatus}
              prosodyProgress={toolProps.prosodyProgress}
              analyzeProsody={toolProps.analyzeProsody}
            />
          )}

          {activeTab === "diarize" && (
            <DiarizePanel
              videoPath={videoPath}
              words={words}
              speakers={toolProps.speakers}
              analyzingSpeakers={toolProps.analyzingSpeakers}
              diarizeStatus={toolProps.diarizeStatus}
              diarizeProgress={toolProps.diarizeProgress}
              diarizeSpeakers={toolProps.diarizeSpeakers}
            />
          )}

          {activeTab === "vosync" && <VoSyncPanel videoPath={videoPath} />}

          {activeTab === "keywords" && <KeywordsPanel words={words} />}
        </div>
      </div>
    </div>
  );
}

export default MoreOptionsModal;
