import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { disable as disableAutostart, enable as enableAutostart, isEnabled as isAutostartEnabled } from "@tauri-apps/plugin-autostart";
import { relaunch } from "@tauri-apps/plugin-process";
import { check as checkForUpdate } from "@tauri-apps/plugin-updater";
import InstagramPanel from "../tools/InstagramPanel.jsx";

// Replaces the old "Developer" menu -- the two original Rust-bridge/ffmpeg
// checks are real, but they're not what most people opening this menu are
// after; Launch-at-login and Connect Instagram are the actual settings a
// normal user wants, front and center, with the original checks tucked
// behind a "Troubleshoot" toggle instead of competing for attention.
function SettingsPanel() {
  const [autostart, setAutostart] = useState(false);
  const [autostartBusy, setAutostartBusy] = useState(false);
  const [showTroubleshoot, setShowTroubleshoot] = useState(false);
  const [greeting, setGreeting] = useState("");
  const [ffmpegVersion, setFfmpegVersion] = useState("");
  const [checking, setChecking] = useState(false);

  const [appVersion, setAppVersion] = useState("");
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [updateStatus, setUpdateStatus] = useState("");
  const [availableUpdate, setAvailableUpdate] = useState(null);
  const [installingUpdate, setInstallingUpdate] = useState(false);
  const [updateProgress, setUpdateProgress] = useState(0);

  useEffect(() => {
    isAutostartEnabled().then(setAutostart).catch(() => {});
    getVersion().then(setAppVersion).catch(() => {});
  }, []);

  async function checkForUpdates() {
    setCheckingUpdate(true);
    setUpdateStatus("");
    setAvailableUpdate(null);
    try {
      const update = await checkForUpdate();
      if (update) {
        setAvailableUpdate(update);
        setUpdateStatus(`Version ${update.version} is available.`);
      } else {
        setUpdateStatus("You're up to date.");
      }
    } catch (err) {
      setUpdateStatus(`Error: ${err}`);
    } finally {
      setCheckingUpdate(false);
    }
  }

  async function installUpdate() {
    if (!availableUpdate) return;
    setInstallingUpdate(true);
    let downloaded = 0;
    let total = 0;
    try {
      await availableUpdate.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          if (total > 0) setUpdateProgress(Math.min(100, Math.round((downloaded / total) * 100)));
        }
      });
      await relaunch();
    } catch (err) {
      setUpdateStatus(`Error: ${err}`);
      setInstallingUpdate(false);
    }
  }

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

      <div className="dev-tools-check">
        <h3>Updates</h3>
        <p className="section-hint">
          {appVersion ? `You're running version ${appVersion}.` : ""} Checked automatically on launch — this is for
          checking again on demand.
        </p>
        <button type="button" onClick={checkForUpdates} disabled={checkingUpdate || installingUpdate}>
          {checkingUpdate ? "Checking…" : "Check for updates"}
        </button>
        {updateStatus && <p className="result">{updateStatus}</p>}
        {availableUpdate && (
          <button type="button" onClick={installUpdate} disabled={installingUpdate}>
            {installingUpdate ? `Installing… ${updateProgress}%` : "Download & Restart"}
          </button>
        )}
      </div>

      <InstagramPanel />

      <div className="dev-tools-check">
        <button type="button" className="link-button" onClick={() => setShowTroubleshoot((v) => !v)}>
          {showTroubleshoot ? "Hide troubleshooting" : "Troubleshoot"}
        </button>

        {showTroubleshoot && (
          <>
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
          </>
        )}
      </div>
    </div>
  );
}

export default SettingsPanel;
