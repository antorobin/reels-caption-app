import { useState } from "react";

// Requested directly, to keep the sidebar clean and minimal: project
// details (title/description/hashtags) used to be permanently-visible
// inline inputs at the top of the sidebar, always taking up space and
// always editable, whether or not anyone was actually editing them right
// now. Moved here -- a small popup opened only by clicking the pencil
// icon next to the project title -- same convention as every other modal
// in this app (`.shell-modal-backdrop`/`.shell-modal`, see
// MoreOptionsModal.jsx).
//
// Local draft state, committed on "Save" -- rather than the old inline
// inputs' every-keystroke-writes-straight-to-the-store behavior. That
// direct-write approach made sense for a permanently-open field; a modal
// that can be dismissed needs a real Cancel that actually discards
// unsaved changes, which means editing a local copy first.
function EditProjectModal({ title, description, hashtags, onSave, onClose }) {
  const [draftTitle, setDraftTitle] = useState(title);
  const [draftDescription, setDraftDescription] = useState(description);
  const [draftHashtags, setDraftHashtags] = useState(hashtags.join(" "));

  function handleSave() {
    onSave({
      title: draftTitle,
      description: draftDescription,
      hashtags: draftHashtags.split(/\s+/).filter(Boolean),
    });
    onClose();
  }

  return (
    <div className="shell-modal-backdrop" onClick={onClose}>
      <div className="shell-modal edit-project-modal" onClick={(e) => e.stopPropagation()}>
        <div className="shell-modal-header">
          <h2>Edit project</h2>
          <button type="button" className="shell-modal-close" onClick={onClose}>
            ✕
          </button>
        </div>

        <label className="edit-project-field">
          Title
          <input
            className="project-title-input"
            value={draftTitle}
            onChange={(e) => setDraftTitle(e.target.value)}
            placeholder="Title"
            autoFocus
          />
        </label>
        <label className="edit-project-field">
          Description
          <textarea
            className="project-description-input"
            value={draftDescription}
            onChange={(e) => setDraftDescription(e.target.value)}
            placeholder="Description"
            rows={3}
          />
        </label>
        <label className="edit-project-field">
          Hashtags
          <input
            className="project-hashtags-input"
            value={draftHashtags}
            onChange={(e) => setDraftHashtags(e.target.value)}
            placeholder="#hashtags #here"
          />
        </label>

        <div className="edit-project-modal-actions">
          <button type="button" onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="edit-project-save" onClick={handleSave}>
            Save
          </button>
        </div>
      </div>
    </div>
  );
}

export default EditProjectModal;
