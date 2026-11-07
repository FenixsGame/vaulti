// tripwire: la regola [hidden] deve battere qualsiasi display autoriale
// (senza di essa, la login card resta a schermo dopo l'accesso — review Fase 1)
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const css = readFileSync(new URL("../css/base.css", import.meta.url), "utf8");

test("[hidden] annulla ogni display autoriale", () => {
  assert.match(css, /\[hidden\][^}]*display\s*:\s*none\s*!important/);
});
