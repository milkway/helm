import { useState } from "react";

export function Toggle({ on, onChange, label }: { on: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <div className="hx-toggle" onClick={() => onChange(!on)}>
      <div className={`hx-toggle__track${on ? " hx-toggle__track--on" : ""}`}>
        <div className="hx-toggle__knob" />
      </div>
      <span className="hx-toggle__label">{label}</span>
    </div>
  );
}

export function EyeIcon({ off }: { off: boolean }) {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
      <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7z" />
      <circle cx="12" cy="12" r="3" />
      {off && <path d="M3 3l18 18" />}
    </svg>
  );
}

/** Campo de senha com botão de mostrar/ocultar. */
export function PasswordField({
  value,
  onChange,
  placeholder,
  onEnter,
  autoFocus,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder: string;
  onEnter?: () => void;
  autoFocus?: boolean;
}) {
  const [show, setShow] = useState(false);
  return (
    <div className="pwd-field">
      <input
        className="add-cred__input pwd-field__input"
        type={show ? "text" : "password"}
        placeholder={placeholder}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && onEnter?.()}
        autoFocus={autoFocus}
      />
      <span
        className="pwd-field__eye"
        title={show ? "Ocultar" : "Mostrar"}
        onClick={() => setShow((v) => !v)}
      >
        <EyeIcon off={show} />
      </span>
    </div>
  );
}
