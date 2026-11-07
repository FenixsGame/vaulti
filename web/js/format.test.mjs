import { test } from "node:test";
import assert from "node:assert/strict";
import { fmtBytes, fmtDate } from "./format.js";

test("fmtBytes", () => {
  assert.equal(fmtBytes(0), "0 B");
  assert.equal(fmtBytes(999), "999 B");
  assert.equal(fmtBytes(1024), "1.0 KB");
  assert.equal(fmtBytes(1073741824), "1.0 GB");
});

test("fmtDate", () => {
  assert.ok(fmtDate("2026-11-07T10:00:00").includes("2026"));
  assert.equal(fmtDate("non data"), "");
});
