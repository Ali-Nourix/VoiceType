# VoiceType

A small, Windows-first voice typing application written in Rust. Speak into the microphone **or transcribe audio playing through your default Windows output device**, and type the result into the focused application.

> Experimental: this project uses the unofficial Chromium speech endpoint observed in the supplied binary, not an official Gboard SDK. The bundled public client key and endpoint may stop working, be rate-limited, or be restricted. Verify service terms before distribution. No claim is made that this is Gboard's exact model.

## Features

- Global configurable shortcut (Ctrl/Alt/Shift plus A–Z; default **Alt+Z**), toggle to start/stop.
- Microphone or Windows WASAPI system-output loopback, using the Windows default device.
- Minimal dark interface, floating listening indicator, system tray, and optional launch on Windows sign-in.
- Persian (`fa-IR`) and English (`en-US`); optional Persian Yeh/Kaf normalization.
- Inserts Unicode into the active app or lets you copy the last transcript.
- Explicit 30-second capture limit. Captured audio is sent after recording ends; this is not true live streaming.

## Use

1. Run `voicetype.exe`. Internet access is required.
2. Open **Settings**, select the language and **Microphone** or **System audio**, and save. System audio captures sounds from the default playback device, including other applications; obtain permission from participants when required.
3. Focus the target text field, press **Alt+Z**, speak or play audio, then press the shortcut again. After 30 seconds capture stops automatically.
4. Text is typed only if the same top-level window remains in the foreground. The application cannot tell whether the focused field within that same window changed. If the target is unavailable, open the app and use **Copy**. Starting with the on-screen button does not select an external target; use the shortcut for direct typing.
5. Close the window to hide it in the tray; click the tray icon to reopen it. Use the tray menu's **Quit** to exit. When tray creation fails, closing the window exits instead.
6. Enable **Launch at Windows startup** in Settings if wanted. Keep the executable at the same path after enabling this option.

Audio recording begins only when the user activates it. Audio from the selected source is sent to Google on stop; do not use this for confidential audio without understanding the service's data handling. Recordings and transcripts are not deliberately written to disk by this application; the latest transcript remains in memory while it is open. The shortcut and preferences are stored as JSON under `%APPDATA%\VoiceType\settings.json`, and startup uses the current user's Windows Run registry key. The key in the source is a bundled client key, not a secret; it is not a reliable private credential.

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
- Currently only default input/output devices can be selected. Capture, tray, shortcut, floating indicator, text injection, and startup must be verified on a real Windows installation; CI compilation alone does not validate them.
- If a target application runs elevated, Windows may block text injection from an unelevated VoiceType process. Use Copy instead.
- Loopback behavior depends on the default playback device and Windows audio settings. DRM-protected playback may not be capturable.
- The included resampler is intentionally simple; audio quality and Persian accuracy need further benchmarking. Visual shaping uses Windows Tahoma for Arabic-script preview; copied/typed text preserves logical Unicode order.
- Closing the tray menu via Quit discards in-progress capture. There is no transcript history or file upload.

## License

GPL-3.0-only. See [LICENSE](LICENSE). This software is provided without warranty.
