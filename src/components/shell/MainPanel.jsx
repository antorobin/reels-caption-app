import { useState } from "react";
import VideoPreview from "../VideoPreview.jsx";
import ContentIdeasPanel from "../tools/ContentIdeasPanel.jsx";
import DuckingPanel from "../tools/DuckingPanel.jsx";
import LoudnessPanel from "../tools/LoudnessPanel.jsx";
import MoreOptionsModal from "./MoreOptionsModal.jsx";
import Timeline from "./Timeline.jsx";
import ToolCard from "./ToolCard.jsx";
import TranscribeStatus from "./TranscribeStatus.jsx";
import VoiceoverSection from "./VoiceoverSection.jsx";

function MainPanel(props) {
  const [moreOptionsOpen, setMoreOptionsOpen] = useState(false);

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
          onTimeUpdate={props.setCurrentTime}
          onLoadedMetadata={props.setDuration}
          onSeek={props.handleSeek}
          voiceoverPath={props.voiceoverPath}
          voiceoverOffset={props.voiceoverOffset}
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
            />
          </div>

          <VoiceoverSection videoPath={props.videoPath} onVoiceoverReady={props.onVoiceoverReady} />

          <div className="tool-card-row">
            <ToolCard title="Loudness" status="−14 LUFS target">
              <LoudnessPanel videoPath={props.videoPath} />
            </ToolCard>
            <ToolCard title="Music ducking" status="No music bed yet">
              <DuckingPanel videoPath={props.videoPath} words={props.words} />
            </ToolCard>
            <ToolCard
              title="Title, hashtags & emoji"
              status={props.contentIdeas ? "Generated" : "Not generated yet"}
              badge="AI"
            >
              <ContentIdeasPanel
                words={props.words}
                result={props.contentIdeas}
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
          onJumpCutApplied={props.handleJumpCutApplied}
          captionStyle={props.captionStyle}
          onCaptionStyleChange={props.setCaptionStyle}
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
