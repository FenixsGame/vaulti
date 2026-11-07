// logica pura, senza DOM: testabile con node --test
export function fmtBytes(n) {
  return n >= 1073741824 ? (n / 1073741824).toFixed(1) + " GB"
    : n >= 1048576 ? (n / 1048576).toFixed(1) + " MB"
    : n >= 1024 ? (n / 1024).toFixed(1) + " KB"
    : n + " B";
}

export function fmtDate(iso) {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? "" : d.toLocaleString();
}
