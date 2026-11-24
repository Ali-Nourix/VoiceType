use serde_json::Value;
pub fn to_pcm16_mono_16k(samples: &[f32], channels: usize, rate: u32) -> Vec<u8> {
    if channels == 0 || rate == 0 {
        return Vec::new();
    }
    let mono: Vec<f32> = samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();
    let length = mono.len().saturating_mul(16_000) / rate as usize;
    let mut out = Vec::with_capacity(length * 2);
    for i in 0..length {
        let p = i as f64 * rate as f64 / 16_000.0;
        let a = p as usize;
        let b = (a + 1).min(mono.len().saturating_sub(1));
        let f = (p - a as f64) as f32;
        let sample = (mono[a] * (1.0 - f) + mono[b] * f).clamp(-1.0, 1.0);
        out.extend_from_slice(&((sample * 32767.0) as i16).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod audio_tests {
    use super::*;
    #[test]
    fn mono_and_resample() {
        let x = vec![0.5f32; 48_000 * 2];
        let y = to_pcm16_mono_16k(&x, 2, 48_000);
        assert_eq!(y.len(), 32_000);
        assert_eq!(i16::from_le_bytes([y[0], y[1]]), 16383);
    }
    #[test]
    fn handles_empty() {
        assert!(to_pcm16_mono_16k(&[], 1, 16000).is_empty());
    }
}

pub fn parse_response(body: &str) -> Option<String> {
    let mut segments = Vec::new();
    for line in body.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(results) = value.get("result").and_then(Value::as_array) {
            for result in results {
                if let Some(text) = result
                    .get("alternative")
                    .and_then(Value::as_array)
                    .and_then(|a| a.first())
                    .and_then(|a| a.get("transcript"))
                    .and_then(Value::as_str)
                {
                    if !text.trim().is_empty() {
                        segments.push(text.trim().to_owned());
                    }
                }
            }
        }
    }
    if segments.is_empty() {
        None
    } else {
        Some(segments.join(" "))
    }
}
#[cfg(test)]
mod speech_tests {
    use super::*;
    #[test]
    fn parses_newline_json() {
        assert_eq!(
            parse_response(
                "{\"result\":[]}\n{\"result\":[{\"alternative\":[{\"transcript\":\"hello\"}]}]}"
            ),
            Some("hello".into())
        );
    }
    #[test]
    fn empty_response() {
        assert_eq!(parse_response("{\"result\":[]}"), None);
    }
}

/// Normalize Arabic variants commonly returned in Persian dictation.
pub fn normalize_persian(text: &str) -> String {
    text.replace('ي', "ی").replace('ى', "ی").replace('ك', "ک")
}

#[cfg(test)]
mod normalization_tests {
    #[test]
    fn converts_arabic_letters() {
        assert_eq!(super::normalize_persian("كيك"), "کیک");
    }
}
