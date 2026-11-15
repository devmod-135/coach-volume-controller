"use strict";

const { invoke } = window.__TAURI__.core;
const dialog = window.__TAURI__.dialog;
const appWindow = window.__TAURI__.window?.getCurrentWindow?.();

const DB_MIN = -30, DB_MAX = 20;
const NEW_MOD = "__new__";
const $ = (id) => document.getElementById(id);

const state = {
  gameDir: null,
  modDir: "",
  defaultModName: "",
  lastSession: "",          // internal preset name of the auto-saved session
  labels: { ko: { slots: {}, parts: {} }, en: { slots: {}, parts: {} } },
  coaches: [],
  parts: [],
  events: [],
  skipped: [],
  mods: [],
  current: { path: null, name: "" },   // the mod being edited
  settings: emptySettings(),
  rows: new Map(),                     // key -> { el, range, val }
};

function emptySettings() {
  return { version: 1, global_db: 0, per_event: {}, source_sha1: {} };
}

// ------------------------------------------------------------- helpers
const pct = (db) => `${Math.round(10 ** (db / 20) * 100)}%`;
const fmt = (db) => `${db >= 0 ? "+" : ""}${db.toFixed(1)}`;
const total = (key) => state.settings.global_db + (state.settings.per_event[key] || 0);
const coachName = (id) => state.coaches.find((c) => c.id === id)?.name || id;
const basename = (p) => (p || "").split(/[\\/]/).pop();
const status = (msg) => { $("status").textContent = msg; $("status").title = msg; };
const slotLabel = (s) => state.labels[LANG]?.slots[s] || s;
const partName = (p) => state.labels[LANG]?.parts[p] || p;
const presetLabel = (n) => (n === state.lastSession ? t("lastSession") : n);

function evLabel(ev) {
  return [...new Set(ev.slots.map(slotLabel))].join(" / ");
}

function zeroPos(range) {
  range.style.setProperty("--zero", `${((0 - DB_MIN) / (DB_MAX - DB_MIN)) * 100}%`);
}

async function call(cmd, args) {
  try {
    return await invoke(cmd, args);
  } catch (e) {
    throw new Error(typeof e === "string" ? e : e?.message || String(e));
  }
}

// ------------------------------------------------------------- modal
function modal({ title, body, ok, cancel, onOpen }) {
  const dlg = $("modal");
  $("mTitle").textContent = title;
  const mb = $("mBody");
  mb.replaceChildren();
  if (typeof body === "string") mb.append(h("p", {}, body));
  else if (body) mb.append(body);
  $("mOk").textContent = ok || t("ok");
  $("mOk").disabled = false;
  const cancelText = cancel === null ? null : cancel || t("cancel");
  $("mCancel").textContent = cancelText || "";
  $("mCancel").style.display = cancelText ? "" : "none";
  return new Promise((resolve) => {
    dlg.returnValue = "cancel";
    dlg.addEventListener("close", () => resolve(dlg.returnValue === "ok"), { once: true });
    dlg.showModal();
    onOpen?.();
  });
}
const alertBox = (title, body) => modal({ title, body, cancel: null });
const confirmBox = (title, body, ok) => modal({ title, body, ok });

function h(tag, attrs = {}, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") el.className = v;
    else if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
    else if (v !== false && v != null) el.setAttribute(k, v === true ? "" : v);
  }
  for (const c of children.flat()) if (c != null) el.append(c);
  return el;
}

// ------------------------------------------------------------- language
async function setLanguage(lang, persist = true) {
  LANG = lang === "en" ? "en" : "ko";
  $("langSelect").value = LANG;
  if (persist) await call("set_lang", { lang: LANG }).catch(() => {});
  applyStaticText();
  appWindow?.setTitle(t("title")).catch(() => {});
  renderFilters();
  renderModSelect();
  await refreshPresets();
  populate(false);
  if (state.gameDir) status(summaryText());
}

