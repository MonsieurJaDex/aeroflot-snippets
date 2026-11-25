// Ответы на ручки, которых нет у текущего сервера. Живой 200 не подменяется.
import { ISSUE_REQUIRED_VEHICLE } from "./catalog.js";
import { activateMap, activeMapId, MAP_FILES } from "./mapSource.js";

const KEY = "oto-scenario-store";

const GHOSTS = [
  { id: "00000000-0000-4000-8000-0000000000a1", name: "Пётр · сценарий" },
  { id: "00000000-0000-4000-8000-0000000000a2", name: "Анна · сценарий" },
  { id: "00000000-0000-4000-8000-0000000000a3", name: "Олег · сценарий" },
];

export function scenarioOn() {
  try {
    // Полевой прогон без моков. Включить обратно: localStorage oto-scenario=1.
    return localStorage.getItem("oto-scenario") === "1";
  } catch {
    return false;
  }
}

export function scenarioEngineerName(id) {
  const hit = GHOSTS.find((row) => row.id === String(id));
  return hit ? hit.name : "";
}

function emptyStore() {
  return { journal: [], alerts: [], movements: [], aog: [], engineers: [], tasks: {}, seeded: false };
}

function load() {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) || "null");
    if (!raw || typeof raw !== "object") return seed(emptyStore());
    raw.journal ||= [];
    raw.alerts ||= [];
    raw.movements ||= [];
    raw.aog ||= [];
    raw.engineers ||= [];
    raw.tasks ||= {};
    if (!raw.seeded) return seed(raw);
    return raw;
  } catch {
    return seed(emptyStore());
  }
}

function save(store) {
  localStorage.setItem(KEY, JSON.stringify(store));
}

function nowIso() {
  return new Date().toISOString();
}

function uid() {
  if (typeof crypto !== "undefined" && crypto.randomUUID) return crypto.randomUUID();
  return `scenario-${Date.now()}`;
}

function pushJournal(store, kind, params, extra = {}) {
  store.journal.push({
    at: nowIso(),
    kind,
    actor: extra.actor || "",
    task_id: extra.task_id || "",
    duration_ms: extra.duration_ms ?? "",
    params: String(params || "").replace(/[\n,]/g, " "),
  });
}

function seed(store) {
  const at = nowIso();
  store.journal = [
    { at, kind: "request_fixed", actor: "mcc", task_id: "", duration_ms: "", params: "замечание экипажа зафиксировано" },
    { at, kind: "candidates_built", actor: "dispatcher", task_id: "", duration_ms: "18", params: "count=3 допуск ETA занятость" },
    { at, kind: "sla_risk", actor: "dispatcher", task_id: "scenario-sla", duration_ms: "", params: "не успевает запас 120с" },
    { at, kind: "track_rejected", actor: "gps", task_id: "", duration_ms: "", params: "рассогласование точка вне полигона" },
  ];
  store.alerts = [{
    request_id: "scenario-sla",
    message: "Не успевает за 15 минут. Ближайший свободный успевает с запасом 2 мин.",
    alternative_engineer_id: GHOSTS[0].id,
    alternative_margin_seconds: 120,
  }];
  store.movements = [{
    id: uid(),
    aircraft_id: "demo-stand",
    map_id: "real_map",
    point: [40, 30],
    arrived_at: at,
    departed_at: null,
  }];
  store.aog = [{ id: "demo-stand", point: [40, 30] }];
  store.seeded = true;
  save(store);
  return store;
}

function csv(store) {
  const lines = ["at,kind,actor,task_id,duration_ms,params"];
  for (const row of store.journal) {
    lines.push([row.at, row.kind, row.actor, row.task_id, row.duration_ms, row.params].join(","));
  }
  return `${lines.join("\n")}\n`;
}

function mapsBody() {
  const active = activeMapId();
  return {
    scenario: true,
    active_id: active,
    maps: Object.entries(MAP_FILES).map(([id, meta]) => ({
      id,
      name: meta.name,
      width: meta.width,
      height: meta.height,
      active: id === active,
    })),
  };
}

function rememberEngineers(store, data) {
  if (!Array.isArray(data)) return;
  const next = [];
  for (const entry of data) {
    const tuple = Array.isArray(entry);
    const id = String(tuple ? entry[0] : entry?.id || "");
    const point = tuple ? entry[1] : (entry?.point || entry?.position);
    if (!id || !Array.isArray(point)) continue;
    next.push({ id, point: [Number(point[0]), Number(point[1])] });
  }
  if (next.length) store.engineers = next;
}

