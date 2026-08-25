// Auth session state, backed by Firebase Auth. Only Firebase's own native
// profile fields are used (displayName, photoURL, email) -- no Firestore or
// other companion store, so this stays a thin wrapper around the SDK
// rather than a second source of truth to keep in sync.

import { createContext, useContext, useEffect, useState } from "react";
import {
  createUserWithEmailAndPassword,
  onAuthStateChanged,
  signInWithEmailAndPassword,
  signOut,
  updateProfile,
} from "firebase/auth";
import { auth } from "../firebase.js";

const AuthContext = createContext(null);

export function AuthProvider({ children }) {
  const [user, setUser] = useState(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    const unsubscribe = onAuthStateChanged(auth, (u) => {
      setUser(u);
      setLoading(false);
    });
    return unsubscribe;
  }, []);

  async function signup(displayName, email, password, photoURL) {
    const credential = await createUserWithEmailAndPassword(auth, email, password);
    const profile = { displayName };
    if (photoURL) profile.photoURL = photoURL;
    await updateProfile(credential.user, profile);
    // updateProfile doesn't itself trigger onAuthStateChanged, so the
    // locally-held user needs its own refresh to pick up the new name/photo
    // immediately rather than waiting for the next auth event.
    setUser({ ...credential.user, ...profile });
  }

  function login(email, password) {
    return signInWithEmailAndPassword(auth, email, password);
  }

  function logout() {
    return signOut(auth);
  }

  return (
    <AuthContext.Provider value={{ user, loading, signup, login, logout }}>{children}</AuthContext.Provider>
  );
}

export function useAuth() {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used within an AuthProvider");
  return ctx;
}
