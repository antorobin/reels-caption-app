import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";

const IMAGE_FILTERS = [{ name: "Image", extensions: ["png", "jpg", "jpeg", "webp"] }];

const GOAL_OPTIONS = ["Awareness", "Engagement", "Sales/Conversion", "Education", "Entertainment", "Other"];
const TONE_OPTIONS = ["Professional", "Casual", "Humorous", "Inspirational", "Urgent/Dramatic", "Educational"];
const LENGTH_OPTIONS = ["15s", "30s", "45s", "60s"];
// Reuses music_gen.rs's own existing 3-category framing (its own doc
// comment has the real, hard-won reasoning for exactly these 3 -- a plain
// "pick a mood" ask collapses to near-duplicate suggestions at this
// model's size, naming genuinely different categories fixed it) so this
// maps directly onto that module's real prompt categories once
// generation is actually wired up, rather than inventing a second,
// parallel mood vocabulary.
const MUSIC_MOOD_OPTIONS = ["Acoustic/organic", "Electronic/synth-based", "Percussion/rhythm-driven"];

// AI content strategy is a Pro-plan feature -- no billing/subscription
// system exists in this app yet (confirmed: no isPro/tier field anywhere
// in AuthContext.jsx or elsewhere), so this is a visual gate only: the
// pricing plans are shown so the user knows what unlocks the tab, and the
// form itself is rendered but fully disabled (see the <fieldset disabled>
// below) rather than hidden -- letting someone see exactly what they'd be
// filling in is a stronger upgrade nudge than an empty locked panel.
//
// INR pricing: derived from this app's own earlier USD plan (Basic $12,
// Pro $39, Enterprise from $299, built on real fal.ai per-second video-
// generation costs -- see README/plan history) at a ~₹87/$1 rate, then
// rounded to native Indian SaaS price anchors (₹999/₹2,999/custom)
// instead of a literal FX conversion -- ₹999 reads as a normal India
// price point, not an odd converted one, while preserving the same
// underlying margin against real per-second compute cost.
const PRICING_PLANS = [
  {
    id: "basic",
    name: "Basic",
    subtitle: "Solo creator",
    price: "₹999",
    seconds: "150 AI video-seconds / mo",
    features: ["1 seat", "Captions, editing & export tools", "AI content strategy — locked"],
    highlighted: false,
  },
  {
    id: "pro",
    name: "Pro",
    subtitle: "Startup / small team",
    price: "₹2,999",
    seconds: "500 AI video-seconds / mo",
    features: ["Up to 5 seats (shared pool)", "Everything in Basic", "✅ AI content strategy unlocked"],
    highlighted: true,
  },
  {
    id: "enterprise",
    name: "Enterprise",
    subtitle: "50+ members",
    price: "From ₹24,999",
    seconds: "5,000+ AI video-seconds / mo",
    features: ["Unlimited seats", "Volume-discounted rate", "✅ AI content strategy unlocked"],
    highlighted: false,
  },
];

function PricingBanner() {
  return (
    <div className="strategy-pricing-banner">
      <span className="strategy-pricing-badge">🔒 Pro feature</span>
      <h3 className="strategy-pricing-title">AI content strategy is a Pro-plan feature</h3>
      <p className="strategy-pricing-sub">
        Upgrade to unlock AI-generated video strategy, script, and scene planning. The form below is shown as a
        preview only until then.
      </p>
      <div className="strategy-pricing-grid">
        {PRICING_PLANS.map((plan) => (
          <div key={plan.id} className={plan.highlighted ? "strategy-pricing-card highlighted" : "strategy-pricing-card"}>
            {plan.highlighted && <div className="strategy-pricing-ribbon">Required</div>}
            <div className="strategy-pricing-plan-name">{plan.name}</div>
            <div className="strategy-pricing-card-subtitle">{plan.subtitle}</div>
            <div className="strategy-pricing-price">
              {plan.price}
              <span>/mo</span>
            </div>
            <div className="strategy-pricing-seconds">{plan.seconds}</div>
            <ul className="strategy-pricing-features">
              {plan.features.map((f) => (
                <li key={f}>{f}</li>
              ))}
            </ul>
          </div>
        ))}
      </div>
      <button type="button" className="strategy-pricing-upgrade-btn" disabled title="Billing isn't wired up yet">
        Upgrade to Pro — coming soon
      </button>
    </div>
  );
}

function emptyDraftFields() {
  return {
    projectName: "",
    brief: "",
    goal: GOAL_OPTIONS[0],
    goalOther: "",
    country: "",
    audience: "",
    industry: "",
    culture: "",
    language: "",
    characterDescription: "",
    characterPhotoPath: null, // a local OS path picked but not yet uploaded, or an already-stored app-managed one when editing
    visualStyle: "",
    brandColors: "",
    tone: TONE_OPTIONS[0],
    targetLength: LENGTH_OPTIONS[1],
    musicMood: MUSIC_MOOD_OPTIONS[0],
    hints: "",
  };
}