function summaryText(extra = "") {
  let msg = t("summary", { n: state.events.length, c: state.coaches.length }) + extra;
  if (state.skipped.length) {
    msg += t("excluded") + state.skipped.map((sk) => {
      const lab = [...new Set(sk.slots.map(slotLabel))].join("/");
      return sk.all_coaches ? lab : `${lab}(${[...new Set(sk.coaches)].join(", ")})`;
    }).join(", ");
  }
  return msg;
}

// ------------------------------------------------------------- settings / labels
function refreshLabels() {
  const g = state.settings.global_db;
  $("globalValue").textContent = `${fmt(g)} dB (${pct(g)})`;
  $("globalSlider").classList.toggle("nonzero", Math.abs(g) >= 0.05);
  for (const [key, row] of state.rows) {
    const ind = state.settings.per_event[key] || 0;
    const tot = total(key);
    // only numbers are interpolated here
    row.val.innerHTML = t("valueLine", { ind: fmt(ind), t: fmt(tot), pct: pct(tot) });
    row.val.className = "val " + (tot > 12 ? "hot" : tot >= 0.05 ? "up" : tot <= -0.05 ? "down" : "");
    row.range.classList.toggle("nonzero", Math.abs(ind) >= 0.05);
    row.el.classList.toggle("changed", Math.abs(tot) >= 0.05);
  }
  scheduleAutosave();
}

function setGlobal(v) {
  state.settings.global_db = v;
  $("globalSlider").value = v;
  refreshLabels();
}

function setEvent(key, v) {
  if (Math.abs(v) < 0.05) delete state.settings.per_event[key];
  else state.settings.per_event[key] = v;
  refreshLabels();
}

let autosaveTimer = null;
function scheduleAutosave() {
  if (!state.gameDir) return;
  clearTimeout(autosaveTimer);
  autosaveTimer = setTimeout(() => {
    call("save_preset", { name: state.lastSession, settings: state.settings }).catch(() => {});
  }, 1200);
}

function applySettings(s, msg) {
  state.settings = { ...emptySettings(), ...s, per_event: { ...(s.per_event || {}) } };
  $("globalSlider").value = state.settings.global_db;
  populate();
  const known = new Set(state.events.map((e) => e.key));
  const unknown = Object.keys(state.settings.per_event).filter((k) => !known.has(k)).length;
  if (unknown) msg += t("unknownIgnored", { n: unknown });
  status(msg + t("applyHint"));
}

// ------------------------------------------------------------- list
function renderFilters() {
  const cf = $("coachFilter"), pf = $("catFilter");
  const cv = cf.value, pv = pf.value;
  cf.replaceChildren(h("option", { value: "" }, t("all")), ...state.coaches.map((c) => h("option", { value: c.id }, c.name)));
  pf.replaceChildren(h("option", { value: "" }, t("all")), ...state.parts.map((p) => h("option", { value: p }, partName(p))));
  cf.value = cv;
  pf.value = pv;
}

