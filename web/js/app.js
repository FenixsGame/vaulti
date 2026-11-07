import { $, api, showLogin, fmt } from "./api.js";

let folderId = null;
let crumbPath = [{ id: null, name: "Home" }];

function merr(e) {
  $("merr").textContent = e.message || String(e);
}

async function listit(fid) {
  folderId = fid;
  const j = await api("/api/files" + (fid ? `?folder_id=${fid}` : ""));
  render(j);
  $("quota").textContent = "usati " + fmt(j.quota.used) + " · liberi " + fmt(j.quota.free);
}
const refresh = () => listit(folderId);

function btn(label, fn) {
  const b = document.createElement("button");
  b.textContent = label;
  b.addEventListener("click", () => fn().catch(merr));
  return b;
}

function row(first) {
  const tr = document.createElement("tr");
  const t = document.createElement("td");
  const a = document.createElement("td");
  if (first) t.append(first);
  tr.append(t, a);
  return [tr, t, a];
}

function render(j) {
  const tb = $("listing").querySelector("tbody");
  tb.replaceChildren();
  const cr = $("crumbs");
  cr.replaceChildren();
  crumbPath.forEach((c, i) => {
    if (i) cr.append(" / ");
    cr.append(btn(c.name, async () => {
      crumbPath = crumbPath.slice(0, i + 1);
      await listit(c.id);
    }));
  });
  const sel = $("mvsel");
  sel.replaceChildren();
  const home = document.createElement("option");
  home.value = "";
  home.textContent = "Home";
  sel.append(home);
  for (const f of j.folders) {
    const o = document.createElement("option");
    o.value = f.id;
    o.textContent = f.name;
    sel.append(o);
  }
  for (const f of j.folders) {
    const [tr, t, act] = row(btn(f.name, async () => {
      crumbPath.push({ id: f.id, name: f.name });
      await listit(f.id);
    }));
    act.append(
      btn("rinomina", async () => {
        const n = prompt("Nuovo nome", f.name);
        if (!n) return;
        await api(`/api/folders/${f.id}`, {
          method: "PATCH",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ name: n }),
        });
        await refresh();
      }),
      btn("elimina", async () => {
        await api(`/api/folders/${f.id}`, { method: "DELETE" });
        await refresh();
      }),
    );
    tb.append(tr);
  }
  for (const f of j.files) {
    const [tr, t, act] = row(null);
    const a = document.createElement("a");
    a.href = `/api/files/${f.id}/download`;
    a.textContent = f.name;
    t.append(a, " (" + fmt(f.size) + ")");
    act.append(
      btn("rinomina", async () => {
        const n = prompt("Nuovo nome", f.name);
        if (!n) return;
        await api(`/api/files/${f.id}`, {
          method: "PATCH",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ name: n }),
        });
        await refresh();
      }),
      btn("sposta", async () => {
        // "" = torna a Home (la PATCH con folder_id vuota imposta root)
        await api(`/api/files/${f.id}`, {
          method: "PATCH",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ folder_id: sel.value }),
        });
        await refresh();
      }),
      btn("elimina", async () => {
        await api(`/api/files/${f.id}`, { method: "DELETE" });
        await refresh();
      }),
    );
    tb.append(tr);
  }
  if (!j.folders.length && !j.files.length) {
    const tr = document.createElement("tr");
    const t = document.createElement("td");
    t.textContent = "cartella vuota";
    tb.append(tr);
    tr.append(t);
  }
}

async function upload(file) {
  const key = "vaulti-up:" + [file.name, file.size, file.lastModified].join(":");
  let saved = JSON.parse(localStorage.getItem(key) || "null");
  let recvd = new Set();
  let up = null;
  let cs = 0;
  if (saved?.id && saved?.cs) {
    try {
      const st = await api(`/api/upload/${saved.id}/status`);
      recvd = new Set(st.received);
      up = saved.id;
      cs = saved.cs;
    } catch {
      localStorage.removeItem(key); // upload morto (sweep 7d): riparti da zero
    }
  }
  if (!up) {
    const j = await api("/api/upload/start", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ name: file.name, size: file.size }),
    });
    up = j.upload_id;
    cs = j.chunk_size;
    localStorage.setItem(key, JSON.stringify({ id: up, cs }));
  }
  const total = Math.ceil(file.size / cs);
  for (let n = 0; n < total; n++) {
    if (recvd.has(n)) continue;
    const blob = file.slice(n * cs, Math.min(file.size, (n + 1) * cs));
    let ok = false;
    for (let att = 0; att < 5 && !ok; att++) {
      if (att) await new Promise((r) => setTimeout(r, 500 * 2 ** (att - 1))); // backoff
      try {
        const r = await fetch(`/api/upload/${up}/chunk/${n}`, {
          method: "PUT",
          credentials: "same-origin",
          headers: { "content-type": "application/octet-stream" },
          body: blob,
        });
        if (r.status === 204) ok = true;
        else throw new Error(`chunk ${n}: HTTP ${r.status}`);
      } catch (e) {
        if (att === 4) throw e;
      }
    }
    if (!ok) throw new Error(`chunk ${n}: non inviato`);
    recvd.add(n);
    $("prog").textContent = `${file.name}: ${recvd.size}/${total} blocchi`;
  }
  await api(`/api/upload/${up}/finish`, { method: "POST" });
  localStorage.removeItem(key);
  $("prog").textContent = `${file.name}: completo`;
}

$("blogin").addEventListener("click", async () => {
  try {
    await api("/auth/login", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ email: $("email").value.trim(), password: $("pw").value }),
    });
    $("login").hidden = true;
    $("main").hidden = false;
    crumbPath = [{ id: null, name: "Home" }];
    await refresh();
  } catch (e) {
    $("lerr").textContent = e.message;
  }
});

$("breg").addEventListener("click", async () => {
  try {
    await api("/auth/register", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ email: $("email").value.trim(), password: $("pw").value }),
    });
    $("blogin").click();
  } catch (e) {
    $("lerr").textContent = e.message;
  }
});

$("blogout").addEventListener("click", async () => {
  try { await api("/auth/logout", { method: "POST" }); } catch { /* comunque esci */ }
  showLogin();
});

$("bpick").addEventListener("click", () => $("file").click());
$("file").addEventListener("change", async () => {
  for (const f of $("file").files) {
    try { await upload(f); } catch (e) { merr(e); }
  }
  $("file").value = "";
  await refresh();
});

$("bfolder").addEventListener("click", async () => {
  const n = prompt("Nome cartella");
  if (!n) return;
  try {
    await api("/api/folders", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(n && folderId ? { name: n, parent_id: folderId } : { name: n }),
    });
    await refresh();
  } catch (e) { merr(e); }
});

// sessione già valida? entra direttamente (401 → schermata login)
listit(null)
  .then(() => { $("login").hidden = true; $("main").hidden = false; })
  .catch(() => {});
