use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::{Receiver, Sender},
        Arc, Mutex,
    },
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    Microphone,
    System,
}

pub enum Event {
    Started,
    Processing,
    Finished(String),
    Failed(String),
    Cancelled,
}

// A bounded capture prevents runaway memory use, even if the GUI loses the stop signal.
pub fn run(
    source: Source,
    lang: String,
    key: String,
    stop: Receiver<()>,
    cancelled: Arc<AtomicBool>,
    events: Sender<Event>,
    level: Arc<AtomicU32>,
    wake: eframe::egui::Context,
) {
    if let Err(e) = record(source, &lang, &key, stop, &events, level, &wake, &cancelled) {
        let _ = events.send(if cancelled.load(Ordering::Acquire) {
            Event::Cancelled
        } else {
            Event::Failed(e)
        });
        wake.request_repaint();
    }
}
fn record(
    source: Source,
    lang: &str,
    key: &str,
    stop: Receiver<()>,
    events: &Sender<Event>,
    level: Arc<AtomicU32>,
    wake: &eframe::egui::Context,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let host = cpal::default_host();
    let device = match source {
        Source::Microphone => host.default_input_device(),
        Source::System => host.default_output_device(),
    }
    .ok_or_else(|| "No default audio device found. Check Windows sound settings.".to_string())?;
    // CPAL's WASAPI backend enables loopback when an output device is used as an input.
    let supported = match source {
        Source::Microphone => device.default_input_config(),
        Source::System => device.default_output_config(),
    }
    .map_err(|e| format!("Audio format unavailable: {e}"))?;
    let rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;
    let config: cpal::StreamConfig = supported.clone().into();
    let buffer = Arc::new(Mutex::new(Vec::<f32>::new()));
    let max = rate as usize * channels * 30;
    let (error_tx, error_rx) = std::sync::mpsc::channel::<String>();
    macro_rules! build {
        ($ty:ty, $convert:expr) => {{
            let out = Arc::clone(&buffer);
            let meter = Arc::clone(&level);
            let err = error_tx.clone();
            device.build_input_stream(
                &config,
                move |data: &[$ty], _| {
                    if let Ok(mut samples) = out.lock() {
                        let remaining = max.saturating_sub(samples.len());
                        let mut peak = 0.0f32;
                        for &sample in data.iter().take(remaining) {
                            let v: f32 = ($convert)(sample);
                            peak = peak.max(v.abs());
                            samples.push(v);
                        }
                        meter.store((peak.min(1.0) * 1000.0) as u32, Ordering::Relaxed);
                    }
                },
                move |e| {
                    let _ = err.send(format!("Audio capture failed: {e}"));
                },
                None,
            )
        }};
    }
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build!(f32, |x: f32| x),
        cpal::SampleFormat::I16 => build!(i16, |x: i16| x as f32 / 32768.0),
        cpal::SampleFormat::U16 => build!(u16, |x: u16| (x as f32 - 32768.0) / 32768.0),
        other => return Err(format!("Unsupported audio format: {other:?}")),
    }
    .map_err(|e| format!("Couldn't open audio capture: {e}"))?;
    stream
        .play()
        .map_err(|e| format!("Couldn't start audio capture: {e}"))?;
    let _ = events.send(Event::Started);
    wake.request_repaint();
    let started = std::time::Instant::now();
    loop {
        if let Ok(err) = error_rx.try_recv() {
            return Err(err);
        }
        if stop.recv_timeout(Duration::from_millis(100)).is_ok()
            || started.elapsed() >= Duration::from_secs(30)
        {
            break;
        }
    }
    drop(stream);
    level.store(0, Ordering::Relaxed);
    if cancelled.load(Ordering::Acquire) {
        let _ = events.send(Event::Cancelled);
        wake.request_repaint();
        return Ok(());
    }
    let _ = events.send(Event::Processing);
    wake.request_repaint();
    let samples = buffer
        .lock()
        .map_err(|_| "Audio buffer unavailable".to_string())?;
    let pcm = voicetype::core::to_pcm16_mono_16k(&samples, channels, rate);
    if pcm.len() < 3200 {
        return Err("Recording was too short or silent. Try again.".into());
    }
    if cancelled.load(Ordering::Acquire) {
        let _ = events.send(Event::Cancelled);
        wake.request_repaint();
        return Ok(());
    }
    let text = crate::speech::transcribe(pcm, lang, key)?;
    let _ = events.send(if cancelled.load(Ordering::Acquire) {
        Event::Cancelled
    } else {
        Event::Finished(text)
    });
    wake.request_repaint();
    Ok(())
}
