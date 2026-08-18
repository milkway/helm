import { useMemo, useRef, useState } from "react";
import { detectRemote, invalidateRemoteInfo, type RemoteInfo } from "../lib/ipc";
import { useHostsStore } from "../stores/hosts";
import { useSessionsStore } from "../stores/sessions";
import { useUiStore, type Agent } from "../stores/ui";
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
  const defaultAgent = useUiStore((s) => s.defaultAgent);
  const setDefaultAgent = useUiStore((s) => s.setDefaultAgent);

  const [hostId, setHostId] = useState(presetHostId ?? hosts[0]?.id ?? "");
  const [project, setProject] = useState("");
  const [dir, setDir] = useState("");
  const [mode, setMode] = useState<Mode>("clmux");
  const [agent, setAgent] = useState<Agent>(defaultAgent);
  const [rememberAgent, setRememberAgent] = useState(false);
  const [remoteInfo, setRemoteInfo] = useState<{ hostId: string; info: RemoteInfo } | null>(null);
  const [probingHostId, setProbingHostId] = useState<string | null>(null);
  const [probeErrorHostId, setProbeErrorHostId] = useState<string | null>(null);
  const currentHostId = useRef(hostId);
  const probeRequestId = useRef(0);
  const latestProbeRequestByHost = useRef(new Map<string, number>());
  currentHostId.current = hostId;

  const probeRemote = (force = false) => {
    if (!hostId || probingHostId === hostId) return;
    if (!force && remoteInfo?.hostId === hostId) return;
    const targetHostId = hostId;
    const requestId = ++probeRequestId.current;
    latestProbeRequestByHost.current.set(targetHostId, requestId);
    if (force) {
      invalidateRemoteInfo(targetHostId);
      setRemoteInfo(null);
    }
    setProbingHostId(targetHostId);
    setProbeErrorHostId(null);
    void detectRemote(targetHostId)
      .then((info) => {
        if (
          latestProbeRequestByHost.current.get(targetHostId) === requestId &&
          currentHostId.current === targetHostId
        ) {
          setRemoteInfo({ hostId: targetHostId, info });
        }
      })
      .catch(() => {
        if (
          latestProbeRequestByHost.current.get(targetHostId) === requestId &&
          currentHostId.current === targetHostId
        ) {
          setProbeErrorHostId(targetHostId);
        }
      })
      .finally(() => {
        if (latestProbeRequestByHost.current.get(targetHostId) === requestId) {
          setProbingHostId((current) => current === targetHostId ? null : current);
        }
      });
  };

  const host = hosts.find((h) => h.id === hostId);
  const hostSession = sessions.find((s) => s.hostId === hostId);
  const currentRemoteInfo = remoteInfo?.hostId === hostId ? remoteInfo.info : null;
  const probing = probingHostId === hostId;
  const probeFailed = probeErrorHostId === hostId;
  const sessionName = tmuxSessionName(project.trim() || host?.name || "helm");
  const valid = !!host;

  const modes = useMemo(
    () => [
      { id: "shell" as Mode, title: t("ns.shell"), sub: t("ns.shellSub") },
      { id: "tmux" as Mode, title: t("ns.tmux"), sub: `tmux new -As ${sessionName}` },
      {
        id: "clmux" as Mode,
        title: t("ns.clmux"),
        sub: `cd ${dir.trim() || "~"} · tmux · ${agent}`,
        badge: t("ns.default"),
      },
    ],
    [t, sessionName, dir, agent],
  );

  const submit = () => {
    if (!valid) return;
    if (mode === "clmux" && rememberAgent) setDefaultAgent(agent);
    open(hostId, {
      mode,
      sessionName: mode === "shell" ? undefined : sessionName,
      projectDir: dir.trim() || undefined,
      agent: mode === "clmux" ? agent : undefined,
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
                onChange={(e) => {
                  const nextHostId = e.target.value;
                  currentHostId.current = nextHostId;
                  setHostId(nextHostId);
                  setRemoteInfo(null);
                  setProbeErrorHostId(null);
                }}
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
                      {m.id === "clmux" && (
                        <label className="hxm__radio-sub" onClick={(e) => e.stopPropagation()}>
                          <input
                            type="checkbox"
                            checked={rememberAgent}
                            onChange={(e) => setRememberAgent(e.target.checked)}
                          />{" "}
                          {t("ns.rememberAgent")}
                        </label>
                      )}
                    </div>
                    {m.id === "clmux" && (
                      <div
                        className="hxm__agent"
                        onClick={(e) => {
                          e.stopPropagation();
                          setMode("clmux");
                          probeRemote();
                        }}
                      >
                        <select
                          className="hxm__agent-select"
                          value={agent}
                          aria-label={t("ns.agent")}
                          onFocus={() => probeRemote()}
                          onChange={(e) => {
                            setAgent(e.target.value as Agent);
                            setMode("clmux");
                          }}
                        >
                          <option value="claude">{t("ns.claude")}</option>
                          <option value="codex">{t("ns.codex")}</option>
                        </select>
                        {currentRemoteInfo && !currentRemoteInfo[agent] && (
                          <span className="hxm__agent-missing">{t("ns.notDetected")}</span>
                        )}
                        {!currentRemoteInfo && !probing && !probeFailed && (
                          <span className="hxm__agent-missing">
                            {t("ns.notVerified")} · {t("ns.checkHost")}
                          </span>
                        )}
                        {probeFailed && !currentRemoteInfo && (
                          <span className="hxm__agent-missing">{t("ns.checkFailed")}</span>
                        )}
                        <button
                          type="button"
                          className="hxm__agent-check"
                          disabled={probing}
                          onClick={(e) => {
                            e.stopPropagation();
                            setMode("clmux");
                            probeRemote(true);
                          }}
                        >
                          {probing ? t("ns.checkingHost") : t("ns.checkHost")}
                        </button>
                      </div>
                    )}
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
