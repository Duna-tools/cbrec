use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VideoQuality {
    AudioOnly,
    P240,
    P360,
    P480,
    P720,
    P1080,
    Best,
}

impl FromStr for VideoQuality {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "audio" | "audio_only" => Ok(VideoQuality::AudioOnly),
            "240p" | "240" => Ok(VideoQuality::P240),
            "360p" | "360" => Ok(VideoQuality::P360),
            "480p" | "480" => Ok(VideoQuality::P480),
            "720p" | "720" => Ok(VideoQuality::P720),
            "1080p" | "1080" => Ok(VideoQuality::P1080),
            "best" => Ok(VideoQuality::Best),
            _ => Err(format!("Calidad invalida: {}", s)),
        }
    }
}

impl std::fmt::Display for VideoQuality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VideoQuality::AudioOnly => write!(f, "audio"),
            VideoQuality::P240 => write!(f, "240p"),
            VideoQuality::P360 => write!(f, "360p"),
            VideoQuality::P480 => write!(f, "480p"),
            VideoQuality::P720 => write!(f, "720p"),
            VideoQuality::P1080 => write!(f, "1080p"),
            VideoQuality::Best => write!(f, "best"),
        }
    }
}

impl VideoQuality {
    pub fn target_height(&self) -> Option<u32> {
        match self {
            VideoQuality::AudioOnly => Some(0),
            VideoQuality::P240 => Some(240),
            VideoQuality::P360 => Some(360),
            VideoQuality::P480 => Some(480),
            VideoQuality::P720 => Some(720),
            VideoQuality::P1080 => Some(1080),
            VideoQuality::Best => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_aliases_case_insensitively() {
        let cases = [
            ("AUDIO_ONLY", VideoQuality::AudioOnly),
            ("240", VideoQuality::P240),
            ("360P", VideoQuality::P360),
            ("480", VideoQuality::P480),
            ("720p", VideoQuality::P720),
            ("1080", VideoQuality::P1080),
            ("BEST", VideoQuality::Best),
        ];

        for (input, expected) in cases {
            assert_eq!(input.parse(), Ok(expected));
        }
        assert_eq!(
            "4k".parse::<VideoQuality>(),
            Err("Calidad invalida: 4k".into())
        );
    }

    #[test]
    fn exposes_display_value_and_target_height() {
        let cases = [
            (VideoQuality::AudioOnly, "audio", Some(0)),
            (VideoQuality::P240, "240p", Some(240)),
            (VideoQuality::P360, "360p", Some(360)),
            (VideoQuality::P480, "480p", Some(480)),
            (VideoQuality::P720, "720p", Some(720)),
            (VideoQuality::P1080, "1080p", Some(1080)),
            (VideoQuality::Best, "best", None),
        ];

        for (quality, display, height) in cases {
            assert_eq!(quality.to_string(), display);
            assert_eq!(quality.target_height(), height);
        }
    }
}
