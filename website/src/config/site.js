/**
 * Central config for the KraftReel.ai marketing site.
 *
 * Download URLs point at GitHub Releases by default. When a signed installer is
 * published by the release workflow, update `version` and the per-asset `file`
 * names below (or swap `url` for a fully custom link, e.g. a CDN / R2 bucket).
 */

const GH_REPO = 'antorobin/reels-caption-app';
const LATEST = `https://github.com/${GH_REPO}/releases/latest/download`;

export const site = {
  name: 'KraftReel.ai',
  domain: 'kraftreel.ai',
  tagline: 'Turn Raw Video into Crafted Reels.',
  description:
    'One intelligent workspace that transforms raw footage into polished, engaging, ready-to-publish short-form content — captions, voiceovers, music, smart copy and scheduling in a single flow.',
  closing: 'Create Less. Craft More. Publish Smarter.',
  releasesUrl: `https://github.com/${GH_REPO}/releases`,
  studioUrl: '#download',
  version: '0.1.0',
};

/** @typedef {{ id: string, label: string, status: 'available'|'soon', note: string, arch: string, file?: string, url?: string }} Download */

/** @type {Download[]} */
export const downloads = [
  {
    id: 'windows',
    label: 'Windows',
    status: 'available',
    note: 'Windows 10 & 11 · 64-bit',
    arch: '.msi installer',
    url: `${LATEST}/KraftReel_${site.version}_x64-setup.exe`,
  },
  {
    id: 'mac',
    label: 'macOS',
    status: 'soon',
    note: 'Apple Silicon & Intel',
    arch: 'Universal .dmg',
  },
  {
    id: 'linux',
    label: 'Linux',
    status: 'soon',
    note: 'AppImage & .deb',
    arch: 'x86_64',
  },
];

export const features = [
  {
    key: 'CC',
    accent: 'teal',
    title: 'Auto Captions',
    body: 'Turn spoken words into accurate, editable captions automatically. Fine-tune timing, text and styles to create captions that don’t just inform — they command attention.',
  },
  {
    key: 'VO',
    accent: 'violet',
    title: 'AI Voice Overs',
    body: 'Bring content to life with flexible voice creation. Upload audio, record from your mic, or turn written text into natural-sounding narration.',
  },
  {
    key: 'AI',
    accent: 'pink',
    title: 'Smart Copy & Hashtags',
    body: 'Let AI understand your content and generate concise summaries, social-ready descriptions and relevant hashtags in seconds.',
  },
  {
    key: '♪',
    accent: 'amber',
    title: 'Background Music',
    body: 'Add the right soundtrack and atmosphere to elevate every story, matched to your content, mood and creative style.',
  },
  {
    key: 'IG',
    accent: 'teal',
    title: 'Post Scheduling',
    body: 'Plan ahead and stay consistent. Prepare finished content and schedule Instagram posts straight from your creative workflow.',
  },
];

export const workflow = [
  'Upload',
  'Caption',
  'Enhance',
  'Voice',
  'Style',
  'Schedule',
  'Publish',
];

export const audiences = [
  'Content creators',
  'Marketers',
  'Educators',
  'Influencers',
  'Businesses',
  'Social media teams',
];
