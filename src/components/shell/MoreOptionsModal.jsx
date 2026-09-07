import { useState } from "react";
import CaptionStyleEditor, { displayNameFor } from "../CaptionStyleEditor.jsx";
import SilenceRemovalPanel from "../SilenceRemovalPanel.jsx";
import DiarizePanel from "../tools/DiarizePanel.jsx";
import KeywordsPanel from "../tools/KeywordsPanel.jsx";
import ProsodyPanel from "../tools/ProsodyPanel.jsx";
import VoSyncPanel from "../tools/VoSyncPanel.jsx";
import { TRANSITION_EFFECT_LABELS } from "../../lib/captions.js";
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
  videoTransitions = [],
  onRemoveVideoTransition,
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
                      {/* displayNameFor, not a raw CAPTION_THEMES lookup -- so a
                          hand-tweaked override reads "<Theme> (Custom)" here too,
                          matching Timeline.jsx's own tooltip/popover for the same
                          override instead of silently showing just the base
                          theme's name as if nothing had been changed. */}
                      <span>{displayNameFor(o.themeId, o.style)}</span>
                      <button type="button" className="link-button" onClick={() => onRemoveCaptionStyleOverride(i)}>
                        Remove
                      </button>
                    </div>
                  ))}
                </div>
              )}
              {videoTransitions.length > 0 && (
                <div className="caption-override-list">
                  <h3>Video transitions</h3>
                  <p className="section-hint">
                    Real effects burned into the footage itself, added from a pin's menu on the timeline above — not part of the caption
                    style. Captions stay fully readable through any of them.
                  </p>
                  {videoTransitions.map((t, i) => (
                    <div key={i} className="caption-override-list-row">
                      <span>{formatTime(t.time)}</span>
                      <span>{TRANSITION_EFFECT_LABELS[t.effect] || t.effect}</span>
                      <button type="button" className="link-button" onClick={() => onRemoveVideoTransition(i)}>
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
