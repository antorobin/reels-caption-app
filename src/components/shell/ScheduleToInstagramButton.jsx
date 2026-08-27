import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

function formatWhen(unixSeconds) {
  return new Date(unixSeconds * 1000).toLocaleString();
}

// Mirrors the title line ContentIdeasPanel itself renders
// (`${emoji.join(" ")} ${title}`), plus the hashtags on their own line --
// title and hashtags specifically, not the longer description/hook, since
// that's what actually belongs in an Instagram caption.
function composeCaptionFromContentIdeas(ideas) {
  if (!ideas) return "";
  const titleLine = `${(ideas.emoji ?? []).join(" ")} ${ideas.title ?? ""}`.trim();
  const hashtagLine = (ideas.hashtags ?? []).join(" ");
  return [titleLine, hashtagLine].filter(Boolean).join("\n\n");
}

// Sits next to Burn & Export (see BurnExportButton.jsx) -- the natural
// next step once a captioned video is actually on disk. Adapts its own
// label ("Connect Instagram" vs "Schedule to Instagram") from the same
// connection state InstagramPanel.jsx tracks, so there's no separate trip
// to Settings needed just to see whether an account is hooked up yet.
function ScheduleToInstagramButton({ videoPath, contentIdeas }) {
  const [open, setOpen] = useState(false);
  const [hasConfig, setHasConfig] = useState(false);
  const [account, setAccount] = useState(null);
  const [connecting, setConnecting] = useState(false);
  const [schedules, setSchedules] = useState([]);
  const [caption, setCaption] = useState("");
  const [mode, setMode] = useState("now"); // "now" | "Once" | "Daily" | "Weekly"
  const [whenLocal, setWhenLocal] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");

  async function refresh() {
    const [config, acct, list] = await Promise.all([
      invoke("has_instagram_app_config").catch(() => false),
      invoke("get_connected_instagram_account").catch(() => null),
      invoke("list_scheduled_posts").catch(() => []),
    ]);
    setHasConfig(config);
    setAccount(acct);
    setSchedules(list || []);
  }

  // Fetch once on mount too (not just on open) so the trigger button's own
  // label already reflects real connection state before the user clicks it.
  useEffect(() => {
    refresh();
  }, []);

  useEffect(() => {
    if (open) {
      setError("");
      setMessage("");
      refresh();
      // Pre-fill from the AI-generated title/hashtags the first time this
      // opens with a caption still empty -- never overwrites something the
      // user already typed. "Use AI title & hashtags" below re-applies it
      // on demand afterward.
      setCaption((prev) => (prev ? prev : composeCaptionFromContentIdeas(contentIdeas)));
    }
  }, [open]);

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

  async function submit() {
    if (!videoPath) return;
    setSubmitting(true);
    setError("");
    setMessage("");
    try {
      if (mode === "now") {
        const result = await invoke("post_to_instagram_now", { videoPath, caption });
        setMessage(`Posted! Media id ${result.media_id}.`);
      } else {
        if (!whenLocal) {
          setError("Pick a date and time first.");
          setSubmitting(false);
          return;
        }
        const firstRun = Math.floor(new Date(whenLocal).getTime() / 1000);
        await invoke("schedule_instagram_post", { videoPath, caption, recurrence: mode, firstRun });
        setMessage(mode === "Once" ? "Scheduled." : "Added to the recurring queue.");
        setCaption("");
        setWhenLocal("");
      }
      await refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setSubmitting(false);
    }
  }

  async function removeSchedule(id) {
    await invoke("delete_scheduled_post", { id }).catch(() => {});
    refresh();
  }

  const connected = !!account;

  return (
    <>
      <button
        type="button"
        className="schedule-instagram-button"
        onClick={() => setOpen(true)}
        disabled={!videoPath}
        title={videoPath ? undefined : "Burn & Export a video first"}
      >
        {connected ? "Schedule to Instagram" : "Connect Instagram"}
      </button>

      {open && (
        <div className="shell-modal-backdrop" onClick={() => setOpen(false)}>
          <div className="shell-modal" onClick={(e) => e.stopPropagation()}>
            <div className="shell-modal-header">
              <h2>Schedule to Instagram</h2>
              <button type="button" className="shell-modal-close" onClick={() => setOpen(false)}>
                ✕
              </button>
            </div>

            <div className="inspector-panel">
              {!hasConfig && (
                <p className="section-hint">
                  Set up your Meta App ID/Secret in <strong>Settings → Instagram</strong> first.
                </p>
              )}

              {hasConfig && !connected && (
                <>
                  <p className="section-hint">Connect an Instagram account before scheduling a post.</p>
                  <button type="button" onClick={connect} disabled={connecting}>
                    {connecting ? "Waiting for sign-in in your browser…" : "Connect Instagram"}
                  </button>
                </>
              )}

              {connected && (
                <>
                  <p className="section-hint">Posting as @{account.username}.</p>

                  <label className="field-row">
                    Caption
                    <textarea
                      rows={3}
                      value={caption}
                      onChange={(e) => setCaption(e.target.value)}
                      placeholder="Write a caption…"
                    />
                  </label>
                  {contentIdeas && (
                    <button
                      type="button"
                      className="link-button"
                      onClick={() => setCaption(composeCaptionFromContentIdeas(contentIdeas))}
                    >
                      Use AI title &amp; hashtags
                    </button>
                  )}

                  <label className="field-row">
                    When
                    <select value={mode} onChange={(e) => setMode(e.target.value)}>
                      <option value="now">Post now</option>
                      <option value="Once">Schedule once</option>
                      <option value="Daily">Add to daily recurring slot</option>
                      <option value="Weekly">Add to weekly recurring slot</option>
                    </select>
                  </label>

                  {mode !== "now" && (
                    <label className="field-row">
                      {mode === "Once" ? "Post at" : "First post at (sets the time of day for every future post too)"}
                      <input type="datetime-local" value={whenLocal} onChange={(e) => setWhenLocal(e.target.value)} />
                    </label>
                  )}

                  <button type="button" onClick={submit} disabled={submitting || !videoPath}>
                    {submitting ? "Working…" : mode === "now" ? "Post now" : "Schedule"}
                  </button>

                  {schedules.length > 0 && (
                    <div className="schedule-list">
                      <h3>Upcoming</h3>
                      {schedules.map((s) => (
                        <div key={s.id} className="schedule-list-item">
                          <div>
                            <strong>{s.recurrence}</strong> — {s.status}
                            <br />
                            Next: {formatWhen(s.next_run)} · Queue: {s.queue.length}
                            {s.last_result && (
                              <>
                                <br />
                                <span className="section-hint">{s.last_result}</span>
                              </>
                            )}
                          </div>
                          <button type="button" className="link-button" onClick={() => removeSchedule(s.id)}>
                            Remove
                          </button>
                        </div>
                      ))}
                    </div>
                  )}
                </>
              )}

              {message && <p className="result result-suggestion">{message}</p>}
              {error && <pre className="result">{error}</pre>}
            </div>
          </div>
        </div>
      )}
    </>
  );
}

export default ScheduleToInstagramButton;