function taskOf(store, session) {
  const id = session?.userId || "engineer";
  store.tasks[id] ||= {};
  return store.tasks[id];
}

const STATUS_RANK = {
  crew_remark: 0,
  unassigned: 0,
  assigned: 1,
  accepted: 2,
  in_progress: 3,
  waiting_part: 3,
  done: 4,
  completed: 4,
  rejected: 4,
};

function preferStatus(server, overlay) {
  const fromServer = typeof server === "string" ? server : "";
  const fromOverlay = typeof overlay === "string" ? overlay : "";
  if (!fromServer) return fromOverlay || undefined;
  if (!fromOverlay) return fromServer;
  return (STATUS_RANK[fromServer] ?? 0) >= (STATUS_RANK[fromOverlay] ?? 0) ? fromServer : fromOverlay;
}

function rememberTask(store, session, data) {
  if (!data || typeof data !== "object" || !data.id) return data;
  const key = session?.userId || "engineer";
  let task = store.tasks[key] || {};
  if (task.id && String(task.id) !== String(data.id)) task = {};
  task.id = String(data.id);
  if (Array.isArray(data.route) && !task.routeLocked) task.route = data.route;
  if (Array.isArray(data.plane_point)) task.plane_point = data.plane_point;
  if (data.eta_seconds != null && !task.routeLocked) task.eta_seconds = data.eta_seconds;
  store.tasks[key] = task;
  if (task.cleared) return null;
  return {
    ...data,
    status: preferStatus(data.status, task.status),
    route: task.route || data.route,
    eta_seconds: task.eta_seconds ?? data.eta_seconds,
    tools_picked: task.tools_picked ?? data.tools_picked ?? false,
    mel_category: task.mel_category ?? data.mel_category ?? null,
    ata_chapter: task.ata_chapter ?? data.ata_chapter ?? null,
    arrived: Boolean(task.arrived ?? data.arrived),
  };
}

function classifyText(text) {
  const raw = String(text || "").toLowerCase();
  const clues = [
    [/топлив|теч|гермет|гидрав/, "integrity_inspector", 0.74],
    [/масл/, "integrity_inspector", 0.7],
    [/двиг|вибра|стружк|тяг/, "engine_technician", 0.76],
    [/авион|рнс|связ|гиро|рлс|инс|курс/, "avionics_engineer", 0.73],
    [/ламп|индика|разъ|ремен/, "crew_remarks_handler", 0.68],
    [/шин|протектор|давлен/, "fueling_crew", 0.66],
  ];
  for (const [re, engineer_type, confidence] of clues) {
    if (re.test(raw)) return { engineer_type, confidence, scenario: true };
  }
  return { engineer_type: "aviation_technician", confidence: 0.42, scenario: true };
}

function decorateAssign(store, body, data) {
  const chosen = String(data.engineer_uuid || "");
  const eta = Number(data.time ?? data.eta_seconds ?? 0);
  const sla = Boolean(data.time_limit_exceeded ?? data.sla_exceeded) || eta > 900;
  const pool = store.engineers.filter((row) => row.id !== chosen);
  const others = pool.length ? pool : GHOSTS.filter((row) => row.id !== chosen);
  const vehicle = Boolean(ISSUE_REQUIRED_VEHICLE[body?.issue]);
  const candidates = [
    {
      engineer_id: chosen,
      eligible: true,
      reason: sla ? "допуск совпал, ETA больше 15 минут" : "допуск совпал, свободен, меньший ETA",
      eta_seconds: eta,
      uses_vehicle: vehicle,
    },
  ];
  if (others[0]) {
    candidates.push({
      engineer_id: others[0].id,
      eligible: true,
      reason: vehicle ? "допуск есть, пешком дольше, чем со спецтранспортом" : "допуск есть, идёт дольше",
      eta_seconds: eta + 180,
      uses_vehicle: false,
    });
  }
  if (others[1]) {
    candidates.push({
      engineer_id: others[1].id,
      eligible: false,
      reason: "уже начал разборку, снятие только старшим смены",
      eta_seconds: Math.max(60, eta - 30),
      uses_vehicle: false,
    });
  }
  if (others[2]) {
    candidates.push({
      engineer_id: others[2].id,
      eligible: false,
      reason: "глава ATA вне допуска",
      eta_seconds: eta + 240,
      uses_vehicle: false,
    });
  }
  pushJournal(store, "candidates_built", `count=${candidates.length}`, { actor: "dispatcher", duration_ms: "18" });
  pushJournal(store, "engineer_assigned", body?.issue || "", { actor: chosen, task_id: data.task_id || "" });
  pushJournal(store, "route_built", `клеток ${Array.isArray(data.route) ? data.route.length : 0}`, { actor: chosen });
  if (sla) {
    const alt = others.find((row) => row.id !== chosen) || GHOSTS[0];
    store.alerts.unshift({
      request_id: data.task_id || uid(),
      message: "Не успевает за 15 минут. Ближайший свободный успевает с запасом 2 мин.",
      alternative_engineer_id: alt.id,
      alternative_margin_seconds: 120,
    });
    pushJournal(store, "sla_risk", "ETA больше норматива", { actor: "dispatcher" });
  }
  if (store.alerts.length > 8) store.alerts.length = 8;
  return { ...data, candidates, candidate_ms: data.candidate_ms ?? 18, scenario: true };
}