function populate(resetScroll = true) {
  const list = $("list");
  const scroll = list.scrollTop;
  list.replaceChildren();
  state.rows.clear();
  if (!state.events.length) {
    list.append(h("div", { class: "empty" }, state.gameDir ? t("noSounds") : t("pickGame")));
    refreshLabels();
    return;
  }
  const coach = $("coachFilter").value;
  const part = $("catFilter").value;
  const showExt = $("extToggle").checked;
  const q = $("search").value.trim().toLowerCase();
  const order = state.parts;
  const evs = state.events
    .map((e) => ({ ...e, label: evLabel(e) }))
    .filter((e) => (!coach || e.coaches.includes(coach)) && (showExt || !e.is_exterior) && (!part || e.part === part)
      && (!q || e.label.toLowerCase().includes(q) || e.event.toLowerCase().includes(q)))
    .sort((a, b) => {
      const ka = [order.indexOf(a.part), +a.is_exterior, +(a.coaches.length === 1), a.label, a.key];
      const kb = [order.indexOf(b.part), +b.is_exterior, +(b.coaches.length === 1), b.label, b.key];
      for (let i = 0; i < ka.length; i++) if (ka[i] !== kb[i]) return ka[i] < kb[i] ? -1 : 1;
      return 0;
    });
  if (!evs.length) list.append(h("div", { class: "empty" }, t("noMatch")));

  let lastSection = null;
  let group = null;
  for (const ev of evs) {
    const sub = ev.is_exterior ? t("secExt") : ev.coaches.length > 1 ? t("secShared") : t("secUnique");
    const section = ev.part + sub;
    if (section !== lastSection) {
      list.append(h("div", { class: "section" }, partName(ev.part), h("span", { class: "hint" }, "— " + sub)));
      group = h("div", { class: "group" });
      list.append(group);
      lastSection = section;
    }
    const range = h("input", { type: "range", min: DB_MIN, max: DB_MAX, step: 0.5, value: state.settings.per_event[ev.key] || 0 });
    zeroPos(range);
    range.addEventListener("input", () => setEvent(ev.key, parseFloat(range.value)));
    range.addEventListener("dblclick", () => { range.value = 0; setEvent(ev.key, 0); });
    const val = h("div", { class: "val" });
    const zero = h("button", { class: "small ghost", title: t("zeroTip"), onclick: () => { range.value = 0; setEvent(ev.key, 0); } }, "0");
    const base = Math.abs(ev.base_db) >= 0.05
      ? "  · " + t("original", { db: `${ev.base_db >= 0 ? "+" : ""}${Math.round(ev.base_db)}` }) : "";
    const el = h("div", { class: "item" },
      h("div", {},
        h("div", { class: "name" }, ev.label),
        h("div", { class: "meta" }, ev.coaches.map(coachName).join(", ") + base),
        h("div", { class: "ev" }, ev.event)),
      range, val, zero);
    group.append(el);
    state.rows.set(ev.key, { el, range, val });
  }
  refreshLabels();
  list.scrollTop = resetScroll ? 0 : scroll;
}

// ------------------------------------------------------------- config
function saveConfig() {
  call("save_config", { gameDir: state.gameDir, modDir: state.modDir, modPath: state.current.path }).catch(() => {});
}

// ------------------------------------------------------------- mods
async function refreshMods() {
  state.mods = await call("list_mods", { dir: state.modDir }).catch(() => []);
  renderModSelect();
}

function renderModSelect() {
  const sel = $("modSelect");
  sel.replaceChildren();
  for (const m of state.mods) sel.append(h("option", { value: m.path }, m.name));
  if (state.current.path && !state.mods.some((m) => m.path === state.current.path)) state.current.path = null;
  sel.append(h("option", { value: NEW_MOD }, t("newMod") + (!state.current.path && state.current.name ? ` (${state.current.name})` : "")));
  sel.value = state.current.path || NEW_MOD;
  $("modFileHint").textContent = state.current.path ? basename(state.current.path) : t("notSaved");
  $("btnDeleteMod").disabled = !state.current.path;
}

async function onModSelect() {
  const v = $("modSelect").value;
  if (v === NEW_MOD) {
    state.current = { path: null, name: "" };
    renderModSelect();
    saveConfig();
    status(t("newModStatus"));
    return;
  }
  const m = state.mods.find((x) => x.path === v);
  state.current = { path: m.path, name: m.name };
  renderModSelect();
  saveConfig();
  const s = await call("read_mod_settings", { path: m.path }).catch(() => null);
  if (s && await confirmBox(t("loadModTitle"), t("loadModAsk", { name: m.name }), t("load"))) {
    applySettings(s, t("loadedMod", { name: m.name }));
    await checkStale(s);
  }
}

async function checkStale(s) {
  const stale = await call("stale_banks", { settings: s }).catch(() => []);
  if (stale.length) await alertBox(t("staleTitle"), t("staleBody") + stale.join("\n"));
}

