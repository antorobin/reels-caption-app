import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { disable as disableAutostart, enable as enableAutostart, isEnabled as isAutostartEnabled } from "@tauri-apps/plugin-autostart";

function DeveloperToolsPanel() {
  const [greeting, setGreeting] = useState("");
  const [ffmpegVersion, setFfmpegVersion] = useState("");
  const [checking, setChecking] = useState(false);
  const [autostart, setAutostart] = useState(false);
  const [autostartBusy, setAutostartBusy] = useState(false);

  useEffect(() => {
    isAutostartEnabled().then(setAutostart).catch(() => {});
  }, []);

  async function toggleAutostart() {
    setAutostartBusy(true);
    try {
      if (autostart) {
        await disableAutostart();
      } else {
        await enableAutostart();
      }
      setAutostart(await isAutostartEnabled());
    } catch (err) {
      console.error("Couldn't change launch-at-login setting:", err);
    } finally {
      setAutostartBusy(false);
    }
  }

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

      <div className="dev-tools-check">
        <h3>Background &amp; startup</h3>
        <label className="checkbox-row">
          <input type="checkbox" checked={autostart} onChange={toggleAutostart} disabled={autostartBusy} />
          Launch at login
        </label>
        <p className="section-hint">
          Closing the window keeps the app running in the system tray (for scheduled posts) instead of quitting —
          use the tray icon's Quit to fully exit.
        </p>
      </div>
    </div>
  );
}

export default DeveloperToolsPanel;