function draftFieldsFromExisting(editingDraft) {
  const saved = editingDraft.state?.contentStrategyDraft || {};
  return {
    ...emptyDraftFields(),
    ...saved,
    projectName: editingDraft.title || "",
    brief: editingDraft.description || "",
    characterPhotoPath: saved.characterPhotoPath || null,
  };
}

// The "+" button's popup -- requested directly, replacing the old
// always-visible "Choose video(s)…" button entirely. Two tabs: a thin
// wrapper around the existing multi-file `onPickVideo` flow (unchanged),
// and a new AI content-strategy intake form. That second tab only ever
// *collects* parameters today -- actual video generation is explicitly
// deferred to a future cloud-API integration (continuing the earlier,
// paused local-model investigation once a real probe showed it wasn't
// CPU-feasible at the planned speed) -- submitting it saves a real,
// visible "Drafted" project (library.rs's `create_strategy_draft`) rather
// than a silent file on disk, so there's something real to come back to
// once generation exists.
//
// Doubles as the *edit* flow for an already-saved draft: pass the
// existing project as `editingDraft` and this opens straight to the
// strategy tab, pre-filled, with Save calling `create_strategy_draft`
// again with that same `id` (an upsert -- see its own doc comment for
// why one command handles both, rather than a separate update path).
function NewProjectModal({ onPickVideo, editingDraft, onClose, onSaved }) {
  const [activeTab, setActiveTab] = useState(editingDraft ? "strategy" : "upload");
  const [importing, setImporting] = useState(false);
  const [fields, setFields] = useState(() => (editingDraft ? draftFieldsFromExisting(editingDraft) : emptyDraftFields()));
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState("");

  function set(key, value) {
    setFields((prev) => ({ ...prev, [key]: value }));
  }

  async function handleUpload() {
    setImporting(true);
    try {
      await onPickVideo();
      onSaved();
      onClose();
    } finally {
      setImporting(false);
    }
  }

  async function pickCharacterPhoto() {
    const selected = await open({ multiple: false, filters: IMAGE_FILTERS });
    if (typeof selected === "string") set("characterPhotoPath", selected);
  }

  async function handleSaveDraft() {
    if (!fields.projectName.trim() || !fields.brief.trim()) {
      setError("Give the draft a project name and a content brief first.");
      return;
    }
    setSubmitting(true);
    setError("");
    const { projectName, brief, characterPhotoPath, ...params } = fields;
    // Only pass a *new* photo path along -- if it's exactly what was
    // already stored (editing a draft without touching the photo), the
    // backend keeps its own already-saved copy rather than re-copying a
    // file onto itself.
    const isNewPhoto = characterPhotoPath && characterPhotoPath !== (editingDraft?.state?.contentStrategyDraft?.characterPhotoPath ?? null);
    try {
      await invoke("create_strategy_draft", {
        id: editingDraft?.id ?? null,
        title: projectName,
        description: brief,
        params,
        characterPhotoPath: isNewPhoto ? characterPhotoPath : null,
      });
      onSaved();
      onClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="shell-modal-backdrop" onClick={onClose}>
      <div className="shell-modal new-project-modal" onClick={(e) => e.stopPropagation()}>
        <div className="shell-modal-header">
          <h2>{editingDraft ? "Edit content strategy" : "New"}</h2>
          <button type="button" className="shell-modal-close" onClick={onClose}>
            ✕
          </button>
        </div>

        {!editingDraft && (
          <div className="more-options-tabs">
            <button
              type="button"
              className={activeTab === "upload" ? "more-options-tab active" : "more-options-tab"}
              onClick={() => setActiveTab("upload")}
            >
              Upload video
            </button>
            <button
              type="button"
              className={activeTab === "strategy" ? "more-options-tab active" : "more-options-tab"}
              onClick={() => setActiveTab("strategy")}
            >
              ✨ AI content strategy 🔒
            </button>
          </div>
        )}

        {activeTab === "upload" && (
          <div className="new-project-upload-tab">
            <p className="section-hint">Choose one or more video files to import as new projects.</p>
            <button type="button" onClick={handleUpload} disabled={importing}>
              {importing ? "Importing…" : "Choose video(s)…"}
            </button>
          </div>
        )}

        {activeTab === "strategy" && (
          <div className="new-project-strategy-tab">
            <PricingBanner />

            <fieldset className="strategy-form-gated" disabled>
            <p className="section-hint">
              Collects everything a future AI video-generation pipeline will need. Actual generation isn't built yet
              (planned for a future cloud-API integration) — this saves your parameters as a Drafted project you can
              come back to and edit any time before then.
            </p>

            <h3 className="new-project-section-heading">Core brief</h3>
            <label className="edit-project-field">
              Project name
              <input value={fields.projectName} onChange={(e) => set("projectName", e.target.value)} placeholder="e.g. Q4 Product Launch" />
            </label>
            <label className="edit-project-field">
              Content brief
              <textarea
                value={fields.brief}
                onChange={(e) => set("brief", e.target.value)}
                rows={3}
                placeholder="What should this video be about?"
              />
            </label>
            <label className="edit-project-field">
              Goal / call-to-action
              <select value={fields.goal} onChange={(e) => set("goal", e.target.value)}>
                {GOAL_OPTIONS.map((g) => (
                  <option key={g} value={g}>
                    {g}
                  </option>
                ))}
              </select>
            </label>
            {fields.goal === "Other" && (
              <label className="edit-project-field">
                Describe the goal
                <input value={fields.goalOther} onChange={(e) => set("goalOther", e.target.value)} />
              </label>
            )}

            <h3 className="new-project-section-heading">Audience &amp; market</h3>
            <label className="edit-project-field">
              Target country / market
              <input value={fields.country} onChange={(e) => set("country", e.target.value)} placeholder="e.g. India, United States" />
            </label>
            <label className="edit-project-field">
              Target audience
              <input value={fields.audience} onChange={(e) => set("audience", e.target.value)} placeholder="e.g. 18-34, young professionals" />
            </label>
            <label className="edit-project-field">
              Industry / sector
              <input value={fields.industry} onChange={(e) => set("industry", e.target.value)} placeholder="e.g. Fitness, SaaS, Food & Beverage" />
            </label>
            <label className="edit-project-field">
              Culture / cultural context
              <input value={fields.culture} onChange={(e) => set("culture", e.target.value)} placeholder="e.g. Tamil Nadu, India" />
            </label>
            <label className="edit-project-field">
              Spoken language
              <input value={fields.language} onChange={(e) => set("language", e.target.value)} placeholder="e.g. English, Tamil" />
            </label>

            <h3 className="new-project-section-heading">Character &amp; visual style</h3>
            <label className="edit-project-field">
              Character description
              <textarea
                value={fields.characterDescription}
                onChange={(e) => set("characterDescription", e.target.value)}
                rows={2}
                placeholder="Who appears/narrates? Personality, role, look…"
              />
            </label>
            <div className="edit-project-field">
              Reference photo (optional)
              <div className="new-project-photo-row">
                <button type="button" onClick={pickCharacterPhoto}>
                  {fields.characterPhotoPath ? "Change photo…" : "Choose photo…"}
                </button>
                {fields.characterPhotoPath && (
                  <span className="new-project-photo-name">{fields.characterPhotoPath.split(/[\\/]/).pop()}</span>
                )}
              </div>
            </div>
            <label className="edit-project-field">
              Visual style / mood
              <input
                value={fields.visualStyle}
                onChange={(e) => set("visualStyle", e.target.value)}
                placeholder="e.g. cinematic, bright and playful, minimalist corporate"
              />
            </label>
            <label className="edit-project-field">
              Brand colors (optional)
              <input value={fields.brandColors} onChange={(e) => set("brandColors", e.target.value)} placeholder="e.g. #1a73e8, navy and gold" />
            </label>

            <h3 className="new-project-section-heading">Tone &amp; format</h3>
            <label className="edit-project-field">
              Tone
              <select value={fields.tone} onChange={(e) => set("tone", e.target.value)}>
                {TONE_OPTIONS.map((t) => (
                  <option key={t} value={t}>
                    {t}
                  </option>
                ))}
              </select>
            </label>
            <label className="edit-project-field">
              Target length
              <select value={fields.targetLength} onChange={(e) => set("targetLength", e.target.value)}>
                {LENGTH_OPTIONS.map((l) => (
                  <option key={l} value={l}>
                    {l}
                  </option>
                ))}
              </select>
            </label>
            <label className="edit-project-field">
              Music mood
              <select value={fields.musicMood} onChange={(e) => set("musicMood", e.target.value)}>
                {MUSIC_MOOD_OPTIONS.map((m) => (
                  <option key={m} value={m}>
                    {m}
                  </option>
                ))}
              </select>
            </label>

            <h3 className="new-project-section-heading">Extra hints (optional)</h3>
            <label className="edit-project-field">
              Additional hints / themes to emphasize
              <input value={fields.hints} onChange={(e) => set("hints", e.target.value)} placeholder="Anything else to steer the strategy" />
            </label>

            {error && <p className="caption-override-panel-error">{error}</p>}
            </fieldset>

            <div className="edit-project-modal-actions">
              <button type="button" onClick={onClose}>
                Cancel
              </button>
              <button
                type="button"
                className="edit-project-save"
                onClick={handleSaveDraft}
                disabled
                title="Upgrade to Pro to save an AI content strategy draft"
              >
                Save draft
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

export default NewProjectModal;
