// cargo run --release --example transcribe -- clip.wav   (16-bit mono 16 kHz WAV)
fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
    murmur_lib::stt::load(&dir.join("whistle.cact")).unwrap();
    let wav = std::fs::read(std::env::args().nth(1).unwrap()).unwrap();
    let data = wav.windows(4).position(|w| w == b"data").unwrap() + 8;
    let pcm: Vec<f32> = wav[data..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
    println!("{}", murmur_lib::stt::transcribe(&pcm).unwrap());
}
