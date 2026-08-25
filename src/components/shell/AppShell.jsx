import BurnExportButton from "./BurnExportButton.jsx";
import MainPanel from "./MainPanel.jsx";
import MenuBar from "./MenuBar.jsx";
import Sidebar from "./Sidebar.jsx";
import TranscriptPanel from "./TranscriptPanel.jsx";

function AppShell(props) {
  return (
    <div className="shell-grid">
      <MenuBar />

      <Sidebar videoPath={props.videoPath} onPickVideo={props.pickVideo} />

      <MainPanel
        videoRef={props.videoRef}
        videoPath={props.videoPath}
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
      />

      <TranscriptPanel
        words={props.words}
        currentTime={props.currentTime}
        onWordChange={props.handleWordChange}
        onWordDelete={props.handleWordDelete}
        onSeek={props.handleSeek}
      />

      <BurnExportButton
        burnCaptions={props.burnCaptions}
        burning={props.burning}
        burnProgress={props.burnProgress}
        burnStatus={props.burnStatus}
        disabled={props.words.length === 0}
      />
    </div>
  );
}

export default AppShell;
