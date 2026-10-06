// Speech → Whistle → cleanup rules, i.e. exactly what dictation types (minus the paste).
// cargo run --release --example e2e -- a.wav b.wav   (16-bit mono 16 kHz WAVs)
fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
    murmur_lib::stt::load(&dir.join("whistle.cact")).unwrap();
    for p in std::env::args().skip(1) {
        let wav = std::fs::read(&p).unwrap();
        let d = wav.windows(4).position(|w| w == b"data").unwrap() + 8;
        let pcm: Vec<f32> = wav[d..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
        let heard = murmur_lib::stt::transcribe(&pcm).unwrap();
        println!("heard: {heard}\ntyped: {}\n", murmur_lib::cleanup::tidy(&heard).replace('\n', " ⏎ "));
    }
}
