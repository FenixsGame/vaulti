import { fmtBytes } from "./format.js";

export const $ = (id) => document.getElementById(id);

export async function api(p, opts = {}) {
  const r = await fetch(p, { credentials: "same-origin", ...opts });
  if (r.status === 401) { showLogin(); throw new Error("401"); }
  if (!r.ok) {
    const e = await r.json().catch(() => ({ error: `HTTP ${r.status}` }));
    throw new Error(e.error || `HTTP ${r.status}`);
  }
  return r.status === 204 ? null : r.json();
}

export function showLogin() {
  $("main").hidden = true;
  $("login").hidden = false;
  $("breg").hidden = false; // primo utente: visibile solo qui, il server rifiuta dopo con 403
}

export const fmt = fmtBytes;
