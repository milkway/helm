import { useEffect, useRef, useState } from "react";
import { LANGS, useLangStore } from "../i18n";

export function LanguageSelector() {
  const lang = useLangStore((s) => s.lang);
  const setLang = useLangStore((s) => s.setLang);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onClick);
    return () => window.removeEventListener("mousedown", onClick);
  }, [open]);

  const current = LANGS.find((l) => l.code === lang) ?? LANGS[0];

  return (
    <div ref={ref} className="lang">
      <div className="lang__btn" title="Language / Idioma" onClick={() => setOpen((v) => !v)}>
        <span className="lang__flag">{current.flag}</span>
        <span className="lang__code">{current.code.toUpperCase()}</span>
      </div>
      {open && (
        <div className="lang__menu">
          {LANGS.map((l) => (
            <div
              key={l.code}
              className={`lang__item${l.code === lang ? " lang__item--on" : ""}`}
              onClick={() => {
                setLang(l.code);
                setOpen(false);
              }}
            >
              <span className="lang__flag">{l.flag}</span>
              <span>{l.label}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
