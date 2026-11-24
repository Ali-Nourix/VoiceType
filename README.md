# VoiceType

A small, Windows-first voice typing application written in Rust. Speak into the microphone **or transcribe audio playing through your default Windows output device**, and type the result into the focused application.

> Experimental: this project uses the unofficial Chromium speech endpoint observed in the supplied binary, not an official Gboard SDK. The bundled public client key and endpoint may stop working, be rate-limited, or be restricted. Verify service terms before distribution. No claim is made that this is Gboard's exact model.

## Features

- Global configurable shortcut (Ctrl/Alt/Shift plus A–Z; default **Alt+Z**), choose toggle or hold-to-talk.
- Microphone or Windows WASAPI system-output loopback, using the Windows default device.
- Minimal dark hub, a compact floating bar near the bottom of the cursor’s monitor, smoothed audio-level history, vector stop/cancel controls, system tray, and optional launch on Windows sign-in. A no-audio preview button lets you inspect the bar.
- Persian (`fa-IR`) and English (`en-US`); optional Persian Yeh/Kaf normalization and bundled Vazirmatn subset for transcript display (SIL OFL 1.1; see `assets/Vazirmatn-OFL.txt`).
- Inserts Unicode into the active app or lets you copy recent session transcripts.
- Explicit 30-second capture limit. Captured audio is sent after recording ends; this is not true live streaming.

## Use

1. Run `voicetype.exe`. Internet access is required.
2. Open **Settings**, select the language and **Microphone** or **System audio**, and save. System audio captures sounds from the default playback device, including other applications; obtain permission from participants when required.
3. Close any older VoiceType instances first. Focus the target text field, press **Alt+Z**, speak or play audio, then press the shortcut again (or enable hold-to-talk in Settings). After 30 seconds capture stops automatically. The floating bar has stop and cancel buttons; Escape cancels while recording if the Escape shortcut could be registered.
4. Text is typed only if the same top-level window remains in the foreground. The application cannot tell whether the focused field within that same window changed. If the target is unavailable, open the app and use **Copy**. Starting with the on-screen button does not select an external target; use the shortcut for direct typing.
5. Close the window to hide it in the tray; click the tray icon to reopen it. Right-click the tray icon and choose **Quit** to exit. When tray creation fails, closing the window exits instead.
6. Enable **Launch at Windows startup** in Settings if wanted. Keep the executable at the same path after enabling this option.

Audio recording begins only when the user activates it. Audio from the selected source is sent to Google on stop; do not use this for confidential audio without understanding the service's data handling. Recordings and transcripts are not deliberately written to disk by this application; up to 20 recent transcripts remain in memory for this session only. Cancelling during processing suppresses the result and insertion, but cannot recall audio already sent to Google. The shortcut and preferences are stored as JSON under `%APPDATA%\VoiceType\settings.json`, and startup uses the current user's Windows Run registry key. The key in the source is a bundled client key, not a secret; it is not a reliable private credential. The app does not need administrator privileges.

## Build

On Windows, install stable Rust and the Visual Studio C++ build tools, then:

```powershell
./scripts/restore-lock.ps1
cargo test --locked
cargo build --release --locked
.\target\release\voicetype.exe
```

The repository stores the exact Cargo lock as `Cargo.lock.gz.b64` to keep the lock as a small text file; the script restores the standard `Cargo.lock`. A GitHub Actions workflow builds the Windows x64 executable and uploads a CI artifact. You can also run `cargo test --lib` on Linux to test the platform-independent audio conversion and response parsing.

## Known limitations

- The endpoint/key is unofficial and may fail despite a successful build; there is no alternate model/provider.
- Audio is buffered (up to 30 seconds) and submitted after stopping, not recognized continuously.
- Currently only default input/output devices can be selected. Capture, tray, shortcut, floating bar placement/focus/click controls (especially on mixed-DPI monitors), hold-to-talk and Escape, text injection, and startup must be verified on a real Windows installation; CI compilation alone does not validate them.
- If a target application runs elevated, Windows may block text injection from an unelevated VoiceType process. Use Copy instead.
- Loopback behavior depends on the default playback device and Windows audio settings. DRM-protected playback may not be capturable.
- The included resampler is intentionally simple; audio quality and Persian accuracy need further benchmarking. Persian text uses the bundled Vazirmatn subset; copied/typed text preserves logical Unicode order.
- When invoked from the tray, the main window may briefly become visible without activation while the floating bar runs. It hides again after successful completion or cancellation; this behavior is experimental and needs native Windows validation.
- Closing the tray menu via Quit discards in-progress capture. Session history is temporary; there is no persistent transcript history or file upload.

## License

GPL-3.0-only. See [LICENSE](LICENSE). This software is provided without warranty.

## Design references

The floating-bar behavior was informed by Wispr Flow documentation (bar placement, recording and processing states, and hold-to-talk); VoiceType is independently implemented and does not include Wispr assets. The `design-taste-frontend` skill was installed locally to inform visual design; its installer reported a critical risk. Its scripts and hooks were not executed or committed. The lockfile records its source for transparency.

## Troubleshooting and trust

- If the shortcut is unavailable, quit any older instance or conflicting shortcut utility (for example, an overlay) and choose a different modifier-plus-letter in Settings. Windows does not allow two apps to register the same shortcut.
- If the tray icon is hidden, check the Windows notification-area overflow. Left-click opens VoiceType; right-click exposes the Quit menu.
- For a visual-only test, choose **Preview floating bar**; it does not capture or transmit audio.
- The executable is **not Authenticode-signed**. Windows SmartScreen or antivirus may warn on new unsigned builds. A verified publisher certificate and a properly signed release are required for publisher identity; a checksum verifies file integrity, not trust or reputation. Do not disable Windows security to run it.
- If startup fails, VoiceType displays the underlying window-startup error in a dialog. Share a screenshot of the exact error when reporting it.
- For development only, `scripts/windows-ui-smoke.ps1` uses the explicit `VOICETYPE_UI_TEST_FAKE` and `VOICETYPE_UI_TEST_LOG` environment variables to simulate audio and record only static state/event messages; it does not capture audio or contact the speech endpoint. This is not a microphone, network, or accuracy test.
