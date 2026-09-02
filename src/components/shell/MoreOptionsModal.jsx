import { useState } from "react";
import CaptionStyleEditor from "../CaptionStyleEditor.jsx";
import SilenceRemovalPanel from "../SilenceRemovalPanel.jsx";
import DiarizePanel from "../tools/DiarizePanel.jsx";
import KeywordsPanel from "../tools/KeywordsPanel.jsx";
import ProsodyPanel from "../tools/ProsodyPanel.jsx";
import VoSyncPanel from "../tools/VoSyncPanel.jsx";
import { CAPTION_THEMES } from "../../lib/themes.js";
import { formatTime } from "../../lib/time.js";

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
function MoreOptionsModal({
  onClose,
  videoPath,
  words,
  projectId,
  onJumpCutApplied,
  captionStyle,
  onCaptionStyleChange,
  captionThemeId,
  onCaptionThemeIdChange,
  onResetCaptionStyleToFactoryDefault,
  captionStyleOverrides = [],
  onRemoveCaptionStyleOverride,
  ...toolProps
}) {
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
          {activeTab === "style" && (
            <>
              <CaptionStyleEditor
                style={captionStyle}
                onChange={onCaptionStyleChange}
                themeId={captionThemeId}
                onThemeIdChange={onCaptionThemeIdChange}
                onResetToFactoryDefault={onResetCaptionStyleToFactoryDefault}
              />
              {captionStyleOverrides.length > 0 && (
                <div className="caption-override-list">
                  <h3>Style overrides</h3>
                  <p className="section-hint">
                    Drag a range directly on the timeline to add another — a portion styled differently from the rest of the video.
                  </p>
                  {captionStyleOverrides.map((o, i) => (
                    <div key={i} className="caption-override-list-row">
                      <span>
                        {formatTime(o.start)} – {formatTime(o.end)}
                      </span>
                      <span>{CAPTION_THEMES.find((t) => t.id === o.themeId)?.name || "Custom"}</span>
                      <button type="button" className="link-button" onClick={() => onRemoveCaptionStyleOverride(i)}>
                        Remove
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </>
          )}

          {activeTab === "silence" && (
            <SilenceRemovalPanel videoPath={videoPath} words={words} projectId={projectId} onApplied={onJumpCutApplied} />
          )}

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
