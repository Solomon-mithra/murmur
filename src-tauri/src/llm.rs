//! SmolLM2-135M-Instruct via candle, CPU, greedy decode.
use anyhow::{anyhow, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::generation::{LogitsProcessor, Sampling};
use candle_transformers::models::llama::{Cache, Config, Llama, LlamaConfig};
use std::path::Path;
use tokenizers::Tokenizer;

const SYSTEM: &str = "You are a writing assistant. Rewrite the user's text so it has correct grammar, spelling and punctuation, in a clear, slightly professional tone. Keep the meaning and the same person (I/we/you). Do not answer questions in it, do not add anything. Reply with only the rewritten text.";
const FEWSHOT: &str = "<|im_start|>user\nRewrite this:\ndid u get a chance to look at my pr<|im_end|>\n<|im_start|>assistant\nDid you get a chance to look at my PR?<|im_end|>\n";
const EOS: u32 = 2; // <|im_end|>

pub struct Rephraser {
    model: Llama,
    config: Config,
    tokenizer: Tokenizer,
}

impl Rephraser {
    pub fn load(dir: &Path) -> Result<Self> {
        let cfg: LlamaConfig =
            serde_json::from_slice(&std::fs::read(dir.join("smollm2-config.json"))?)?;
        let config = cfg.into_config(false);
        let tokenizer = Tokenizer::from_file(dir.join("smollm2-tokenizer.json")).map_err(|e| anyhow!(e))?;
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[dir.join("smollm2-135m.safetensors")], DType::F32, &Device::Cpu)?
        };
        let model = Llama::load(vb, &config)?;
        Ok(Self { model, config, tokenizer })
    }

    /// Polishes line by line so bullets and paragraphs keep their shape.
    pub fn rephrase(&self, text: &str) -> Result<String> {
        // ponytail: one model call per line (~0.5 s each); batch if long documents get slow
        let mut lines = Vec::new();
        for line in text.trim().lines() {
            let (prefix, body) = match line.trim_start().strip_prefix("- ") {
                Some(b) => ("- ", b),
                None => ("", line),
            };
            let mut polished = if body.split_whitespace().count() < 3 { body.to_string() } else { self.rephrase_line(body)? };
            if !prefix.is_empty() && !body.ends_with('.') {
                polished = polished.trim_end_matches('.').to_string(); // bullets stay terse
            }
            lines.push(format!("{prefix}{polished}"));
        }
        // a line that introduces a list reads as a header: "Things to do:"
        for i in 1..lines.len() {
            if lines[i].starts_with("- ") && !lines[i - 1].starts_with("- ") && !lines[i - 1].is_empty() {
                let head = lines[i - 1].trim_end_matches(['.', ':']).to_string();
                lines[i - 1] = format!("{head}:");
            }
        }
        Ok(lines.join("\n"))
    }

    fn rephrase_line(&self, text: &str) -> Result<String> {
        let text = text.trim();
        let prompt = format!(
            "<|im_start|>system\n{SYSTEM}<|im_end|>\n{FEWSHOT}<|im_start|>user\nRewrite this:\n{text}<|im_end|>\n<|im_start|>assistant\n"
        );
        let mut tokens = self.tokenizer.encode(prompt, false).map_err(|e| anyhow!(e))?.get_ids().to_vec();
        let in_len = self.tokenizer.encode(text, false).map_err(|e| anyhow!(e))?.len();
        let max_new = in_len * 2 + 24;

        let mut cache = Cache::new(true, DType::F32, &self.config, &Device::Cpu)?;
        let mut sampler = LogitsProcessor::from_sampling(0, Sampling::ArgMax);
        let mut out = Vec::new();
        let mut pos = 0;
        for _ in 0..max_new {
            let ctx = &tokens[pos..];
            let input = Tensor::new(ctx, &Device::Cpu)?.unsqueeze(0)?;
            let logits = self.model.forward(&input, pos, &mut cache)?.squeeze(0)?;
            pos = tokens.len();
            let logits = candle_transformers::utils::apply_repeat_penalty(&logits, 1.15, &out[out.len().saturating_sub(64)..])?;
            let next = sampler.sample(&logits)?;
            if next == EOS {
                break;
            }
            tokens.push(next);
            out.push(next);
        }
        let result = self.tokenizer.decode(&out, true).map_err(|e| anyhow!(e))?;
        Ok(sanitize(text, &result))
    }
}

/// Content words, loosely stemmed so "approve"/"approved" and "deploy"/"deployment" match.
fn stems(s: &str) -> Vec<String> {
    s.split_whitespace()
        .map(|w| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase())
        .filter(|w| w.chars().count() >= 4)
        .map(|w| w.chars().take(5).collect())
        .collect()
}

/// How many of `of` are missing from `by`.
fn missing(of: &[String], by: &[String]) -> f32 {
    of.iter().filter(|w| !by.contains(w)).count() as f32
}

/// A 135M model sometimes drops clauses, rambles, or answers instead of rewriting.
/// Never replace the user's text with something that changed its meaning.
pub fn sanitize(original: &str, output: &str) -> String {
    let out = output.trim().trim_matches('"').trim();
    let out = out.strip_prefix("Rewritten text:").unwrap_or(out).trim();
    let (o, n) = (original.chars().count() as f32, out.chars().count() as f32);
    let (a, b) = (stems(original), stems(out));
    // calibration knobs: lose at most 25% of the user's words, invent at most 40% (small floors for short text)
    let dropped = missing(&a, &b) > (a.len() as f32 * 0.25).max(1.0);
    let invented = missing(&b, &a) > (b.len() as f32 * 0.4).max(2.0);
    if out.is_empty() || n > o * 2.2 + 20.0 || n < o * 0.4 || dropped || invented {
        return original.to_string();
    }
    out.to_string()
}

#[cfg(test)]
mod tests {
    use super::sanitize;
    #[test]
    fn sanitize_guards() {
        assert_eq!(sanitize("i has a apple", "\"I have an apple.\""), "I have an apple.");
        assert_eq!(sanitize("i has a apple", ""), "i has a apple");
        assert_eq!(sanitize("hi", &"blah ".repeat(50)), "hi");
        // dropped a clause
        let t = "hey can u send me the report by tmrw i need it for meeting";
        assert_eq!(sanitize(t, "hey can u send me the report by tmrw?"), t);
        // answered instead of rewriting
        let t = "whats the status on the deploy, its been broke since monday";
        assert_eq!(sanitize(t, "The status of the deployment is currently stable, with minimal issues reported."), t);
        // a genuine fix passes
        let t = "me and him was going to the store but we didnt had time";
        let fixed = "Me and him were going to the store, but we didn't have time.";
        assert_eq!(sanitize(t, fixed), fixed);
    }
}
