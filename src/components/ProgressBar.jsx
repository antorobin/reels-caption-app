import { useEffect, useRef, useState } from "react";

const STAGE_LABELS = {
  extracting_audio: "Extracting audio",
  transcribing: "Transcribing & aligning",
  burning: "Burning captions",
};

export function formatElapsed(seconds) {
  const whole = Math.floor(seconds);
  if (whole < 60) return `${whole}s`;
  const minutes = Math.floor(whole / 60);
  const rest = whole % 60;
  return `${minutes}m ${rest.toString().padStart(2, "0")}s`;
}

function ProgressBar({ progress }) {
  const isActive = Boolean(progress);
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  const startedAtRef = useRef(null);

  // A ticking timer independent of individual progress events — those can
  // arrive in bursts or go quiet for a beat, but the elapsed clock should
  // still count up smoothly. Keyed on "is a job running at all", not on
  // the progress object itself (which is a new reference on every event
  // and would otherwise restart this timer dozens of times a second).
  useEffect(() => {
    if (!isActive) {
      startedAtRef.current = null;
      setElapsedSeconds(0);
      return;
    }
    startedAtRef.current = Date.now();
    setElapsedSeconds(0);
    const interval = setInterval(() => {
      setElapsedSeconds((Date.now() - startedAtRef.current) / 1000);
    }, 250);
    return () => clearInterval(interval);
  }, [isActive]);

  if (!progress) return null;
  const { stage, percent, message } = progress;
  const label = STAGE_LABELS[stage] ?? stage;

  return (
    <div className="progress-row">
      <progress value={percent ?? undefined} max="100" />
      <span className="progress-label">
        {label}
        {percent != null ? ` — ${Math.round(percent)}%` : "…"}
        {` (${formatElapsed(elapsedSeconds)})`}
        {message ? ` — ${message}` : ""}
      </span>
    </div>
  );
}

export default ProgressBar;
