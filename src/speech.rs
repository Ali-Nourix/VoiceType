use std::time::Duration;

// Compatibility with the Chromium endpoint observed in the supplied binary.
// This is not a documented or guaranteed Gboard integration.
pub const DEFAULT_KEY: &str = "AIzaSyBOti4mM-6x9WDnZIjIeyEU21OpBXqWBgw";

pub fn transcribe(pcm: Vec<u8>, language: &str, key: &str) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(25))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Couldn't initialize HTTPS client".to_owned())?;
    let response = client
        .post("https://www.google.com/speech-api/v2/recognize")
        .query(&[
            ("client", "chromium"),
            ("lang", language),
            ("key", key),
            ("pFilter", "0"),
        ])
        .header("Content-Type", "audio/l16; rate=16000")
        .body(pcm)
        .send()
        .map_err(|e| format!("Network error: {}", e.without_url()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("Google endpoint returned HTTP {status}. The Chromium endpoint/key may be unavailable or restricted."));
    }
    let body = response
        .text()
        .map_err(|_| "Couldn't read Google's response".to_owned())?;
    voicetype::core::parse_response(&body).ok_or_else(|| {
        "No speech recognized. Check the language, audio source, and endpoint access.".to_owned()
    })
}
