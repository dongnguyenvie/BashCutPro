//! BashCutPro's read-only measurements (`bashcut/in-app-ui.md` §2, issue #36): pure functions over
//! planar `f32` samples and RGBA8 frames, used by the `bashcut.*` engine commands.
//!
//! - [`loudness`]: integrated / short-term loudness, true peak and clipping (BS.1770 / EBU R128 through
//!   `filmcraft-audio-dsp`'s meter).
//! - [`beats`]: tempo, beats and downbeats from an onset envelope.
//! - [`picture`]: black, frozen and long-still stretches and flashes from small luma thumbnails.
//!
//! Times are sample counts (audio) or the caller's own integer unit (pictures: the engine passes ticks).
//! Nothing here panics on hostile input: empty, short, silent or NaN signals give empty results.

pub mod beats;
pub mod loudness;
pub mod picture;
