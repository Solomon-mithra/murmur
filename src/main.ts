/// <reference types="vite/client" />
import { listen } from "@tauri-apps/api/event";

type State = "loading" | "idle" | "listening" | "thinking" | "polishing" | "done" | "error" | "access";

/** One calm face per state. Only the eyes, mouth and a small accent change. */
const LOOKS: Record<Exclude<State, "idle">, { eyes: string; mouth: string; cls?: string; accent?: string; drift?: boolean; shine?: boolean; blinks?: boolean; smile?: boolean }> = {
  loading: { eyes: "–", mouth: "ω", accent: "z", drift: true, blinks: false },
  listening: { eyes: "•", mouth: "ᴗ", cls: "hearing" },
  thinking: { eyes: "•", mouth: "", cls: "writing" },
  polishing: { eyes: "˘", mouth: "ᴗ", cls: "gleaming" },
  done: { eyes: "•", mouth: "‿", blinks: false, smile: true },
  error: { eyes: "•", mouth: "_", accent: "?", cls: "tilted" },
  access: { eyes: "•", mouth: "ᴖ", shine: true },
};

const esc = (s: string) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);
const TEXT: Record<Exclude<State, "idle">, (t: string) => string> = {
  loading: () => `<span class="muted">waking up</span>`,
  listening: () => `<span class="label">Listening</span><span class="time" data-time>0:00</span>`,
  thinking: () => `<span class="muted">writing it down</span>`,
  polishing: () => `<span class="sheen">polishing</span>`,
  done: (t) => `<svg class="tick" viewBox="0 0 16 16"><path d="M3 8.5l3.2 3L13 4.5"/></svg><span class="snip">${esc(t)}</span>`,
  error: (t) => `<span class="muted">${esc(t || "didn't catch that")}</span>`,
  access: () => `<span class="label">may I come in?</span><span class="chip">Accessibility</span>`,
};

const $ = <T extends Element = HTMLElement>(s: string) => document.querySelector(s) as T;
const pill = $("#pill");
const inner = $(".inner");
const txt = $(".txt");
const char = $(".char");
const lift = $(".lift");
const eyes = [...document.querySelectorAll<HTMLElement>(".eye")];
const mouth = $(".mouth");
const accent = $(".accent");
const ears = [...document.querySelectorAll<HTMLElement>(".ears span")];

const rand = (a: number, b: number) => a + Math.random() * (b - a);
let state: State = "idle";
let blinkTimer = 0, smileTimer = 0, settleTimer = 0, accentTimer = 0;
let level = 0, target = 0, started = 0;
const bars = [0, 0, 0];

function blink() {
  eyes.forEach((e) => e.classList.add("shut"));
  setTimeout(() => eyes.forEach((e) => e.classList.remove("shut")), 130);
}
function scheduleBlink(ms: number) {
  blinkTimer = window.setTimeout(() => {
    blink();
    if (Math.random() < 0.12) setTimeout(blink, 260);
    scheduleBlink(rand(2800, 5200));
  }, ms);
}

function dress(next: Exclude<State, "idle">, text: string) {
  const L = LOOKS[next];
  char.className = "char " + (L.cls ?? "");
  eyes.forEach((e) => {
    e.textContent = L.eyes;
    e.classList.toggle("shine", !!L.shine);
    e.classList.remove("shut");
  });
  mouth.textContent = L.mouth;
  accent.textContent = L.accent ?? "";
  accent.className = "accent";
  if (L.accent) accentTimer = window.setTimeout(() => accent.classList.add("on", ...(L.drift ? ["drift"] : [])), 500);
  lift.style.transform = "";
  lift.style.animation = "";
  if (L.smile) {
    // the satisfying moment: a beat, then the eyes close into a smile
    smileTimer = window.setTimeout(() => {
      eyes.forEach((e) => e.classList.add("shut"));
      setTimeout(() => {
        eyes.forEach((e) => {
          e.textContent = "^";
          e.classList.remove("shut");
        });
        lift.style.animation = "bob .9s cubic-bezier(.37,0,.63,1)";
      }, 110);
    }, 380);
  }
  if (L.blinks !== false) scheduleBlink(rand(900, 1800));
  txt.innerHTML = TEXT[next](text);
  pill.style.width = inner.scrollWidth + 34 + "px";
  inner.classList.remove("fading");
}

function set(next: State, text = "") {
  [blinkTimer, smileTimer, settleTimer, accentTimer].forEach(clearTimeout);
  const wasHidden = pill.classList.contains("hidden");
  state = next;

  if (next === "idle") {
    if (wasHidden) return;
    pill.animate(
      [{ opacity: 1, transform: "translateY(0) scale(1)" }, { opacity: 0, transform: "translateY(6px) scale(.97)" }],
      { duration: 420, easing: "cubic-bezier(.32,0,.67,0)" },
    ).onfinish = () => state === "idle" && pill.classList.add("hidden");
    return;
  }

  if (next === "listening") {
    started = performance.now();
    level = target = 0;
    bars.fill(0);
  }
  if (wasHidden) {
    dress(next, text);
    pill.classList.remove("hidden");
    pill.animate(
      [{ opacity: 0, transform: "translateY(8px) scale(.96)" }, { opacity: 1, transform: "translateY(0) scale(1)" }],
      { duration: 520, easing: "cubic-bezier(.33,1,.68,1)" },
    );
  } else {
    inner.classList.add("fading");
    setTimeout(() => state === next && dress(next, text), 170);
  }
  if (next === "done" || next === "error") settleTimer = window.setTimeout(() => set("idle"), next === "done" ? 2200 : 2600);
}

// listening: soft responses to the real microphone level
function frame(now: number) {
  requestAnimationFrame(frame);
  if (state !== "listening") return;
  level += (target - level) * (target > level ? 0.18 : 0.05); // soft attack, slow release
  target *= 0.9;
  ears.forEach((bar, i) => {
    const wobble = 0.75 + 0.25 * Math.sin(now / (260 + i * 70) + i * 1.7);
    const goal = 0.28 + 0.72 * Math.min(1, level * (i === 1 ? 1.25 : 1) * wobble);
    bars[i] += (goal - bars[i]) * 0.2;
    bar.style.transform = `scaleY(${bars[i].toFixed(3)})`;
  });
  lift.style.transform = `rotate(${(level * 3).toFixed(2)}deg) translateY(${(-level).toFixed(2)}px)`; // leans in
  const t = txt.querySelector("[data-time]");
  if (t) {
    const s = Math.floor((now - started) / 1000);
    t.textContent = `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  }
}
requestAnimationFrame(frame);

listen<[State, string]>("state", (e) => set(...e.payload));
listen<number>("level", (e) => {
  target = Math.max(target, Math.min(1, Math.sqrt(e.payload) * 2.4));
});

// dev-only: drive states from the browser console / preview
if (import.meta.env.DEV) Object.assign(window, { set, mic: (v: number) => (target = v) });
