import "./style.css";

// ── Cards de recursos ──────────────────────────────────────────────────
const features: { icon: string; title: string; body: string; tint: string }[] = [
  {
    icon: "⟳",
    title: "Auto-reconnect + tmux",
    body: "Backoff exponencial (1–30s, 5 tentativas) e re-attach automático na sessão tmux do projeto. Nunca perca o contexto.",
    tint: "text-[#63d29b]",
  },
  {
    icon: "⏏",
    title: "Detach sem Ctrl+B",
    body: "Um clique destacha a sessão — o tmux segue rodando no servidor. Volte de onde parou, em qualquer máquina.",
    tint: "text-[#e0a15e]",
  },
  {
    icon: "▦",
    title: "Modo grid",
    body: "Vários terminais vivos lado a lado, densidade 2×/3×/4×. Copie a saída ou exporte um card como PNG.",
    tint: "text-[#5aa9e0]",
  },
  {
    icon: "⬢",
    title: "Cofre com Touch ID",
    body: "Credenciais no Keychain (macOS) / Secret Service (Linux). Só metadados no banco — o segredo nunca toca o disco.",
    tint: "text-[#e0a15e]",
  },
  {
    icon: "!",
    title: "Detecção de atenção",
    body: "O Helm percebe quando uma sessão em segundo plano está aguardando input e te avisa com um toque de destaque.",
    tint: "text-[#f0785a]",
  },
  {
    icon: "⌘",
    title: "⌘K + clmux",
    body: "Command palette para tudo, import do ~/.ssh/config e clmux: entra na pasta, abre o tmux e sobe o Claude num clique.",
    tint: "text-[#5aa9e0]",
  },
  {
    icon: "⛨",
    title: "VPN automática",
    body: "Conecta a VPN (Tunnelblick/nmcli) antes do SSH e desconecta quando o último host que a usa fecha. Por refcount.",
    tint: "text-[#5aa9e0]",
  },
  {
    icon: "$",
    title: "OpenSSH do sistema",
    body: "Orquestra o ssh do seu OS via ssh -tt — herda agent, ~/.ssh/config, ProxyCommand e known_hosts de graça.",
    tint: "text-[#63d29b]",
  },
  {
    icon: "◆",
    title: "Nativo e leve",
    body: "Tauri 2 + Rust: instalador de poucos MB, sem Electron. macOS e Linux, com CI e releases assinados.",
    tint: "text-[#e0a15e]",
  },
];

const grid = document.getElementById("feature-grid");
if (grid) {
  grid.innerHTML = features
    .map(
      (f, i) => `
    <div class="card rounded-2xl p-6 transition" data-rise style="animation-delay:${0.05 * (i % 3) + 0.1}s">
      <div class="flex h-11 w-11 items-center justify-center rounded-xl bg-white/[0.04] font-mono text-lg ${f.tint}">${f.icon}</div>
      <h3 class="mt-5 text-lg font-semibold tracking-tight">${f.title}</h3>
      <p class="mt-2 text-sm leading-relaxed text-(--color-ink-2)">${f.body}</p>
    </div>`,
    )
    .join("");
}

// ── Copiar comando ──────────────────────────────────────────────────────
document.querySelectorAll<HTMLButtonElement>("[data-copy]").forEach((btn) => {
  btn.addEventListener("click", async () => {
    await navigator.clipboard.writeText(btn.dataset.copy ?? "");
    const original = btn.textContent;
    btn.textContent = "copiado ✓";
    btn.classList.add("text-(--color-green)");
    setTimeout(() => {
      btn.textContent = original;
      btn.classList.remove("text-(--color-green)");
    }, 1500);
  });
});

// ── Reveal on scroll (para seções abaixo da dobra) ─────────────────────
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
