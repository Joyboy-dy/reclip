//! Parsing of yt-dlp progress output (see the `--progress-template` in `args`).

/// One step of a running download, as reported by yt-dlp.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProgressEvent {
    /// Bytes are arriving. `speed` is in bytes per second, `eta` in seconds.
    Download {
        percent: Option<f32>,
        speed: Option<u64>,
        eta: Option<u64>,
    },
    /// Download finished; a post-processor (merge, audio extraction…) runs.
    Processing,
}

/// Prefix of the lines produced by our `--progress-template`.
const PREFIX: &str = "[reclip] ";
/// Post-processors that run once the download itself is over.
const POSTPROCESSORS: [&str; 4] = [
    "[Merger]",
    "[ExtractAudio]",
    "[VideoConvertor]",
    "[FixupM3u8]",
];

/// Parse one stdout line. Unrelated lines give `None`.
///
/// Progress lines are `[reclip] downloaded total estimate speed eta`, with
/// `NA` for unknown values. The percentage uses `total`, else `estimate`.
pub fn parse_line(line: &str) -> Option<ProgressEvent> {
    let line = line.trim();
    if POSTPROCESSORS.iter().any(|p| line.starts_with(p)) {
        return Some(ProgressEvent::Processing);
    }
    let fields: Vec<&str> = line.strip_prefix(PREFIX)?.split_whitespace().collect();
    let [downloaded, total, estimate, speed, eta] = fields[..] else {
        return None;
    };
    let percent = number(downloaded).and_then(|done| {
        let size = number(total)
            .filter(|t| *t > 0.0)
            .or_else(|| number(estimate).filter(|t| *t > 0.0))?;
        Some((done / size * 100.0).min(100.0) as f32)
    });
    Some(ProgressEvent::Download {
        percent,
        speed: number(speed).map(|s| s as u64),
        eta: number(eta).map(|e| e as u64),
    })
}

/// A finite, non-negative number; `NA` and anything else give `None`.
fn number(s: &str) -> Option<f64> {
    s.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn download(line: &str) -> (Option<f32>, Option<u64>, Option<u64>) {
        match parse_line(line) {
            Some(ProgressEvent::Download {
                percent,
                speed,
                eta,
            }) => (percent, speed, eta),
            other => panic!("expected a download event, got {other:?}"),
        }
    }

    #[test]
    fn parses_known_total() {
        let (p, s, e) = download("[reclip] 250 1000 NA 1500.5 3");
        assert_eq!(p, Some(25.0));
        assert_eq!(s, Some(1500));
        assert_eq!(e, Some(3));
    }
    #[test]
    fn uses_estimate_when_total_missing() {
        let (p, _, _) = download("[reclip] 500 NA 2000.0 NA NA");
        assert_eq!(p, Some(25.0));
    }
    #[test]
    fn na_values_are_none() {
        assert_eq!(download("[reclip] NA NA NA NA NA"), (None, None, None));
        assert_eq!(download("[reclip] 10 NA NA NA NA").0, None);
    }
    #[test]
    fn clamps_to_100() {
        let (p, _, _) = download("[reclip] 1200 NA 1000.0 10 0");
        assert_eq!(p, Some(100.0));
    }
    #[test]
    fn postprocessor_lines_mean_processing() {
        for l in [
            "[Merger] Merging formats into \"media.mp4\"",
            "[ExtractAudio] Destination: media.mp3",
            "[VideoConvertor] Converting video",
            "[FixupM3u8] Fixing MPEG-TS in MP4 container",
        ] {
            assert_eq!(parse_line(l), Some(ProgressEvent::Processing), "{l}");
        }
    }
    #[test]
    fn other_lines_are_ignored() {
        for l in [
            "",
            "[youtube] abc: Downloading webpage",
            "[download] Destination: media.f137.mp4",
            "[reclip] garbage",
            "WARNING: something",
        ] {
            assert_eq!(parse_line(l), None, "{l}");
        }
    }
}
