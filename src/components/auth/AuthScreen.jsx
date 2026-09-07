import { useState } from "react";
import { useAuth } from "../../context/AuthContext.jsx";

const ERROR_MESSAGES = {
  "auth/email-already-in-use": "That email already has an account — try logging in instead.",
  "auth/invalid-email": "That doesn't look like a valid email address.",
  "auth/weak-password": "Password must be at least 6 characters.",
  "auth/invalid-credential": "Incorrect email or password.",
  "auth/user-not-found": "Incorrect email or password.",
  "auth/wrong-password": "Incorrect email or password.",
  "auth/too-many-requests": "Too many attempts — please wait a moment and try again.",
};

function friendlyError(err) {
  return ERROR_MESSAGES[err?.code] ?? err?.message ?? String(err);
}

function AuthScreen() {
  const { signup, login } = useAuth();
  const [mode, setMode] = useState("login"); // "login" | "signup"
  const [displayName, setDisplayName] = useState("");
  const [photoURL, setPhotoURL] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [error, setError] = useState("");
  const [submitting, setSubmitting] = useState(false);

  function switchMode(next) {
    setMode(next);
    setError("");
  }

  async function handleSubmit(e) {
    e.preventDefault();
    setError("");

    if (mode === "signup" && password !== confirmPassword) {
      setError("Passwords don't match.");
      return;
    }

    setSubmitting(true);
    try {
      if (mode === "signup") {
        await signup(displayName.trim(), email.trim(), password, photoURL.trim());
      } else {
        await login(email.trim(), password);
      }
    } catch (err) {
      setError(friendlyError(err));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="container auth-screen">
      <h1>KraftReel.App</h1>
      <p className="subtitle">{mode === "login" ? "Log in to continue" : "Create an account"}</p>

      <form className="card" onSubmit={handleSubmit}>
        {mode === "signup" && (
          <label className="field-row">
            Display name
            <input type="text" value={displayName} onChange={(e) => setDisplayName(e.target.value)} required />
          </label>
        )}

        <label className="field-row">
          Email
          <input type="email" value={email} onChange={(e) => setEmail(e.target.value)} required />
        </label>

        <label className="field-row">
          Password
          <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} required minLength={6} />
        </label>

        {mode === "signup" && (
          <>
            <label className="field-row">
              Confirm password
              <input
                type="password"
                value={confirmPassword}
                onChange={(e) => setConfirmPassword(e.target.value)}
                required
                minLength={6}
              />
            </label>
            <label className="field-row">
              Profile photo URL (optional)
              <input type="url" value={photoURL} onChange={(e) => setPhotoURL(e.target.value)} placeholder="https://…" />
            </label>
          </>
        )}

        {error && <pre className="result">{error}</pre>}

        <button type="submit" disabled={submitting}>
          {submitting ? "Please wait…" : mode === "login" ? "Log in" : "Sign up"}
        </button>

        <p className="subtitle" style={{ marginTop: 12, marginBottom: 0, fontSize: 13 }}>
          {mode === "login" ? (
            <>
              Don't have an account?{" "}
              <button type="button" className="link-button" onClick={() => switchMode("signup")}>
                Sign up
              </button>
            </>
          ) : (
            <>
              Already have an account?{" "}
              <button type="button" className="link-button" onClick={() => switchMode("login")}>
                Log in
              </button>
            </>
          )}
        </p>
      </form>
    </div>
  );
}

export default AuthScreen;
