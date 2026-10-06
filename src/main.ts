/// <reference types="vite/client" />
import { listen } from "@tauri-apps/api/event";

type State = "loading" | "idle" | "listening" | "thinking" | "polishing" | "done" | "error" | "access";

const pill = document.getElementById("pill")!;
const label = pill.querySelector<HTMLElement>(".label")!;
const clock = pill.querySelector<HTMLElement>(".clock")!;
const canvas = document.getElementById("silk") as HTMLCanvasElement;
const ctx = canvas.getContext("2d")!;

const LABELS: Partial<Record<State, string>> = {
  thinking: "transcribing",
  polishing: "polishing",
  access: "Grant Murmur keyboard access",
};

let state: State = "loading";
let level = 0; // smoothed mic level 0..1
let target = 0;
let started = 0;
let settle: number | undefined;
let raf = 0;

function set(next: State, text = "") {
  clearTimeout(settle);
  state = next;
  pill.dataset.state = next;
  // Re-trigger the shake / rise animations on repeated states.
  pill.style.animation = "none";
  void pill.offsetWidth;
  pill.style.animation = "";
  label.textContent = text || LABELS[next] || "";
  if (next === "listening") started = performance.now();
  if (next === "done" || next === "error") {
    settle = window.setTimeout(() => set("idle"), next === "done" ? 1900 : 2600);
  }
  if (["listening", "thinking", "polishing"].includes(next) && !raf) raf = requestAnimationFrame(draw);
}

const css = (v: string) => getComputedStyle(pill).getPropertyValue(v).trim();

function draw(t: number) {
  if (!["listening", "thinking", "polishing"].includes(state)) {
    raf = 0;
    target = level = 0;
    return;
  }
  raf = requestAnimationFrame(draw);

  // fit canvas to its box at device resolution
  const dpr = devicePixelRatio || 1;
  const { width: w, height: h } = canvas.getBoundingClientRect();
  if (canvas.width !== Math.round(w * dpr)) {
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);

  level += (target - level) * (target > level ? 0.35 : 0.08);
  target *= 0.92;
  pill.style.setProperty("--lvl", level.toFixed(3));

  if (state === "listening") {
    const s = Math.floor((performance.now() - started) / 1000);
    clock.textContent = `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  }

  const colors = [css("--c1"), css("--c2"), css("--c3")];
  const mid = h / 2;
  const sec = t / 1000;
  ctx.globalCompositeOperation = "lighter";
  ctx.lineCap = "round";

  // three silk threads: amplitude follows the voice while listening,
  // braid tightly while polishing, flatten into a rail while thinking
  const amp =
    state === "listening" ? 1.2 + level * mid * 0.85 : state === "polishing" ? mid * 0.42 : 1.2;
  const speed = state === "polishing" ? 5 : 2.4;
  colors.forEach((color, i) => {
    ctx.beginPath();
    for (let x = 0; x <= w; x += 1.5) {
      const u = x / w;
      const env = Math.pow(Math.sin(Math.PI * u), 1.6);
      const k = 2.2 + i * 0.9;
      const y =
        mid +
        amp * env * Math.sin(u * Math.PI * k + sec * speed * (1 + i * 0.35) + i * 2.1) *
          (state === "polishing" ? (i % 2 ? -1 : 1) : 1);
      x === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y);
    }
    ctx.strokeStyle = color;
    ctx.globalAlpha = 0.75;
    ctx.lineWidth = 1.6;
    ctx.shadowColor = color;
    ctx.shadowBlur = 8;
    ctx.stroke();
  });

  if (state === "thinking") {
    // beads racing along the rail
    colors.forEach((color, i) => {
      const p = (sec * 0.9 + i / 3) % 1;
      const x = w * (0.04 + 0.92 * (0.5 - 0.5 * Math.cos(Math.PI * 2 * p))); // ping-pong
      ctx.globalAlpha = 1;
      ctx.fillStyle = color;
      ctx.shadowBlur = 12;
      ctx.beginPath();
      ctx.arc(x, mid, 2.2, 0, Math.PI * 2);
      ctx.fill();
    });
  }
  ctx.globalAlpha = 1;
  ctx.shadowBlur = 0;
}

listen<[State, string]>("state", (e) => set(...e.payload));
listen<number>("level", (e) => {
  target = Math.max(target, Math.min(1, Math.sqrt(e.payload) * 2.4));
});

// dev-only: drive states from the browser console / preview
if (import.meta.env.DEV) Object.assign(window, { set, mic: (v: number) => (target = v) });
