import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

function DeveloperToolsPanel() {
  const [greeting, setGreeting] = useState("");
  const [ffmpegVersion, setFfmpegVersion] = useState("");
  const [checking, setChecking] = useState(false);

  async function sayHello() {
    const result = await invoke("greet", { name: "Reels Creator" });
    setGreeting(result);
  }

  async function checkFfmpeg() {
    setChecking(true);
    try {
      setFfmpegVersion(await invoke("check_ffmpeg"));
    } catch (err) {
      setFfmpegVersion(`Error: ${err}`);
    } finally {
      setChecking(false);
    }
  }

  return (
    <div className="dev-tools-panel">
      <div className="dev-tools-check">
        <h3>Rust ↔ React bridge</h3>
        <button onClick={sayHello}>Say hello from Rust</button>
        {greeting && <p className="result">{greeting}</p>}
      </div>

      <div className="dev-tools-check">
        <h3>Check ffmpeg (video engine)</h3>
        <button onClick={checkFfmpeg} disabled={checking}>
          Check ffmpeg version
        </button>
        {ffmpegVersion && <pre className="result">{ffmpegVersion}</pre>}
      </div>
    </div>
  );
}

export default DeveloperToolsPanel;
