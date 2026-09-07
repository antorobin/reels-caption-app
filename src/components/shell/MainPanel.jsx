import { useState } from "react";
import VideoPreview from "../VideoPreview.jsx";
import ContentIdeasPanel from "../tools/ContentIdeasPanel.jsx";
import DuckingPanel from "../tools/DuckingPanel.jsx";
import LoudnessPanel from "../tools/LoudnessPanel.jsx";
import { CAPTION_THEMES } from "../../lib/themes.js";
import CaptionOverridePanel from "./CaptionOverridePanel.jsx";
import LiveDictationPanel from "./LiveDictationPanel.jsx";
import MoreOptionsModal from "./MoreOptionsModal.jsx";
import Timeline from "./Timeline.jsx";
import ToolCard from "./ToolCard.jsx";
import TranscribeStatus from "./TranscribeStatus.jsx";
import VoiceoverSection from "./VoiceoverSection.jsx";
import { RuntimePackGate } from "../../lib/runtimePacks.jsx";

function MainPanel(props) {
  // Lifted to AppShell.jsx so TranscriptPanel's "Change theme" link (a
  // sibling of this component) can open the same modal -- see its own
  // doc comment there.
  const { moreOptionsOpen, setMoreOptionsOpen } = props;
  // A range just selected (either dragged directly on Timeline, or from
  // its "🎨 Add caption style" pin action), waiting for the user to pick
  // a style for it (or cancel) via CaptionOverridePanel below -- local to
  // this component since it's a transient UI-only step, never itself
  // persisted (only the finished override, once Applied, is).
  const [pendingRange, setPendingRange] = useState(null);
  // Set only by the pin action, when the clicked pin came with known
  // "reasons" (a suggestion) -- seeds CaptionOverridePanel's draft with
  // the auto-picked theme for that reason instead of the project's plain
  // base style, so "Add caption style" starts from a good guess without
  // needing its own separate one-click "auto-apply" action. `null` for
  // every other entry point (a direct drag, a plain pin with no reasons
  // to guess from), which falls back to the base style as it always has.
  const [pendingSeedThemeId, setPendingSeedThemeId] = useState(null);
  // Index into captionStyleOverrides currently open for editing (via the
  // Timeline popover's "Change style" action) -- mutually exclusive with
  // pendingRange; opening either one closes the other so only one
  // CaptionOverridePanel is ever on screen at once.
  const [editingIndex, setEditingIndex] = useState(null);
  const editingOverride = editingIndex != null ? props.captionStyleOverrides?.[editingIndex] : null;

  // Single entry point for both ways of starting a create-flow (a direct
  // drag, or a pin's "🎨 Add caption style") -- both just want the same
  // panel opened over the same range, one of them also carrying a seed
  // theme guess.
  function openCreatePanel(start, end, seedThemeId = null) {
    setEditingIndex(null);
    setPendingRange({ start, end });
    setPendingSeedThemeId(seedThemeId);
  }

  if (!props.videoPath) {
    return (
      <div className="shell-main-panel">
        <p className="shell-viewer-empty">Click the + next to "Projects" to upload a video, or open a project from the list to get started.</p>
      </div>
    );
  }

  return (
    <div className="shell-main-panel">
      <div className="shell-viewer-header">{props.videoPath.split(/[\\/]/).pop()}</div>

      <div className="main-panel-layout">
        <VideoPreview
          videoRef={props.videoRef}
          videoPath={props.videoPath}
          currentTime={props.currentTime}
          duration={props.duration}
          words={props.words}
          captionStyle={props.captionStyle}
          captionStyleOverrides={props.captionStyleOverrides}
          videoTransitions={props.videoTransitions}
          onTimeUpdate={props.setCurrentTime}
          onLoadedMetadata={props.setDuration}
          onSeek={props.handleSeek}
          voiceoverPath={props.voiceoverPath}
          voiceoverOffset={props.voiceoverOffset}
          musicPath={props.musicPath}
          duckLevel={props.duckLevel}
        />

        <div className="main-panel-side">
          <TranscribeStatus
            pipelineRunning={props.pipelineRunning}
            pipelineProgress={props.pipelineProgress}
            pipelineStatus={props.pipelineStatus}
            detectedLanguage={props.detectedLanguage}
            wordsCount={props.words.length}
          />

          <div className="main-panel-timeline">
            {/* Two ways to start an override, both ending in the same
                openCreatePanel call below: click a pin's "🎨 Add caption
                style" (any pin -- an auto-suggested point, a manually
                dropped one, or the timeline's own "📍 Pin a style
                override" button), or skip pins entirely and drag directly
                on the waveform -- the pin path is the more discoverable,
                editor-like route; the direct drag stays as a fast path
                for anyone who doesn't bother with it. */}
            <Timeline
              videoPath={props.videoPath}
              words={props.words}
              currentTime={props.currentTime}
              duration={props.duration}
              onSeek={props.handleSeek}
              prosody={props.prosody}
              speakers={props.speakers}
              captionStyleOverrides={props.captionStyleOverrides}
              videoTransitions={props.videoTransitions}
              onAddVideoTransition={(time, effect) => props.addVideoTransition({ time, effect })}
              onUpdateVideoTransition={(index, transition) => props.updateVideoTransition(index, transition)}
              onRemoveVideoTransition={(index) => props.removeVideoTransition(index)}
              onSuggestTransitionPlan={(candidates) => props.suggestTransitionPlan(candidates, props.currentProjectId)}
              suggestingTransitionPlan={props.suggestingTransitionPlan}
              onRangeSelected={(start, end) => openCreatePanel(start, end)}
              onAddCaptionStyle={(start, end, seedThemeId) => openCreatePanel(start, end, seedThemeId)}
              onEditOverride={(index) => {
                setPendingRange(null);
                setEditingIndex(index);
              }}
              onRemoveOverride={(index) => props.removeCaptionStyleOverride(index)}
            />
          </div>

          {pendingRange &&
            (() => {
              // A seed theme (from a pin with known "reasons") pre-fills
              // the draft with that guess instead of the project's plain
              // base style -- see pendingSeedThemeId's own doc comment.
              const seedTheme = pendingSeedThemeId ? CAPTION_THEMES.find((t) => t.id === pendingSeedThemeId) : null;
              return (
                <CaptionOverridePanel
                  range={pendingRange}
                  duration={props.duration}
                  baseStyle={seedTheme ? { ...seedTheme.style } : props.captionStyle}
                  baseThemeId={seedTheme ? seedTheme.id : props.captionThemeId}
                  existingOverrides={props.captionStyleOverrides}
                  onApply={(override) => {
                    props.addCaptionStyleOverride(override);
                    setPendingRange(null);
                  }}
                  onCancel={() => setPendingRange(null)}
                />
              );
            })()}

          {editingOverride && (
            <CaptionOverridePanel
              range={{ start: editingOverride.start, end: editingOverride.end }}
              duration={props.duration}
              baseStyle={editingOverride.style}
              baseThemeId={editingOverride.themeId}
              existingOverrides={props.captionStyleOverrides}
              excludeIndex={editingIndex}
              onApply={(override) => {
                props.updateCaptionStyleOverride(editingIndex, override);
                setEditingIndex(null);
              }}
              onCancel={() => setEditingIndex(null)}
              onRemove={() => {
                props.removeCaptionStyleOverride(editingIndex);
                setEditingIndex(null);
              }}
            />
          )}

          <RuntimePackGate need="python-voice">
            <VoiceoverSection
              videoPath={props.videoPath}
              projectId={props.currentProjectId}
              onVoiceoverReady={props.onVoiceoverReady}
              onCaptionsFromRecording={props.onCaptionsFromRecording}
            />
          </RuntimePackGate>

          <LiveDictationPanel onCaptionsFromRecording={props.onCaptionsFromRecording} />

          <div className="tool-card-row">
            <ToolCard title="Loudness" status="−14 LUFS target">
              <LoudnessPanel videoPath={props.videoPath} />
            </ToolCard>
            <ToolCard title="Music ducking" status="No music bed yet" badge="AI">
              <DuckingPanel
                videoPath={props.videoPath}
                words={props.words}
                detectedLanguage={props.detectedLanguage}
                projectId={props.currentProjectId}
                onMusicBedChange={props.onMusicBedChange}
              />
            </ToolCard>
            <ToolCard
              title="Title, hashtags & emoji"
              status={props.contentIdeas ? "Generated" : "Not generated yet"}
              badge="AI"
            >
              <RuntimePackGate need="llm">
                <ContentIdeasPanel
                  words={props.words}
                  result={props.contentIdeas}
                  options={props.contentStrategyOptions}
                  hints={props.contentHints}
                  onHintsChange={props.setContentHints}
                  onSelectOption={props.selectContentStrategyOption}
                  generating={props.generatingContentIdeas}
                  error={props.contentIdeasError}
                  onGenerate={props.generateContentIdeas}
                />
              </RuntimePackGate>
            </ToolCard>
          </div>

          <button type="button" className="more-options-link" onClick={() => setMoreOptionsOpen(true)}>
            More options (style, silence removal, speaker labels…)
          </button>
        </div>
      </div>

      {moreOptionsOpen && (
        <MoreOptionsModal
          onClose={() => setMoreOptionsOpen(false)}
          videoPath={props.videoPath}
          words={props.words}
          projectId={props.currentProjectId}
          onJumpCutApplied={props.handleJumpCutApplied}
          captionStyle={props.captionStyle}
          onCaptionStyleChange={props.setCaptionStyle}
          captionThemeId={props.captionThemeId}
          onCaptionThemeIdChange={props.setCaptionThemeId}
          onResetCaptionStyleToFactoryDefault={props.resetCaptionStyleToFactoryDefault}
          captionStyleOverrides={props.captionStyleOverrides}
          onRemoveCaptionStyleOverride={props.removeCaptionStyleOverride}
          videoTransitions={props.videoTransitions}
          onRemoveVideoTransition={props.removeVideoTransition}
          prosody={props.prosody}
          analyzingProsody={props.analyzingProsody}
          prosodyStatus={props.prosodyStatus}
          prosodyProgress={props.prosodyProgress}
          analyzeProsody={props.analyzeProsody}
          speakers={props.speakers}
          analyzingSpeakers={props.analyzingSpeakers}
          diarizeStatus={props.diarizeStatus}
          diarizeProgress={props.diarizeProgress}
          diarizeSpeakers={props.diarizeSpeakers}
        />
      )}
    </div>
  );
}

export default MainPanel;
