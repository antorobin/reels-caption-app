import BackgroundJobsBanner from "./BackgroundJobsBanner.jsx";
import BurnExportButton from "./BurnExportButton.jsx";
import MainPanel from "./MainPanel.jsx";
import MenuBar from "./MenuBar.jsx";
import OptionalModelsBanner from "./OptionalModelsBanner.jsx";
import Sidebar from "./Sidebar.jsx";
import TranscriptPanel from "./TranscriptPanel.jsx";
import UpdateBanner from "./UpdateBanner.jsx";

function AppShell(props) {
  return (
    <div className="shell-grid">
      <MenuBar />

      <Sidebar
        videoPath={props.videoPath}
        originalVideoPath={props.originalVideoPath}
        onPickVideo={props.pickVideo}
        importing={props.importing}
        importProgress={props.importProgress}
        currentProjectId={props.currentProjectId}
        onSelectProject={props.loadProject}
        projectTitle={props.projectTitle}
        setProjectTitle={props.setProjectTitle}
        projectDescription={props.projectDescription}
        setProjectDescription={props.setProjectDescription}
        projectHashtags={props.projectHashtags}
        setProjectHashtags={props.setProjectHashtags}
        generatingContentIdeas={props.generatingContentIdeas}
      />

      <MainPanel
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
      />

      <BurnExportButton
        burnCaptions={props.burnCaptions}
        burning={props.burning}
        burnProgress={props.burnProgress}
        burnStatus={props.burnStatus}
        disabled={props.words.length === 0}
        lastBurnedPath={props.lastBurnedPath}
        contentIdeas={props.contentIdeas}
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