async function buildDialog() {
  if (!state.gameDir) return;
  const input = h("input", { type: "text", maxlength: 80, spellcheck: "false" });
  input.value = state.current.name || state.defaultModName;
  const fileNote = h("div", { class: "note" });
  const warn = h("div", { class: "warn" });
  const err = h("div", { class: "err" });
  const delBox = h("input", { type: "checkbox", checked: true });
  const delLabel = h("span");
  const delRow = h("label", { class: "check" }, delBox, delLabel);
  const body = h("div", {},
    h("div", { class: "field" }, h("label", {}, t("modName")), input),
    h("div", { class: "note" }, t("modNameNote")),
    fileNote, warn, err, delRow);

  let target = null;
  let seq = 0;
  const update = async () => {
    const my = ++seq;
    const name = input.value.trim();
    warn.textContent = err.textContent = "";
    delRow.style.display = "none";
    if (!name) {
      fileNote.textContent = "";
      err.textContent = t("enterModName");
      $("mOk").disabled = true;
      return;
    }
    let path;
    try {
      // same name as the mod being edited -> keep its file (old mods may have a different file name)
      path = state.current.path && name === state.current.name
        ? state.current.path
        : await call("mod_file_path", { dir: state.modDir, name });
    } catch (e) {
      if (my !== seq) return;
      err.textContent = e.message;
      $("mOk").disabled = true;
      return;
    }
    const fs = await call("file_state", { path });
    if (my !== seq) return;
    target = path;
    fileNote.textContent = t("fileLine", { path });
    const same = state.current.path && state.current.path.toLowerCase() === path.toLowerCase();
    $("mOk").disabled = false;
    if (fs.exists && !fs.tool_mod) {
      err.textContent = t("notToolMod");
      $("mOk").disabled = true;
    } else if (fs.exists && !same) {
      const other = state.mods.find((m) => m.path.toLowerCase() === path.toLowerCase());
      warn.textContent = t("willOverwrite", { other: other ? ` '${other.name}'` : "" });
    }
    if (state.current.path && !same) {
      delLabel.textContent = t("deleteOld", { file: basename(state.current.path) });
      delRow.style.display = "";
    }
  };
  input.addEventListener("input", update);
  input.addEventListener("keydown", (e) => { if (e.key === "Enter" && $("mOk").disabled) e.preventDefault(); });

  const ok = await modal({
    title: t("buildTitle"), body, ok: t("buildOk"),
    onOpen: () => { update(); input.focus(); input.select(); },
  });
  if (!ok || !target) return;
  const name = input.value.trim();
  const removeOld = state.current.path && delRow.style.display !== "none" && delBox.checked ? state.current.path : null;
  await doBuild(name, target, removeOld);
}

async function doBuild(name, outPath, removeOld) {
  status(t("building"));
  let res;
  try {
    res = await call("build_mod", { settings: state.settings, modName: name, outPath, removeOld });
  } catch (e) {
    status(t("buildFailed"));
    return alertBox(t("saveFailed"), e.message);
  }
  state.settings.source_sha1 = res.settings.source_sha1;
  call("save_preset", { name: state.lastSession, settings: state.settings }).catch(() => {});
  if (res.path === null) {
    state.current = { path: null, name };
    await refreshMods();
    saveConfig();
    status(t("noChangeStatus"));
    return alertBox(t("done"), t("noChangeBody"));
  }
  state.current = { path: res.path, name };
  await refreshMods();
  saveConfig();
  status(t("savedStatus", { path: res.path }));
  const body = h("div", {},
    h("p", {}, t("builtBody", { n: res.changed_events, name, path: res.path })),
    h("p", {}, t("enableBody", { name }) + (removeOld ? t("oldDeleted", { file: basename(removeOld) }) : "")));
  if (res.clamped.length) {
    body.append(h("div", { class: "warn" }, t("clampedHead")),
      h("ul", {}, res.clamped.map((c) => h("li", {}, `${c.key.split("#")[1]}: ${fmt(c.wanted)} → ${fmt(c.applied)} dB`))));
  }
  await alertBox(t("done"), body);
}

