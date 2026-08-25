import { useState } from "react";
import { useAuth } from "../../context/AuthContext.jsx";
import DeveloperToolsPanel from "./DeveloperToolsPanel.jsx";

function MenuBar() {
  const [devToolsOpen, setDevToolsOpen] = useState(false);
  const { user, logout } = useAuth();

  return (
    <header className="shell-menubar">
      <span className="shell-menubar-brand">🎬 Reels Caption App</span>
      <nav className="shell-menubar-menu">
        <button type="button" className="shell-menubar-item" onClick={() => setDevToolsOpen(true)}>
          Developer
        </button>
      </nav>

      {user && (
        <div className="shell-menubar-user">
          {user.photoURL && <img className="shell-menubar-avatar" src={user.photoURL} alt="" />}
          <span>{user.displayName || user.email}</span>
          <button type="button" className="shell-menubar-item" onClick={logout}>
            Log out
          </button>
        </div>
      )}

      {devToolsOpen && (
        <div className="shell-modal-backdrop" onClick={() => setDevToolsOpen(false)}>
          <div className="shell-modal" onClick={(e) => e.stopPropagation()}>
            <div className="shell-modal-header">
              <h2>Developer tools</h2>
              <button type="button" className="shell-modal-close" onClick={() => setDevToolsOpen(false)}>
                ✕
              </button>
            </div>
            <DeveloperToolsPanel />
          </div>
        </div>
      )}
    </header>
  );
}

export default MenuBar;
