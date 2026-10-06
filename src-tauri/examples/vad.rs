// cargo run --release --example vad -- a.wav b.wav   (16-bit mono 16 kHz WAVs)
fn main() {
    for p in std::env::args().skip(1) {
        let wav = std::fs::read(&p).unwrap();
        let d = wav.windows(4).position(|w| w == b"data").unwrap() + 8;
        let pcm: Vec<f32> = wav[d..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
        println!("{p}: {} ms voiced", murmur_lib::audio::voiced_ms(&pcm));
    }
}
