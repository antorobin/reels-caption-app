import ReactDOM from "react-dom/client";
import DictationHud from "./components/DictationHud.jsx";
import "./dictation-hud.css";

// Deliberately no <React.StrictMode> here, unlike main.jsx. StrictMode's
// dev-only mount -> cleanup -> mount-again cycle is meant to surface missing
// cleanup for *idempotent* effects -- it's actively harmful for this one
// component, whose mount effect starts a real, non-idempotent backend
// session (spawns a native mic-capture thread and talks to a live worker
// process). The two near-simultaneous start_live_dictation calls that
// produces race over IPC -- whichever request reaches the Rust-side mutex
// second sees the first's session as already running and fails immediately,
// which is exactly the "A dictation session is already running." error a
// user hit on a completely fresh hotkey press. This window always mounts
// exactly once in production (StrictMode's double-invoke is dev-only
// anyway), so there's nothing to lose by skipping it here.
ReactDOM.createRoot(document.getElementById("root")).render(<DictationHud />);
