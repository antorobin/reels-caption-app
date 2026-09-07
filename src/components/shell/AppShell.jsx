import { useState } from "react";
import BackgroundJobsBanner from "./BackgroundJobsBanner.jsx";
import BurnExportButton from "./BurnExportButton.jsx";
import MainPanel from "./MainPanel.jsx";
import MenuBar from "./MenuBar.jsx";
import OptionalModelsBanner from "./OptionalModelsBanner.jsx";
import Sidebar from "./Sidebar.jsx";
import TranscriptPanel from "./TranscriptPanel.jsx";
import UpdateBanner from "./UpdateBanner.jsx";

function AppShell(props) {
  // Lifted here (rather than staying local to MainPanel, where it lived
  // before) so TranscriptPanel's new "Change theme" link -- a sibling of
  // MainPanel, not a child -- can open the same modal MainPanel renders.
  // Always mounts fresh when opened (MoreOptionsModal's own `activeTab`
  // state defaults to "style"), so this doubles as "always land on the
  // style tab" for free, without needing a separate initial-tab prop.
  const [moreOptionsOpen, setMoreOptionsOpen] = useState(false);

  return (
    <div className="shell-grid">
      <MenuBar />

      <Sidebar
        onPickVideo={props.pickVideo}
        currentProjectId={props.currentProjectId}
        onSelectProject={props.loadProject}
        projectTitle={props.projectTitle}
        setProjectTitle={props.setProjectTitle}
        projectDescription={props.projectDescription}
        setProjectDescription={props.setProjectDescription}
        projectHashtags={props.projectHashtags}
        setProjectHashtags={props.setProjectHashtags}
      />

      <MainPanel
        moreOptionsOpen={moreOptionsOpen}
        setMoreOptionsOpen={setMoreOptionsOpen}
        videoRef={props.videoRef}
        videoPath={props.videoPath}
        currentProjectId={props.currentProjectId}
        currentTime={props.currentTime}
        duration={props.duration}
        setCurrentTime={props.setCurrentTime}
        setDuration={props.setDuration}
        words={props.words}
        pipelineRunning={props.pipelineRunning}
        pipelineProgress={props.pipelineProgress}
        pipelineStatus={props.pipelineStatus}
        detectedLanguage={props.detectedLanguage}
        captionStyle={props.captionStyle}
        setCaptionStyle={props.setCaptionStyle}
        captionThemeId={props.captionThemeId}
        setCaptionThemeId={props.setCaptionThemeId}
        resetCaptionStyleToFactoryDefault={props.resetCaptionStyleToFactoryDefault}
        captionStyleOverrides={props.captionStyleOverrides}
        addCaptionStyleOverride={props.addCaptionStyleOverride}
        removeCaptionStyleOverride={props.removeCaptionStyleOverride}
        updateCaptionStyleOverride={props.updateCaptionStyleOverride}
        videoTransitions={props.videoTransitions}
        addVideoTransition={props.addVideoTransition}
        updateVideoTransition={props.updateVideoTransition}
        removeVideoTransition={props.removeVideoTransition}
        suggestTransitionPlan={props.suggestTransitionPlan}
        suggestingTransitionPlan={props.suggestingTransitionPlan}
        transitionPlanError={props.transitionPlanError}
        handleSeek={props.handleSeek}
        handleJumpCutApplied={props.handleJumpCutApplied}
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
        voiceoverPath={props.voiceoverPath}
        voiceoverOffset={props.voiceoverOffset}
        onVoiceoverReady={props.onVoiceoverReady}
        musicPath={props.musicPath}
        duckLevel={props.duckLevel}
        onMusicBedChange={props.onMusicBedChange}
        onCaptionsFromRecording={props.onCaptionsFromRecording}
        contentIdeas={props.contentIdeas}
        contentStrategyOptions={props.contentStrategyOptions}
        contentHints={props.contentHints}
        setContentHints={props.setContentHints}
        selectContentStrategyOption={props.selectContentStrategyOption}
        generatingContentIdeas={props.generatingContentIdeas}
        contentIdeasError={props.contentIdeasError}
        generateContentIdeas={props.generateContentIdeas}
      />

      <TranscriptPanel
        words={props.words}
        currentTime={props.currentTime}
        onWordChange={props.handleWordChange}
        onWordDelete={props.handleWordDelete}
        onSeek={props.handleSeek}
        currentProjectId={props.currentProjectId}
        onDeleteProject={props.deleteCurrentProject}
        videoPath={props.videoPath}
        pipelineRunning={props.pipelineRunning}
        pipelineStatus={props.pipelineStatus}
        onTranscribe={props.retranscribe}
        captionStyle={props.captionStyle}
        captionThemeId={props.captionThemeId}
        onOpenStylePicker={() => setMoreOptionsOpen(true)}
      />

      <BurnExportButton
        burnCaptions={props.burnCaptions}
        burning={props.burning}
        burnProgress={props.burnProgress}
        burnStatus={props.burnStatus}
        disabled={props.words.length === 0}
        lastBurnedPath={props.lastBurnedPath}
        contentIdeas={props.contentIdeas}
        currentProjectId={props.currentProjectId}
      />

      <div className="bottom-left-stack">
        <BackgroundJobsBanner />
        <OptionalModelsBanner />
        <UpdateBanner />
      </div>
    </div>
  );
}

export default AppShell;
