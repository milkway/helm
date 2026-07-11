import "./style.css";
import { applyLang, initLang, LANGS, type Lang } from "./i18n";

// ── Cards de recursos (traduzidos por chave) ────────────────────────────
const featureDefs: { icon: string; key: string; tint: string }[] = [
  { icon: "⟳", key: "reconnect", tint: "text-[#63d29b]" },
  { icon: "⏏", key: "detach", tint: "text-[#e0a15e]" },
  { icon: "▦", key: "grid", tint: "text-[#5aa9e0]" },
  { icon: "⬢", key: "vault", tint: "text-[#e0a15e]" },
  { icon: "!", key: "attn", tint: "text-[#f0785a]" },
  { icon: "⌘", key: "cmd", tint: "text-[#5aa9e0]" },
  { icon: "⛨", key: "vpn", tint: "text-[#5aa9e0]" },
  { icon: "$", key: "ssh", tint: "text-[#63d29b]" },
  { icon: "◆", key: "native", tint: "text-[#e0a15e]" },
];

const grid = document.getElementById("feature-grid");
if (grid) {
  grid.innerHTML = featureDefs
    .map(
      (f, i) => `
    <div class="card rounded-2xl p-6 transition" data-rise style="animation-delay:${0.05 * (i % 3) + 0.1}s">
      <div class="flex h-11 w-11 items-center justify-center rounded-xl bg-white/[0.04] font-mono text-lg ${f.tint}">${f.icon}</div>
      <h3 class="mt-5 text-lg font-semibold tracking-tight" data-i18n="f.${f.key}.t"></h3>
      <p class="mt-2 text-sm leading-relaxed text-(--color-ink-2)" data-i18n="f.${f.key}.b"></p>
    </div>`,
    )
    .join("");
}

// ── i18n: aplica idioma + seletor ──────────────────────────────────────
let lang: Lang = initLang();

const flagEl = document.getElementById("lang-flag");
const codeEl = document.getElementById("lang-code");
const btn = document.getElementById("lang-btn");
const menu = document.getElementById("lang-menu");

function refreshSelector() {
  const cur = LANGS.find((l) => l.code === lang) ?? LANGS[0];
  if (flagEl) flagEl.textContent = cur.flag;
  if (codeEl) codeEl.textContent = cur.code.toUpperCase();
}

if (menu) {
  menu.innerHTML = LANGS.map(
    (l) => `
    <button data-lang="${l.code}" class="flex w-full items-center gap-2.5 rounded-lg px-3 py-2 text-left text-sm text-(--color-ink-2) transition hover:bg-white/5 hover:text-(--color-ink)">
      <span>${l.flag}</span><span>${l.label}</span>
    </button>`,
  ).join("");
}

btn?.addEventListener("click", (e) => {
  e.stopPropagation();
  menu?.classList.toggle("hidden");
});
document.addEventListener("click", () => menu?.classList.add("hidden"));
menu?.querySelectorAll<HTMLButtonElement>("[data-lang]").forEach((b) => {
  b.addEventListener("click", () => {
    lang = b.dataset.lang as Lang;
    applyLang(lang);
    refreshSelector();
    menu?.classList.add("hidden");
  });
});

applyLang(lang);
refreshSelector();

// ── Copiar comando ──────────────────────────────────────────────────────
document.querySelectorAll<HTMLButtonElement>("[data-copy]").forEach((cbtn) => {
  cbtn.addEventListener("click", async () => {
    await navigator.clipboard.writeText(cbtn.dataset.copy ?? "");
    const original = cbtn.textContent;
    cbtn.textContent = "✓";
    setTimeout(() => (cbtn.textContent = original), 1500);
  });
});

// ── Reveal on scroll ────────────────────────────────────────────────────
const io = new IntersectionObserver(
  (entries) => {
    for (const e of entries) {
      if (e.isIntersecting) {
        (e.target as HTMLElement).style.animationPlayState = "running";
        io.unobserve(e.target);
      }
    }
  },
  { threshold: 0.1 },
);
document.querySelectorAll("#feature-grid [data-rise]").forEach((el) => {
  (el as HTMLElement).style.animationPlayState = "paused";
  io.observe(el);
});
