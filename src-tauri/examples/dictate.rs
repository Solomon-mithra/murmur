// Full dictation text pipeline: transcript -> rules -> light polish.
// cargo run --release --example dictate [-- "transcript" ...]
const CASES: &[&str] = &[
    "Um, so I think we should, like, meet tomorrow at three to discuss the the budget.",
    "Let's refactor the auth module. Oh no, I meant let's continue with the implementation of search.",
    "Let's meet at three, no wait, four.",
    "Things to do bullet point fix the login bug bullet point update the docs bullet point email Sarah about the launch",
    "Whats the status on the deploy, its been broke since Monday.",
    "Send the draft to Bob. Scratch that. Send it to Alice and cc the team.",
];
fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
    let r = murmur_lib::llm::Rephraser::load(&dir).unwrap();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cases: Vec<&str> = if args.is_empty() { CASES.to_vec() } else { args.iter().map(|s| s.as_str()).collect() };
    for t in cases {
        let tidy = murmur_lib::cleanup::tidy(t);
        let s = std::time::Instant::now();
        let out = r.rephrase(&tidy).unwrap();
        println!("IN:    {t}\nRULES: {tidy}\nFINAL: {out}  ({:?})\n", s.elapsed());
    }
}