function shortenRoute(route) {
  if (!Array.isArray(route) || route.length < 3) return Array.isArray(route) ? route : [];
  return route.slice(1);
}

export function answer404(path, method, body, session) {
  const store = load();
  const verb = String(method || "GET").toUpperCase();
  const task = () => taskOf(store, session);

  if (path === "/api/alerts" && verb === "GET") return { alerts: store.alerts };
  if (path === "/api/journal.csv" && verb === "GET") return csv(store);
  if (path === "/api/maps" && verb === "GET") return mapsBody();
  if (path === "/api/maps/active" && verb === "GET") {
    const id = activeMapId();
    return { id, ...MAP_FILES[id], scenario: true };
  }
  if (path === "/api/maps/active" && verb === "PUT") {
    const meta = activateMap(body?.id);
    if (!meta) return undefined;
    pushJournal(store, "map_selected", meta.id, { actor: "dispatcher" });
    save(store);
    return { ...meta, reload: true, scenario: true };
  }
  if (path.startsWith("/api/aircraft/movements") && verb === "GET") {
    return { movements: store.movements };
  }
  if (path === "/api/tasks/aog" && verb === "GET") {
    return { aircraft: store.aog, plane_uuids: store.aog.map((row) => row.id) };
  }
  if (path === "/api/requests" && verb === "POST") {
    const id = uid();
    pushJournal(store, "request_fixed", body?.text || "", { actor: "mcc", task_id: id });
    save(store);
    return { id, status: "new", scenario: true };
  }
  const queue = path.match(/^\/api\/requests\/([^/]+)\/queue$/);
  if (queue && verb === "POST") {
    pushJournal(store, "status_changed", "unassigned", { actor: "dispatcher", task_id: decodeURIComponent(queue[1]) });
    save(store);
    return { id: decodeURIComponent(queue[1]), status: "unassigned", scenario: true };
  }
  const reassign = path.match(/^\/api\/requests\/([^/]+)\/reassign$/);
  if (reassign && verb === "POST") {
    pushJournal(store, "request_reassigned", body?.reason || "", {
      actor: body?.engineer_id || "",
      task_id: decodeURIComponent(reassign[1]),
    });
    save(store);
    return { id: decodeURIComponent(reassign[1]), status: "assigned", scenario: true };
  }
  if (path === "/api/classify" && verb === "POST") return classifyText(body?.text);
  if (path === "/api/classify/train" && verb === "POST") {
    pushJournal(store, "classifier_trained", "documents=24", { actor: "dispatcher" });
    save(store);
    return { documents: 24, trained_at: nowIso(), accuracy: 0.81, scenario: true };
  }
  if (path === "/api/defects" && verb === "POST") {
    const id = uid();
    pushJournal(store, "defect_recorded", `${body?.ata_chapter || ""} ${body?.text || ""}`, { actor: "dispatcher", task_id: id });
    save(store);
    return { id, text: body?.text || "", ata_chapter: body?.ata_chapter || "", scenario: true };
  }
  // Полевой трек не выдумываем.
  if (path === "/api/aircraft/arrive" && verb === "POST") {
    const id = body?.id || uid();
    const row = {
      id: uid(),
      aircraft_id: id,
      map_id: activeMapId(),
      point: body?.point || null,
      arrived_at: nowIso(),
      departed_at: null,
    };
    store.movements.unshift(row);
    pushJournal(store, "aircraft_arrived", id, { actor: "dispatcher" });
    save(store);
    return row;
  }
  if (path === "/api/aircraft/depart" && verb === "POST") {
    const id = String(body?.id || "");
    const row = store.movements.find((item) => String(item.aircraft_id) === id && !item.departed_at);
    if (row) row.departed_at = nowIso();
    pushJournal(store, "aircraft_departed", id, { actor: "dispatcher" });
    save(store);
    return row || { id: uid(), aircraft_id: id, departed_at: nowIso(), scenario: true };
  }
  if (path === "/api/tasks/current/reject" && verb === "POST") {
    const current = task();
    current.cleared = true;
    current.status = "unassigned";
    pushJournal(store, "status_changed", "rejected", { actor: session?.userId || "", task_id: current.id || "" });
    save(store);
    return { status: "unassigned", scenario: true };
  }
  if (path === "/api/tasks/current/tools" && verb === "POST") {
    const current = task();
    current.tools_picked = true;
    current.status = "in_progress";
    pushJournal(store, "status_changed", "tools", { actor: session?.userId || "", task_id: current.id || "" });
    save(store);
    return { tools_picked: true, status: "in_progress", scenario: true };
  }
  if (path === "/api/tasks/current/waiting-part" && verb === "POST") {
    const current = task();
    current.cleared = true;
    current.status = "waiting_part";
    pushJournal(store, "status_changed", "waiting_part", { actor: session?.userId || "", task_id: current.id || "" });
    save(store);
    return { status: "waiting_part", scenario: true };
  }
  if (path === "/api/tasks/current/mel" && verb === "POST") {
    const current = task();
    current.mel_category = body?.category || "A";
    current.status = current.status || "in_progress";
    pushJournal(store, "status_changed", `MEL ${current.mel_category}`, { actor: session?.userId || "", task_id: current.id || "" });
    save(store);
    return { mel_category: current.mel_category, scenario: true };
  }
  if (path === "/api/tasks/current/complete" && verb === "POST") {
    const current = task();
    current.cleared = true;
    current.status = "done";
    pushJournal(store, "status_changed", "done", { actor: session?.userId || "", task_id: current.id || "" });
    save(store);
    return { status: "done", scenario: true };
  }
  if (path === "/api/tasks/current/aog" && (verb === "PUT" || verb === "POST")) {
    const on = Boolean(body?.aog);
    const current = task();
    const plane = current.plane_point ? "scenario-plane" : "demo-stand";
    if (on) {
      if (!store.aog.some((row) => row.id === plane)) store.aog.push({ id: plane, point: current.plane_point || [40, 30] });
    } else {
      store.aog = store.aog.filter((row) => row.id !== plane);
    }
    pushJournal(store, "status_changed", on ? "aog" : "aog_off", { actor: session?.userId || "", task_id: current.id || "" });
    save(store);
    return { aog: on, plane_uuid: plane, scenario: true };
  }
  const route = path.match(/^\/api\/tasks\/([^/]+)\/route$/);
  if (route && verb === "POST") {
    const current = task();
    current.routeLocked = true;
    current.route = shortenRoute(current.route);
    const eta = Math.max(30, Number(current.eta_seconds || 120) - 15);
    current.eta_seconds = eta;
    pushJournal(store, "route_recalculated", `eta ${eta}`, {
      actor: session?.userId || "",
      task_id: decodeURIComponent(route[1]),
    });
    save(store);
    return { route: current.route || [], eta_seconds: eta, distance: Math.max(0, (current.route || []).length - 1), scenario: true };
  }
  return undefined;
}

export function afterSuccess(path, method, body, data, session) {
  const store = load();
  const verb = String(method || "GET").toUpperCase();
  if (path === "/api/simulate/get_engineers_positions") {
    rememberEngineers(store, data);
    save(store);
    return data;
  }
  if (path === "/api/assign" && verb === "POST" && data && !Array.isArray(data.candidates)) {
    const next = decorateAssign(store, body, data);
    save(store);
    return next;
  }
  if (path === "/api/tasks/current" && verb === "GET") {
    const next = rememberTask(store, session, data);
    save(store);
    return next;
  }
  if (path === "/api/tasks/current/accept" && verb === "POST") {
    const current = taskOf(store, session);
    if (!current.cleared && current.status !== "done") current.status = "accepted";
    pushJournal(store, "status_changed", "accepted", { actor: session?.userId || "", task_id: current.id || "" });
    save(store);
  }
  return data;
}
