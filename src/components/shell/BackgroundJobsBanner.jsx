import { useCurrentProjectId, useAnyProjectProcessing } from "../../state/projectStore.js";

// A small, quiet summary of background work -- true multi-project
// processing (see src/state/projectStore.js) means a project can keep
// transcribing/generating/burning while a completely different one is
// displayed, with no other indication anywhere in the main view that
// anything is happening elsewhere. Self-contained (reads the store
// directly, same pattern as OptionalModelsBanner.jsx/UpdateBanner.jsx) so
// it needs no props threaded through App.jsx/AppShell.jsx.
//
// Deliberately only counts jobs for projects OTHER than the one currently
// displayed -- the displayed project's own progress already has a real
// progress bar in the main panel (TranscribeStatus, Burn & Export, ...),
// so surfacing it a second time here would just be noise.
function BackgroundJobsBanner() {
  const currentProjectId = useCurrentProjectId();
  const processingIds = useAnyProjectProcessing();
  const backgroundCount = processingIds.filter((id) => id !== currentProjectId).length;

  if (backgroundCount === 0) return null;

  return (
    <div className="background-jobs-banner">
      <span className="project-processing-badge" />
      {backgroundCount} other video{backgroundCount === 1 ? "" : "s"} processing in the background
    </div>
  );
}

export default BackgroundJobsBanner;
