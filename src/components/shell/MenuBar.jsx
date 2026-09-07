import { useState } from "react";
import { useAuth } from "../../context/AuthContext.jsx";
import SettingsPanel from "./SettingsPanel.jsx";

function MenuBar() {
  const [settingsOpen, setSettingsOpen] = useState(false);
  const { user, logout } = useAuth();

  return (
    <header className="shell-menubar">
      <span className="shell-menubar-brand">🎬 KraftReel.App</span>
      <nav className="shell-menubar-menu">
        <button type="button" className="shell-menubar-item" onClick={() => setSettingsOpen(true)}>
          Settings
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

      {settingsOpen && (
        <div className="shell-modal-backdrop" onClick={() => setSettingsOpen(false)}>
          <div className="shell-modal" onClick={(e) => e.stopPropagation()}>
            <div className="shell-modal-header">
              <h2>Settings</h2>
              <button type="button" className="shell-modal-close" onClick={() => setSettingsOpen(false)}>
                ✕
              </button>
            </div>
            <SettingsPanel />
          </div>
        </div>
      )}
    </header>
  );
}

export default MenuBar;
