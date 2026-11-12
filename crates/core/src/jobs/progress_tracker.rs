//! Turns raw yt-dlp progress events into one monotonic percentage.
//!
//! yt-dlp reports each stream (video, then audio for a merged mp4) from 0 to
//! 100 %. The tracker maps them onto a single bar: a drop of more than
//! [`SWITCH_DROP`] points after the current stream passed [`SWITCH_AFTER`] %
//! starts the next stream; overall = (finished × 100 + current) / expected.
//! The bar never goes back and stays below 100 until post-processing starts.

use reclip_shared::MediaFormat;

use crate::ytdlp::args::format_selector;
use crate::ytdlp::progress::ProgressEvent;

const SWITCH_AFTER: f32 = 90.0;
const SWITCH_DROP: f32 = 50.0;
/// Highest value shown while bytes are still arriving.
const DOWNLOAD_CAP: f32 = 99.9;

/// What the worker shows for a job at a given moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Live {
    pub progress: f32,
    pub speed: Option<u64>,
    pub eta: Option<u64>,
}

pub struct ProgressTracker {
    expected: u32,
    finished: u32,
    current: f32,
    shown: f32,
    processing: bool,
}

impl ProgressTracker {
    pub fn new(expected_streams: u32) -> ProgressTracker {
        ProgressTracker {
            expected: expected_streams.max(1),
            finished: 0,
            current: 0.0,
            shown: 0.0,
            processing: false,
        }
    }

    /// Two streams when the format selector merges (`a+b`), else one.
    pub fn for_download(format: MediaFormat, format_id: Option<&str>) -> ProgressTracker {
        let merged = format_selector(format, format_id).is_some_and(|s| s.contains('+'));
        ProgressTracker::new(if merged { 2 } else { 1 })
    }

    pub fn expected_streams(&self) -> u32 {
        self.expected
    }

    /// A post-processor (merge, audio extraction…) has started.
    pub fn is_processing(&self) -> bool {
        self.processing
    }

    pub fn update(&mut self, event: ProgressEvent) -> Live {
        match event {
            ProgressEvent::Processing => {
                self.processing = true;
                self.shown = 100.0;
                Live {
                    progress: self.shown,
                    speed: None,
                    eta: None,
                }
            }
            ProgressEvent::Download {
                percent,
                speed,
                eta,
            } => {
                if let Some(p) = percent.filter(|p| p.is_finite()) {
                    self.advance(p.clamp(0.0, 100.0));
                }
                Live {
                    progress: self.shown,
                    speed,
                    eta,
                }
            }
        }
    }

    fn advance(&mut self, percent: f32) {
        let next_stream = self.current > SWITCH_AFTER && self.current - percent > SWITCH_DROP;
        if next_stream && self.finished + 1 < self.expected {
            self.finished += 1;
        }
        self.current = percent;
        let overall = (self.finished as f32 * 100.0 + percent) / self.expected as f32;
        let cap = if self.processing { 100.0 } else { DOWNLOAD_CAP };
        self.shown = self.shown.max(overall.min(cap));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dl(percent: f32) -> ProgressEvent {
        ProgressEvent::Download {
            percent: Some(percent),
            speed: Some(7),
            eta: Some(3),
        }
    }

    fn feed(t: &mut ProgressTracker, percents: &[f32]) -> Vec<f32> {
        percents.iter().map(|p| t.update(dl(*p)).progress).collect()
    }

    #[test]
    fn expected_streams_follow_the_format_selector() {
        let n = |f, id| ProgressTracker::for_download(f, id).expected_streams();
        assert_eq!(n(MediaFormat::Mp4, None), 2);
        assert_eq!(n(MediaFormat::Mp4, Some("137")), 2);
        assert_eq!(n(MediaFormat::Mp3, None), 1);
    }

    #[test]
    fn single_stream_caps_until_processing() {
        let mut t = ProgressTracker::new(1);
        assert_eq!(feed(&mut t, &[0.0, 50.0, 100.0]), [0.0, 50.0, 99.9]);
        assert!(!t.is_processing());
        let live = t.update(ProgressEvent::Processing);
        assert!(t.is_processing());
        assert_eq!(live.progress, 100.0);
        assert_eq!((live.speed, live.eta), (None, None));
    }

    #[test]
    fn two_streams_span_the_bar() {
        let mut t = ProgressTracker::new(2);
        assert_eq!(
            feed(&mut t, &[0.0, 50.0, 100.0, 0.0, 50.0, 100.0]),
            [0.0, 25.0, 50.0, 50.0, 75.0, 99.9]
        );
    }

    #[test]
    fn never_decreases() {
        let mut t = ProgressTracker::new(2);
        // A drop below the stream-switch threshold is ignored.
        assert_eq!(feed(&mut t, &[80.0, 20.0, 85.0]), [40.0, 40.0, 42.5]);
        // A small drop after 90 % is not a new stream either.
        assert_eq!(feed(&mut t, &[95.0, 60.0]), [47.5, 47.5]);
        // More streams than expected never move the bar back.
        let mut t = ProgressTracker::new(1);
        assert_eq!(feed(&mut t, &[95.0, 10.0, 50.0]), [95.0, 95.0, 95.0]);
    }

    #[test]
    fn speed_and_eta_pass_through_and_unknown_percent_keeps_value() {
        let mut t = ProgressTracker::new(1);
        t.update(dl(40.0));
        let live = t.update(ProgressEvent::Download {
            percent: None,
            speed: Some(1000),
            eta: None,
        });
        assert_eq!(
            live,
            Live {
                progress: 40.0,
                speed: Some(1000),
                eta: None
            }
        );
    }
}