async function deleteMod() {
  const p = state.current.path;
  if (!p) return;
  if (!(await confirmBox(t("deleteModTitle"), t("deleteModAsk", { name: state.current.name, path: p }), t("delete")))) return;
  try {
    await call("delete_mod", { path: p });
  } catch (e) {
    return alertBox(t("deleteFailed"), e.message);
  }
  state.current = { path: null, name: "" };
  await refreshMods();
  saveConfig();
  status(t("modDeleted"));
}

// ------------------------------------------------------------- presets
async function refreshPresets(select) {
  const names = await call("list_presets").catch(() => []);
  const sel = $("presetSelect");
  const prev = select ?? sel.value;
  sel.replaceChildren(h("option", { value: "" }, names.length ? t("pickPreset") : t("noPresets")),
    ...names.map((n) => h("option", { value: n }, presetLabel(n))));
  sel.value = names.includes(prev) ? prev : "";
  return names;
}

async function loadPresetUi() {
  const name = $("presetSelect").value;
  if (!name) return status(t("pickPresetToLoad"));
  try {
    applySettings(await call("load_preset", { name }), t("presetLoaded", { name: presetLabel(name) }));
  } catch (e) {
    alertBox(t("loadFailed"), t("presetLoadFail", { name: presetLabel(name), e: e.message }));
  }
}

async function savePresetUi() {
  const names = await refreshPresets();
  const sel = $("presetSelect").value;
  const input = h("input", { type: "text", maxlength: 80, spellcheck: "false", placeholder: t("presetPlaceholder") });
  input.value = sel && sel !== state.lastSession ? sel : "";
  const warn = h("div", { class: "warn" });
  const reserved = (n) => n === state.lastSession || n === STR.ko.lastSession || n === STR.en.lastSession;
  const update = () => {
    const n = input.value.trim();
    warn.textContent = names.includes(n) && !reserved(n) ? t("presetExists", { name: n }) : "";
    $("mOk").disabled = !n || reserved(n);
  };
  input.addEventListener("input", update);
  const body = h("div", {},
    h("div", { class: "field" }, h("label", {}, t("presetName")), input),
    h("div", { class: "note" }, t("presetNote")),
    warn);
  const ok = await modal({ title: t("savePresetTitle"), body, ok: t("save"), onOpen: () => { update(); input.focus(); input.select(); } });
  const name = input.value.trim();
  if (!ok || !name) return;
  try {
    await call("save_preset", { name, settings: state.settings });
  } catch (e) {
    return alertBox(t("saveFailed"), e.message);
  }
  await refreshPresets(name);
  status(t("presetSaved", { name }));
}

async function deletePresetUi() {
  const name = $("presetSelect").value;
  if (!name) return status(t("pickPresetToDelete"));
  if (!(await confirmBox(t("deletePresetTitle"), t("deletePresetAsk", { name: presetLabel(name) }), t("delete")))) return;
  try {
    await call("delete_preset", { name });
  } catch (e) {
    return alertBox(t("deleteFailed"), e.message);
  }
  await refreshPresets("");
  status(t("presetDeleted", { name: presetLabel(name) }));
}

async function loadFromFile() {
  const p = await dialog.open({
    title: t("loadFileTitle"),
    defaultPath: state.modDir || undefined,
    filters: [{ name: t("fileFilter"), extensions: ["scs", "json"] }, { name: t("allFiles"), extensions: ["*"] }],
  });
  if (!p) return;
  const s = await call("load_settings_file", { path: p }).catch(() => null);
  if (!s) return alertBox(t("loadFailed"), t("loadFileFail"));
  applySettings(s, t("loadedFile", { file: basename(p) }));
}

// ------------------------------------------------------------- game
async function loadGame(path) {
  state.events = [];
  state.gameDir = null;
  $("gameDir").textContent = path || "—";
  if (!path) {
    populate();
    return status(t("gameNotFound"));
  }
  status(t("readingGame"));
  let info;
  try {
    info = await call("load_game", { path });
  } catch (e) {
    populate();
    status(t("gameReadFail"));
    return alertBox(t("error"), e.message);
  }
  state.gameDir = info.game_dir;
  state.coaches = info.coaches;
  state.parts = info.parts;
  state.events = info.events;
  state.skipped = info.skipped;
  renderFilters();
  saveConfig();
  return info;
}

