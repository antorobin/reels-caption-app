// Mirrors stt::INDIC_LANGUAGES / tts::TTS_INDIC_LANGUAGES in the Rust
// backend (src-tauri/src/stt.rs, tts.rs) -- update both together when
// wiring up another language. There is no language dropdown anywhere in
// this app: speech-to-text auto-detects from the audio (see
// stt::detect_spoken_language), and text-to-speech auto-detects from the
// script text (see detectTextLanguage below).
export const SUPPORTED_LANGUAGES = ["English", "Tamil"];

// Tamil uses a dedicated Unicode block (U+0B80-U+0BFF), so counting
// characters in that range reliably distinguishes Tamil script from Latin
// script without any NLP -- a script-level distinction, not a same-script
// language call that would need real language ID. Extend this map (and
// TTS_INDIC_LANGUAGES in tts.rs) if a non-Latin-script language is added
// that isn't script-distinguishable this way.
const SCRIPT_RANGES = [{ code: "ta", from: 0x0b80, to: 0x0bff }];

// Detects the language of a voiceover script so it can be routed to the
// matching TTS engine (Piper for English, MMS-TTS for Indian languages)
// with no language picker in the UI. Falls back to English when no
// recognized non-Latin script is present.
export function detectTextLanguage(text) {
  const counts = {};
  for (const ch of text) {
    const point = ch.codePointAt(0);
    for (const range of SCRIPT_RANGES) {
      if (point >= range.from && point <= range.to) {
        counts[range.code] = (counts[range.code] || 0) + 1;
      }
    }
  }
  const [topCode, topCount] = Object.entries(counts).sort((a, b) => b[1] - a[1])[0] ?? [null, 0];
  return topCount > 0 ? topCode : "en";
}
