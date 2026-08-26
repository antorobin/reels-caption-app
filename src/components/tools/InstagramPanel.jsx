import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

const REDIRECT_URI = "https://localhost:47829/instagram/callback";

// Full walkthrough condensed for in-app display -- the same steps as the
// README's section 9.1, verified interactively against a real Meta app
// while building this panel. Kept here (not just in the README) since
// "what's an App ID" is exactly the question someone lands on this panel
// asking, not one they'll go dig through docs for first.
function SetupGuide() {
  function openExternal(url) {
    invoke("open_external_url", { url }).catch(() => {});
  }

  return (
    <div className="instagram-setup-guide">
      <ol>
        <li>
          <strong>Instagram:</strong> profile → <em>Edit Profile → Switch to Professional Account</em> → choose{" "}
          <strong>Creator</strong> or <strong>Business</strong>.
        </li>
        <li>
          <strong>Facebook Page:</strong>{" "}
          <button type="button" className="link-button" onClick={() => openExternal("https://www.facebook.com/pages/creation/")}>
            Create a Facebook Page
          </button>{" "}
          — name it anything, pick a category. No followers or posts needed.
        </li>
        <li>
          <strong>Link them:</strong> Facebook → profile picture → <em>Settings &amp; Privacy → Settings → Accounts
          Center → Add accounts → Instagram</em> — sign into the same Instagram account.
        </li>
        <li>
          <strong>Meta Developer App:</strong>{" "}
          <button type="button" className="link-button" onClick={() => openExternal("https://developers.facebook.com/apps/")}>
            Create an app at developers.facebook.com
          </button>{" "}
          — type <strong>Business</strong>, name it.
        </li>
        <li>
          <strong>Add the use case:</strong> <em>Add use cases</em> → filter <em>All</em> or <em>Content management</em> →
          select <strong>"Manage messaging &amp; content on Instagram"</strong> (not Facebook Login, not Marketing API).
        </li>
        <li>
          <strong>Add the specific permissions</strong> (this step is easy to miss — the use case above doesn't add
          these automatically): open the use case → <em>Permissions and features</em> → click <strong>+ Add</strong>{" "}
          on exactly these three:
          <ul>
            <li>
              <code>pages_show_list</code>
            </li>
            <li>
              <code>instagram_basic</code>
            </li>
            <li>
              <code>instagram_content_publish</code>
            </li>
          </ul>
          Leave the <code>instagram_business_*</code> ones alone — those belong to a different, newer login product
          this app doesn't use. Without adding these three, sign-in fails with "Invalid Scopes" even though the
          names are otherwise correct.
        </li>
        <li>
          <strong>Get your credentials:</strong> <em>App settings → Basic</em> → copy the <strong>App ID</strong> and{" "}
          <strong>App Secret</strong> (click "Show") — paste them below.
        </li>
        <li>
          <strong>Redirect URI:</strong> <em>App settings → Advanced → App authentication</em> → toggle{" "}
          <strong>"Native or desktop app?" ON</strong>, set <em>Authorize callback URL</em> to exactly:
          <pre className="result">{REDIRECT_URI}</pre>
          It must be <strong>https</strong>, not http — Meta rejects a plain http redirect even for localhost.
          Your browser will show a one-time "connection isn't private" warning on that final step since it's a
          self-signed local certificate; click through it — that's expected, not a sign anything's wrong. Leave{" "}
          <strong>"App secret embedded in client" OFF</strong> — that's for apps that ship the secret publicly,
          which this one never does.
        </li>
        <li>
          <strong>Add yourself as a Tester</strong> (skips Meta's 2-4 week App Review): <em>App roles → Roles →
          Instagram Testers → Add Instagram Testers</em> → your username → send invite. Then accept it{" "}
          <em>from Instagram</em>: profile → <em>Settings → Apps and Websites → Tester Invites → Accept</em>.
        </li>
      </ol>
    </div>
  );
}

// Instagram account connection (the official Graph API OAuth flow --
// instagram.rs's own doc comment explains why, and what this deliberately
// doesn't do). App ID/Secret are saved locally (this machine's app-data
// folder, never the git repo, never compiled into the app) via
// save_instagram_app_config before "Connect Instagram" can run at all.
function InstagramPanel() {
  const [hasConfig, setHasConfig] = useState(false);
  const [appId, setAppId] = useState("");
  const [appSecret, setAppSecret] = useState("");
  const [savingConfig, setSavingConfig] = useState(false);
  const [account, setAccount] = useState(null);
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState("");
  const [showGuide, setShowGuide] = useState(true);

  useEffect(() => {
    invoke("has_instagram_app_config").then(setHasConfig).catch(() => {});
    invoke("get_connected_instagram_account").then(setAccount).catch(() => {});
  }, []);

  async function saveConfig() {
    setSavingConfig(true);
    setError("");
    try {
      await invoke("save_instagram_app_config", { appId: appId.trim(), appSecret: appSecret.trim() });
      setHasConfig(true);
      setAppSecret(""); // never keep the secret sitting in a form field longer than it has to
    } catch (err) {
      setError(String(err));
    } finally {
      setSavingConfig(false);
    }
  }

  async function connect() {
    setConnecting(true);
    setError("");
    try {
      const result = await invoke("connect_instagram_account");
      setAccount(result);
    } catch (err) {
      setError(String(err));
    } finally {
      setConnecting(false);
    }
  }

  async function disconnect() {
    await invoke("disconnect_instagram_account").catch(() => {});
    setAccount(null);
  }

  return (
    <div className="inspector-panel">
      <h2>Instagram</h2>
      <p className="section-hint">
        Connects via Meta's official Graph API (the same sanctioned path Buffer/Later use) -- only works for an
        Instagram Business or Creator account linked to a Facebook Page. Needs your own free Meta Developer App;
        nothing here is shared across users.
      </p>

      {!hasConfig && (
        <>
          <button type="button" className="link-button" onClick={() => setShowGuide((v) => !v)}>
            {showGuide ? "Hide setup steps" : "Don't have an App ID/Secret yet? Show setup steps"}
          </button>
          {showGuide && <SetupGuide />}

          <label className="field-row">
            Meta App ID
            <input type="text" value={appId} onChange={(e) => setAppId(e.target.value)} placeholder="e.g. 948785624912753" />
          </label>
          <label className="field-row">
            Meta App Secret
            <input type="password" value={appSecret} onChange={(e) => setAppSecret(e.target.value)} />
          </label>
          <button onClick={saveConfig} disabled={!appId.trim() || !appSecret.trim() || savingConfig}>
            {savingConfig ? "Saving…" : "Save"}
          </button>
        </>
      )}

      {hasConfig && !account && (
        <button onClick={connect} disabled={connecting}>
          {connecting ? "Waiting for sign-in in your browser…" : "Connect Instagram"}
        </button>
      )}

      {hasConfig && account && (
        <>
          <p className="result result-suggestion">Connected as @{account.username}</p>
          <button onClick={disconnect}>Disconnect</button>
        </>
      )}

      {hasConfig && (
        <button type="button" className="link-button" onClick={() => setHasConfig(false)} disabled={connecting}>
          Change App ID / Secret
        </button>
      )}

      {error && <pre className="result">{error}</pre>}
    </div>
  );
}

export default InstagramPanel;
