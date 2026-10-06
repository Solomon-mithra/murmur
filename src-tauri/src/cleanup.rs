//! Deterministic dictation cleanup, run before the LLM's light polish:
//! fillers, stutters, spoken self-corrections, and spoken list/line cues.

const FILLERS: &[&str] = &["um", "umm", "uh", "uhh", "uhm", "erm", "er", "hmm", "mm"];

/// Cues that drop the previous sentence entirely.
const SCRATCH: &[&str] = &["scratch that", "delete that", "forget that", "never mind that"];

/// Cues that correct what came just before them. Longest first.
const CORRECT: &[&str] = &[
    "oh no i meant", "no no i meant", "no sorry i meant", "sorry i meant", "no wait i meant", "oh wait i meant",
    "actually i meant", "no i meant", "i meant to say", "i meant", "no wait", "oh wait", "sorry no", "actually no",
    "or rather",
];

/// Spoken structure cues -> markdown-ish structure.
const BULLET: &[&str] = &["next bullet point", "new bullet point", "bullet point", "next bullet", "new bullet"];
const NEWLINE: &[&str] = &["new paragraph", "new line", "next line"];

fn norm(w: &str) -> String {
    w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase()
}

/// Index of a cue starting at word `i`, returning how many words it spans.
fn cue_at(words: &[&str], i: usize, cues: &[&str]) -> Option<usize> {
    cues.iter().find_map(|cue| {
        let parts: Vec<&str> = cue.split(' ').collect();
        let hit = parts.len() <= words.len() - i && parts.iter().zip(&words[i..]).all(|(p, w)| norm(w) == *p);
        hit.then_some(parts.len())
    })
}

fn ends_sentence(w: &str) -> bool {
    w.ends_with(['.', '!', '?'])
}

/// Start index (in `out`) of the sentence the last word belongs to.
/// If that word just ended a sentence, the cue refers to that sentence.
fn sentence_start(out: &[String]) -> usize {
    let end = out.len().saturating_sub(1);
    out[..end].iter().rposition(|w| ends_sentence(w)).map_or(0, |i| i + 1)
}

/// Apply a correction `fix` (words after the cue) to what was said so far.
fn apply_correction(out: &mut Vec<String>, fix: &[&str]) {
    let start = sentence_start(out);
    // "let's do X. oh no i meant let's do Y" -> anchor on the repeated lead word.
    if let Some(first) = fix.first().map(|w| norm(w)) {
        if let Some(p) = out[start..].iter().rposition(|w| norm(w) == first) {
            out.truncate(start + p);
            return;
        }
    }
    // "meet at three, no wait, four" -> a short fix replaces as many trailing words.
    if fix.len() <= 3 {
        let n = fix.len().min(out.len() - start);
        out.truncate(out.len() - n);
    } else {
        out.truncate(start);
    }
}

pub fn tidy(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        if let Some(n) = cue_at(&words, i, SCRATCH) {
            let start = sentence_start(&out);
            out.truncate(start);
            i += n;
            continue;
        }
        if let Some(n) = cue_at(&words, i, CORRECT) {
            let rest = &words[i + n..];
            let fix_len = rest.iter().position(|w| ends_sentence(w)).map_or(rest.len(), |p| p + 1);
            apply_correction(&mut out, &rest[..fix_len]);
            strip_trailing_punct(&mut out);
            i += n;
            continue;
        }
        if let Some(n) = cue_at(&words, i, BULLET) {
            strip_trailing_punct(&mut out);
            out.push("\n-".into());
            i += n;
            continue;
        }
        if let Some(n) = cue_at(&words, i, NEWLINE) {
            out.push("\n".into());
            i += n;
            continue;
        }
        let n = norm(w);
        let filler = FILLERS.contains(&n.as_str());
        let stutter = out.last().is_some_and(|p| norm(p) == n && !n.is_empty() && !ends_sentence(p));
        if !filler && !stutter {
            out.push(w.to_string());
        } else if filler && w.ends_with(['.', '!', '?']) {
            // keep the sentence boundary a filler was carrying
            if let Some(last) = out.last_mut() {
                if !ends_sentence(last) {
                    last.push('.');
                }
            }
        }
        i += 1;
    }
    let joined = out.join(" ").replace(" \n", "\n").replace("\n ", "\n");
    joined
        .lines()
        .map(|l| capitalize(l.trim().trim_start_matches(',').trim()))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("\n-", "\n- ")
        .replace("-  ", "- ")
        .trim()
        .to_string()
}

fn strip_trailing_punct(out: &mut [String]) {
    if let Some(last) = out.last_mut() {
        while last.ends_with([',', ';', ':']) {
            last.pop();
        }
    }
}

fn capitalize(line: &str) -> String {
    let (prefix, body) = match line.strip_prefix('-') {
        Some(b) => ("- ", b.trim_start().trim_start_matches(',').trim_start()),
        None => ("", line),
    };
    let mut c = body.chars();
    match c.next() {
        Some(f) => format!("{prefix}{}{}", f.to_uppercase(), c.as_str()),
        None => prefix.trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::tidy;

    #[test]
    fn fillers_and_stutters() {
        assert_eq!(tidy("Um, so I think we should meet at the the office."), "So I think we should meet at the office.");
    }

    #[test]
    fn correction_with_repeated_lead_word() {
        assert_eq!(
            tidy("Let's refactor the auth module. Oh no, I meant let's continue with the implementation of search."),
            "Let's continue with the implementation of search."
        );
    }

    #[test]
    fn short_correction_replaces_tail() {
        assert_eq!(tidy("Let's meet at three, no wait, four."), "Let's meet at four.");
    }

    #[test]
    fn scratch_that_drops_sentence() {
        assert_eq!(tidy("Send it to Bob. Scratch that. Send it to Alice."), "Send it to Alice.");
    }

    #[test]
    fn spoken_bullets() {
        assert_eq!(
            tidy("Things to do bullet point fix the login bug bullet point update the docs"),
            "Things to do\n- Fix the login bug\n- Update the docs"
        );
    }

    #[test]
    fn plain_text_untouched() {
        let s = "I wanted to let you know the client approved the design.";
        assert_eq!(tidy(s), s);
    }
}

/// The examples in README.md, verbatim, so the docs can't drift from the code.
#[cfg(test)]
mod readme_examples {
    use super::tidy;

    #[test]
    fn readme_table() {
        let rows = [
            ("Um, so we should meet at the the office.", "So we should meet at the office."),
            ("Let's refactor auth. Oh no, I meant let's continue with search.", "Let's continue with search."),
            ("Let's meet at three, no wait, four.", "Let's meet at four."),
            ("Send it to Bob. Scratch that. Send it to Alice.", "Send it to Alice."),
            ("Things to do bullet point fix login bullet point update docs", "Things to do\n- Fix login\n- Update docs"),
        ];
        for (said, typed) in rows {
            assert_eq!(tidy(said), typed, "\n  said: {said}");
        }
    }
}
