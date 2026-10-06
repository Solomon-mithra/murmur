//! Cactus Whistle via libneedle.a (see vendor/needle/needle.h).
use anyhow::{anyhow, Result};
use std::ffi::{c_char, c_int, c_ulonglong, CStr};
use std::path::Path;

extern "C" {
    fn needle_load(cact: *const u8, n: c_ulonglong) -> c_int;
    fn needle_last_error() -> *const c_char;
    fn needle_transcribe(
        pcm: *const f32,
        samples: c_int,
        language: *const c_char,
        keywords: *const c_char,
        word_timestamps: c_int,
        out: *mut c_char,
        out_capacity: c_int,
    ) -> c_int;
}

fn last_error() -> String {
    unsafe { CStr::from_ptr(needle_last_error()).to_string_lossy().into_owned() }
}

/// Loads whistle.cact. The runtime is process-global; call once.
pub fn load(path: &Path) -> Result<()> {
    // The engine may keep pointers into the buffer, so it lives for the process.
    let bytes: &'static [u8] = Vec::leak(std::fs::read(path)?);
    if unsafe { needle_load(bytes.as_ptr(), bytes.len() as _) } < 0 {
        return Err(anyhow!("whistle load failed: {}", last_error()));
    }
    Ok(())
}

const MAX: usize = 30 * 16_000;

/// Spoken cues that cleanup.rs acts on. Whistle's keyword biasing makes it favour
/// these exact phrases ("no wait", not "no weight"; "scratch that", not "scratched that").
const CUES: &std::ffi::CStr = c"no wait\nI meant\noh no I meant\nsorry I meant\nactually no\nor rather\nscratch that\nbullet point\nnext bullet\nnew line\nnew paragraph";

/// 16 kHz mono PCM -> text. Not thread-safe: call from one thread only.
pub fn transcribe(pcm: &[f32]) -> Result<String> {
    // ponytail: hard 30 s windows may split a word at the seam; use needle_stream_transcribe_* if long dictation matters
    let mut parts = Vec::new();
    for chunk in pcm.chunks(MAX) {
        if chunk.len() < 16_000 / 4 {
            continue; // <250 ms: nothing worth decoding
        }
        let mut out = vec![0 as c_char; 64 * 1024];
        // MURMUR_NO_KEYWORDS=1 turns biasing off, for A/B testing with examples/e2e.rs
        let cues = if std::env::var_os("MURMUR_NO_KEYWORDS").is_some() { std::ptr::null() } else { CUES.as_ptr() };
        let rc = unsafe {
            needle_transcribe(chunk.as_ptr(), chunk.len() as _, c"en".as_ptr(), cues, 0, out.as_mut_ptr(), out.len() as _)
        };
        if rc < 0 {
            return Err(anyhow!("whistle: {}", last_error()));
        }
        let json = unsafe { CStr::from_ptr(out.as_ptr()) }.to_string_lossy();
        let v: serde_json::Value = serde_json::from_str(&json)?;
        let t = v["text"].as_str().unwrap_or("").trim();
        if !t.is_empty() {
            parts.push(t.to_string());
        }
    }
    Ok(parts.join(" "))
}
