import { useMemo, useState } from "react";
import { useHostsStore } from "../stores/hosts";
import { useSessionsStore } from "../stores/sessions";
import { useUiStore } from "../stores/ui";
import { statusColor, tmuxSessionName } from "../types";
import { useT } from "../i18n";

type Mode = "shell" | "tmux" | "clmux";

/** Nova sessão (projeto) — design 1c. */
export function NewSessionModal({ presetHostId }: { presetHostId?: string }) {
  const t = useT();
  const closeModal = useUiStore((s) => s.closeModal);
  const hosts = useHostsStore((s) => s.hosts);
  const sessions = useSessionsStore((s) => s.sessions);
  const open = useSessionsStore((s) => s.open);

  const [hostId, setHostId] = useState(presetHostId ?? hosts[0]?.id ?? "");
  const [project, setProject] = useState("");
  const [dir, setDir] = useState("");
  const [mode, setMode] = useState<Mode>("clmux");

  const host = hosts.find((h) => h.id === hostId);
  const hostSession = sessions.find((s) => s.hostId === hostId);
  const sessionName = tmuxSessionName(project.trim() || host?.name || "helm");
  const valid = !!host;

  const modes = useMemo(
    () => [
      { id: "shell" as Mode, title: t("ns.shell"), sub: t("ns.shellSub") },
      { id: "tmux" as Mode, title: t("ns.tmux"), sub: `tmux new -As ${sessionName}` },
      {
        id: "clmux" as Mode,
        title: t("ns.clmux"),
        sub: `cd ${dir.trim() || "~"} · tmux · claude`,
        badge: t("ns.default"),
      },
    ],
    [t, sessionName, dir],
  );

  const submit = () => {
    if (!valid) return;
    open(hostId, {
      mode,
      sessionName: mode === "shell" ? undefined : sessionName,
      projectDir: dir.trim() || undefined,
    });
    closeModal();
  };

  return (
    <div className="modal-backdrop" onClick={closeModal}>
      <div className="hxm" onClick={(e) => e.stopPropagation()}>
        <div className="hxm__header">
          <div className="hxm__icon hxm__icon--mono">$</div>
          <div className="hxm__titles">
            <div className="hxm__title">{t("ns.title")}</div>
            <div className="hxm__sub">{t("ns.sub")}</div>
          </div>
          <span className="hxm__close" onClick={closeModal}>×</span>
        </div>

        <div className="hxm__body">
          <div>
            <div className="hxm__label">{t("ns.host")}</div>
            <div className="hxm__host-pick">
              <span
                className="hxm__host-dot"
                style={{ background: statusColor(hostSession?.status) }}
              />
              <select
                className="hxm__host-select"
                value={hostId}
                onChange={(e) => setHostId(e.target.value)}
              >
                {hosts.map((h) => (
                  <option key={h.id} value={h.id}>
                    {h.name} — {h.user ? `${h.user}@${h.host}` : h.host}
                  </option>
                ))}
              </select>
            </div>
          </div>

          <div className="hxm__grid" style={{ gridTemplateColumns: "1fr 1fr" }}>
            <div>
              <div className="hxm__label">{t("ns.project")}</div>
              <input
                className="hxm__input hxm__input--mono"
                value={project}
                onChange={(e) => setProject(e.target.value)}
                placeholder={host ? tmuxSessionName(host.name) : "atlas-api"}
                autoFocus
              />
            </div>
            <div>
              <div className="hxm__label">{t("ns.folder")}</div>
              <input
                className="hxm__input hxm__input--mono"
                value={dir}
                onChange={(e) => setDir(e.target.value)}
                placeholder="~/apps/atlas-api"
              />
            </div>
          </div>

          <div>
            <div className="hxm__label">{t("ns.onConnect")}</div>
            <div className="hxm__radios">
              {modes.map((m) => {
                const on = mode === m.id;
                return (
                  <div
                    key={m.id}
                    className={`hxm__radio${on ? " hxm__radio--on" : ""}${m.id === "clmux" ? " hxm__radio--clmux" : ""}`}
                    onClick={() => setMode(m.id)}
                  >
                    <span className={`hxm__radio-dot${on ? " hxm__radio-dot--on" : ""}`} />
                    <div className="hxm__radio-body">
                      <div className="hxm__radio-title">{m.title}</div>
                      <div className="hxm__radio-sub">{m.sub}</div>
                    </div>
                    {m.badge && <span className="hxm__badge hxm__badge--amber">{m.badge}</span>}
                  </div>
                );
              })}
            </div>
          </div>
        </div>

        <div className="hxm__footer">
          <div className="hxm__btn hxm__btn--ghost" onClick={closeModal}>{t("hm.cancel")}</div>
          <div className={`hxm__btn hxm__btn--primary${valid ? "" : " hxm__btn--off"}`} onClick={submit}>
            {t("ns.open")}
          </div>
        </div>
      </div>
    </div>
  );
}