async function startup() {
  const init = await call("init");
  state.labels = await call("labels");
  state.modDir = init.mod_dir;
  state.defaultModName = init.default_mod_name;
  state.lastSession = init.last_session;
  // first run: follow the system language and remember it
  const lang = init.lang || ((navigator.language || "").toLowerCase().startsWith("ko") ? "ko" : "en");
  await setLanguage(lang, !init.lang);
  $("modDir").textContent = state.modDir;

  const info = await loadGame(init.game_dir);
  await refreshMods();
  const cur = state.mods.find((m) => m.path === init.mod_path) || state.mods[0];
  state.current = cur ? { path: cur.path, name: cur.name } : { path: null, name: "" };
  renderModSelect();
  saveConfig();
  if (!info) return;

  let extra = "";
  const presets = await call("list_presets").catch(() => []);
  let s = null;
  if (presets.includes(state.lastSession)) {
    s = await call("load_preset", { name: state.lastSession }).catch(() => null);
    if (s) extra = t("lastLoaded");
  }
  if (!s && cur) {
    s = await call("read_mod_settings", { path: cur.path }).catch(() => null);
    if (s) extra = t("modLoaded", { name: cur.name });
  }
  state.settings = s ? { ...emptySettings(), ...s } : emptySettings();
  $("globalSlider").value = state.settings.global_db;
  populate();
  status(summaryText(extra));
  if (cur) {
    const ms = await call("read_mod_settings", { path: cur.path }).catch(() => null);
    if (ms) await checkStale(ms);
  }
}

// ------------------------------------------------------------- wiring
$("langSelect").onchange = (e) => setLanguage(e.target.value);
$("btnGame").onclick = async () => {
  const p = await dialog.open({ directory: true, title: t("pickGameDir") });
  if (!p) return;
  const info = await loadGame(p);
  if (info) {
    populate();
    status(summaryText());
  }
};
$("btnModDir").onclick = async () => {
  const p = await dialog.open({ directory: true, title: t("pickModDir"), defaultPath: state.modDir });
  if (!p) return;
  state.modDir = p;
  $("modDir").textContent = p;
  state.current = { path: null, name: "" };
  await refreshMods();
  if (state.mods[0]) state.current = { path: state.mods[0].path, name: state.mods[0].name };
  renderModSelect();
  saveConfig();
  status(t("modDirStatus", { path: p, n: state.mods.length }));
};
$("modSelect").onchange = onModSelect;
$("btnDeleteMod").onclick = deleteMod;
$("btnLoadPreset").onclick = loadPresetUi;
$("btnSavePreset").onclick = savePresetUi;
$("btnDeletePreset").onclick = deletePresetUi;
$("btnLoadFile").onclick = loadFromFile;
$("presetSelect").addEventListener("focus", () => refreshPresets());
$("coachFilter").onchange = () => populate();
$("catFilter").onchange = () => populate();
$("extToggle").onchange = () => populate();
$("search").oninput = () => populate();
zeroPos($("globalSlider"));
$("globalSlider").oninput = (e) => setGlobal(parseFloat(e.target.value));
$("globalSlider").ondblclick = () => setGlobal(0);
$("btnGlobalZero").onclick = () => setGlobal(0);
$("btnBuild").onclick = buildDialog;
$("btnResetAll").onclick = () => {
  state.settings.per_event = {};
  state.settings.global_db = 0;
  $("globalSlider").value = 0;
  for (const row of state.rows.values()) row.range.value = 0;
  refreshLabels();
  status(t("resetStatus"));
};
document.addEventListener("contextmenu", (e) => { if (!e.target.closest("input")) e.preventDefault(); });

startup().catch((e) => {
  status(t("initFail") + e.message);
  alertBox(t("error"), e.message);
});
