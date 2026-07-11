import { useLangStore, type Lang } from "./lang";
import { dict } from "./dict";

export { useLangStore, LANGS, type Lang } from "./lang";

type Vars = Record<string, string | number>;

function interpolate(s: string, vars?: Vars): string {
  if (!vars) return s;
  return s.replace(/\{(\w+)\}/g, (_, k) => String(vars[k] ?? `{${k}}`));
}

/** Tradução fora de componentes React (ex.: stores). */
export function translate(lang: Lang, key: string, vars?: Vars): string {
  const s = dict[lang]?.[key] ?? dict.en[key] ?? key;
  return interpolate(s, vars);
}

/** Hook: retorna t(key, vars) que reage à troca de idioma. */
export function useT(): (key: string, vars?: Vars) => string {
  const lang = useLangStore((s) => s.lang);
  return (key, vars) => translate(lang, key, vars);
}
