// VizRS CG Playout sidecar: VizRS graphics for ErsatzTV channels.
// Successor of the Node.js sidecar with the same contract (DOKUMENTACJA.md);
// the migration: docs/plany/PLAN-rust.md.

pub mod log;

pub mod area;
pub mod base64;
pub mod capture;
pub mod config;
pub mod devtools;
pub mod etv_client;
pub mod etv_sync;
pub mod frames;
pub mod http;
pub mod ingest;
pub mod json;
pub mod listener;
pub mod live_shows;
pub mod native;
pub mod net;
pub mod overlay;
pub mod packets;
pub mod png;
pub mod rects;
pub mod scale;
pub mod server;
pub mod stream;
pub mod sync;
pub mod sys;
pub mod takeover;
pub mod tape;
pub mod template;
pub mod time;
pub mod vizrs;
pub mod yuva;
