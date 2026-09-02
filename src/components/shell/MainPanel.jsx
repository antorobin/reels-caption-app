import { useState } from "react";
import VideoPreview from "../VideoPreview.jsx";
import ContentIdeasPanel from "../tools/ContentIdeasPanel.jsx";
import DuckingPanel from "../tools/DuckingPanel.jsx";
import LoudnessPanel from "../tools/LoudnessPanel.jsx";
import CaptionOverridePanel from "./CaptionOverridePanel.jsx";
import LiveDictationPanel from "./LiveDictationPanel.jsx";
import MoreOptionsModal from "./MoreOptionsModal.jsx";
import Timeline from "./Timeline.jsx";
import ToolCard from "./ToolCard.jsx";
import TranscribeStatus from "./TranscribeStatus.jsx";
import VoiceoverSection from "./VoiceoverSection.jsx";

function MainPanel(props) {
  const [moreOptionsOpen, setMoreOptionsOpen] = useState(false);
  // A range just dragged on the Timeline, waiting for the user to pick a
  // style for it (or cancel) via CaptionOverridePanel below -- local to
  // this component since it's a transient UI-only step, never itself
  // persisted (only the finished override, once Applied, is).
  const [pendingRange, setPendingRange] = useState(null);

  if (!props.videoPath) {
    return (
      <div className="shell-main-panel">
        <p className="shell-viewer-empty">Choose a video from the Media Pool to get started.</p>
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
            <Timeline
              videoPath={props.videoPath}
              words={props.words}
              currentTime={props.currentTime}
              duration={props.duration}
              onSeek={props.handleSeek}
              prosody={props.prosody}
              speakers={props.speakers}
              captionStyleOverrides={props.captionStyleOverrides}
              onRangeSelected={(start, end) => setPendingRange({ start, end })}
            />
            {/* Second, explicit way to start an override (alongside
                dragging directly on the waveform above) -- a drag gesture
                on a waveform isn't something everyone would think to try
                unprompted. Defaults to a short window starting at the
                current playhead position; Apply/Cancel and the same
                overlap check work identically either way. */}
            {props.duration > 0 && (
              <button
                type="button"
                className="link-button add-override-button"
                onClick={() => {
                  const defaultSpan = Math.min(3, props.duration);
                  const start = Math.min(props.currentTime, Math.max(0, props.duration - defaultSpan));
                  setPendingRange({ start, end: Math.min(start + defaultSpan, props.duration) });
                }}
              >
                + Add a style override for this portion
              </button>
            )}
          </div>

          {pendingRange && (
            <CaptionOverridePanel
              range={pendingRange}
              duration={props.duration}
              baseStyle={props.captionStyle}
              baseThemeId={props.captionThemeId}
              existingOverrides={props.captionStyleOverrides}
              onApply={(override) => {
                props.addCaptionStyleOverride(override);
                setPendingRange(null);
              }}
              onCancel={() => setPendingRange(null)}
            />
          )}

          <VoiceoverSection
            videoPath={props.videoPath}
            projectId={props.currentProjectId}
            onVoiceoverReady={props.onVoiceoverReady}
            onCaptionsFromRecording={props.onCaptionsFromRecording}
          />

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
