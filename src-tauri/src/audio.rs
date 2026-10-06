//! Mic capture -> mono f32 at the device rate, resampled to 16 kHz on stop.
use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

pub struct Recording {
    _stream: cpal::Stream,
    buf: Arc<Mutex<Vec<f32>>>,
    rate: u32,
}

/// `on_level` gets an RMS value (0..~1) roughly every 30 ms.
pub fn start(on_level: impl Fn(f32) + Send + 'static) -> Result<Recording> {
    let device = cpal::default_host().default_input_device().ok_or(anyhow!("no microphone"))?;
    let config = device.default_input_config()?;
    let rate = config.sample_rate();
    let channels = config.channels() as usize;
    let buf = Arc::new(Mutex::new(Vec::<f32>::new()));
    let sink = buf.clone();
    let (mut acc, mut n, every) = (0f32, 0usize, (rate / 33) as usize);

    let mut push = move |mono: &mut dyn Iterator<Item = f32>| {
        let mut b = sink.lock().unwrap();
        for s in mono {
            b.push(s);
            acc += s * s;
            n += 1;
            if n >= every {
                on_level((acc / n as f32).sqrt());
                (acc, n) = (0.0, 0);
            }
        }
    };
    let err = |e| eprintln!("mic error: {e}");
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            config.into(),
            move |d: &[f32], _: &_| push(&mut d.chunks(channels).map(|f| f.iter().sum::<f32>() / channels as f32)),
            err,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_input_stream(
            config.into(),
            move |d: &[i16], _: &_| {
                push(&mut d.chunks(channels).map(|f| f.iter().map(|&s| s as f32 / 32768.0).sum::<f32>() / channels as f32))
            },
            err,
            None,
        )?,
        f => return Err(anyhow!("unsupported mic format {f}")),
    };
    stream.play()?;
    Ok(Recording { _stream: stream, buf, rate })
}

impl Recording {
    /// Stops the mic and returns 16 kHz mono PCM.
    pub fn finish(self) -> Vec<f32> {
        let Recording { _stream, buf, rate } = self;
        drop(_stream);
        let pcm = std::mem::take(&mut *buf.lock().unwrap());
        resample(&pcm, rate, 16_000)
    }
}

/// Quietest level that can count as voice (~ -52 dBFS), so whispers pass.
/// Calibration knob: lower for very quiet mics.
pub const VOICE_FLOOR: f32 = 0.0025;
/// Voice must also be this many times louder than the clip's background noise.
pub const OVER_NOISE: f32 = 2.5;
/// Loudest background we'll assume (~ -40 dBFS, a noisy room).
const MAX_NOISE: f32 = 0.01;

/// Milliseconds of 16 kHz audio that sound like voice. Loudness is judged
/// against the clip's own noise floor, so whispers count but steady hum
/// doesn't. Clicks and taps are only a few frames, so the caller's minimum
/// duration rejects them; Whistle hallucinates "Thank you." on such clips.
pub fn voiced_ms(pcm: &[f32]) -> usize {
    let rms: Vec<f32> = pcm.chunks(480).map(|f| (f.iter().map(|s| s * s).sum::<f32>() / f.len() as f32).sqrt()).collect();
    let mut sorted = rms.clone();
    sorted.sort_by(f32::total_cmp);
    // quietest 10% of frames = background; capped so nonstop speech isn't mistaken for noise
    let noise = sorted.get(sorted.len() / 10).copied().unwrap_or(0.0).min(MAX_NOISE);
    let threshold = VOICE_FLOOR.max(noise * OVER_NOISE);
    rms.iter().filter(|&&r| r > threshold).count() * 30
}

// ponytail: linear resample, fine for speech; swap in a windowed-sinc if accuracy suffers
pub fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || x.is_empty() {
        return x.to_vec();
    }
    let step = from as f64 / to as f64;
    let n = (x.len() as f64 / step) as usize;
    (0..n)
        .map(|i| {
            let p = i as f64 * step;
            let j = p as usize;
            let t = (p - j as f64) as f32;
            let a = x[j];
            let b = *x.get(j + 1).unwrap_or(&a);
            a + (b - a) * t
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn resample_lengths_and_values() {
        let x: Vec<f32> = (0..48_000).map(|i| i as f32).collect();
        let y = super::resample(&x, 48_000, 16_000);
        assert_eq!(y.len(), 16_000);
        assert_eq!(y[1], 3.0);
        assert_eq!(super::resample(&[1.0, 2.0], 16_000, 16_000), vec![1.0, 2.0]);
    }

    #[test]
    fn clicks_are_not_voice() {
        let click: Vec<f32> = (0..32_000).map(|i| if i % 8000 < 40 { 0.3 } else { 0.001 }).collect();
        assert!(super::voiced_ms(&click) < 250);
        let speech: Vec<f32> = (0..16_000).map(|i| 0.1 * (i as f32 * 0.05).sin()).collect();
        assert!(super::voiced_ms(&speech) >= 900);
    }

    #[test]
    fn whisper_counts_but_hum_does_not() {
        // 1 s quiet room, then 1 s whisper at ~ -45 dBFS
        let whisper: Vec<f32> = (0..32_000)
            .map(|i| if i < 16_000 { 0.0005 * (i as f32).sin() } else { 0.008 * (i as f32 * 0.07).sin() })
            .collect();
        assert!(super::voiced_ms(&whisper) >= 900);
        let hum: Vec<f32> = (0..32_000).map(|i| 0.01 * (i as f32 * 0.0236).sin()).collect();
        assert!(super::voiced_ms(&hum) < 250);
    }
}
