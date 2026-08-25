// Firebase app initialization, shared by everything under src/context/AuthContext.jsx.
//
// Config values come from Vite env vars (VITE_FIREBASE_*, see .env.example)
// rather than being hardcoded here -- not because they're secret (a
// Firebase web config is meant to be public; access control is enforced by
// Firebase's own security rules, not by hiding these values) but so a
// different Firebase project can be swapped in per environment without
// editing source.

import { initializeApp } from "firebase/app";
import { getAuth } from "firebase/auth";

const firebaseConfig = {
  apiKey: import.meta.env.VITE_FIREBASE_API_KEY,
  authDomain: import.meta.env.VITE_FIREBASE_AUTH_DOMAIN,
  projectId: import.meta.env.VITE_FIREBASE_PROJECT_ID,
  storageBucket: import.meta.env.VITE_FIREBASE_STORAGE_BUCKET,
  messagingSenderId: import.meta.env.VITE_FIREBASE_MESSAGING_SENDER_ID,
  appId: import.meta.env.VITE_FIREBASE_APP_ID,
};

export const app = initializeApp(firebaseConfig);
export const auth = getAuth(app);
