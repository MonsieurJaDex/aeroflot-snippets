import { apiRequest } from "./session.js";
import { vehicleLabel } from "./catalog.js";
import { scenarioEngineerName } from "./scenario.js";

const names = new Map();

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

function statusOf(error) {
  const status = Number(error?.status);
  return Number.isFinite(status) ? status : 0;
}

function asPoint(point) {
  if (!Array.isArray(point) || point.length < 2) return null;
  const x = Number(point[0]);
  const y = Number(point[1]);
  if (!Number.isFinite(x) || !Number.isFinite(y)) return null;
  return [x, y];
}

function cellPair(point) {
  const cell = asPoint(point);
  if (!cell) return null;
  return [Math.trunc(cell[0]), Math.trunc(cell[1])];
}

function positionBatch(batch) {
  if (!Array.isArray(batch)) return [];
  const rows = [];
  for (const sample of batch) {
    if (!Array.isArray(sample)) continue;
    const cell = cellPair(sample[0]) || cellPair(sample);
    if (!cell) continue;
    const stamp = Number(sample[1]);
    rows.push([cell, Number.isFinite(stamp) ? Math.trunc(stamp) : Math.floor(Date.now() / 1000)]);
  }
  return rows;
}

function idSet(ids) {
  const list = Array.isArray(ids) ? ids : [];
  return new Set(list.map((item) => String(item?.id ?? item)).filter(Boolean));
}

function asJson(data) {
  if (typeof data !== "string") return data;
  const text = data.trim();
  if (!text || (text[0] !== "[" && text[0] !== "{")) return data;
  try {
    return JSON.parse(text);
  } catch {
    return data;
  }
}

function pointFromUpdate(batch) {
  const bare = asPoint(batch);
  if (bare) return bare;
  if (!Array.isArray(batch)) return null;
  let best = null;
  let bestT = -Infinity;
  for (const sample of batch) {
    if (!Array.isArray(sample)) continue;
    const point = asPoint(sample[0]) || asPoint(sample);
    if (!point) continue;
    const stamp = Number(sample[1]);
    const t = Number.isFinite(stamp) ? stamp : -Infinity;
    if (!best || t >= bestT) {
      best = point;
      bestT = t;
    }
  }
  return best;
}

// OpenAPI Route = Point[]; Point = [x, y].
export function normalizeRoute(route) {
  if (!Array.isArray(route)) return [];
  const out = [];
  for (const item of route) {
    const point = asPoint(item);
    if (!point) continue;
    out.push(point);
  }
  return out;
}

export async function engineerName(id) {
  const key = String(id);
  if (names.has(key)) return names.get(key);
  const scripted = scenarioEngineerName(key);
  if (scripted) {
    names.set(key, scripted);
    return scripted;
  }
  const label = `Инженер ${key.slice(0, 8)}`;
  const readName = (data) => {
    const parsed = asJson(data);
    return (typeof parsed === "string" ? parsed : parsed?.name) || label;
  };
  try {
    const data = await apiRequest(`/api/auth/getEngineerName/${encodeURIComponent(key)}`, { auth: false });
    const name = readName(data);
    names.set(key, name);
    return name;
  } catch (error) {
    if (statusOf(error) !== 404) {
      names.set(key, label);
      return label;
    }
  }
  try {
    const data = await apiRequest("/api/auth/get_engineer_name", {
      method: "POST",
      auth: false,
      body: { id: key },
    });
    const name = readName(data);
    names.set(key, name);
    return name;
  } catch {
    names.set(key, label);
    return label;
  }
}

// Причина сбоя загрузки списка инженеров в человекочитаемом виде.
// 404 сюда не попадает: отсутствие ручки — обычный пустой список, не ошибка.
function staffLoadFailure(error) {
  const status = statusOf(error);
  const why = status ? ("HTTP " + status) : "сервер не ответил";
  const detail = error?.message ? (", " + String(error.message)) : "";
  return {
    status,
    message: "Список инженеров не загружен (" + why + detail + ") — "
      + "на карте ноль инженеров из-за сбоя запроса, а не потому что их нет.",
  };
}

export async function loadStaff() {
  let engineers = [];
  try {
    const data = asJson(await apiRequest("/api/geo/getEngineersPositions?limit=1000"));
    const list = Array.isArray(data?.engineers) ? data.engineers : [];
    engineers = list.map((entry) => ({
      id: entry?.id,
      position: asPoint(entry?.position),
    }));
  } catch (error) {
    if (statusOf(error) !== 404) {
      console.warn("[map] не удалось загрузить список сотрудников", error);
      return { plotted: [], missing: [], error: staffLoadFailure(error) };
    }
    try {
      engineers = asJson(await apiRequest("/api/simulate/get_engineers_positions"));
    } catch (fallbackError) {
      if (statusOf(fallbackError) !== 404) {
        console.warn("[map] не удалось загрузить список сотрудников", fallbackError);
        return { plotted: [], missing: [], error: staffLoadFailure(fallbackError) };
      }
      engineers = [];
    }
  }
  if (!Array.isArray(engineers)) {
    return {
      plotted: [],
      missing: [],
      error: {
        status: 0,
        message: "Ответ со списком инженеров пришёл в неизвестном формате — на карте ноль инженеров, хотя API ответил.",
      },
    };
  }

  const plotted = [];
  const missing = [];
  await Promise.all(engineers.map(async (entry) => {
    // Живой ответ — [id, point]; объект с position/point/new_point тоже принимаем.
    const tuple = Array.isArray(entry);
    const id = String(tuple ? entry[0] : entry?.id || "");
    if (!id) return;
    const name = await engineerName(id);
    const point = tuple
      ? asPoint(entry[1])
      : asPoint(entry.position) || asPoint(entry.point) || asPoint(entry.new_point);
    if (!point) {
      missing.push({ id, name });
      return;
    }
    plotted.push({ id, name, point, busy: false });
  }));
  plotted.sort((a, b) => a.name.localeCompare(b.name, "ru"));
  return { plotted, missing };
}

export async function loadAircraft() {
  try {
    const data = asJson(await apiRequest("/api/aircraft?limit=1000"));
    const list = Array.isArray(data?.aircraft) ? data.aircraft : [];
    return list
      .map((row) => ({
        id: String(row?.id ?? ""),
        point: asPoint(row?.point),
      }))
      .filter((row) => row.id && row.point);
  } catch (error) {
    if (statusOf(error) === 404 || statusOf(error) === 403) return [];
    console.warn("[map] не удалось загрузить борта", error);
    return [];
  }
}

export async function loadAogIds() {
  try {
    const data = await apiRequest("/api/tasks/aog");
    if (Array.isArray(data?.aircraft)) {
      return new Set(data.aircraft.map((row) => String(row.id ?? row.plane_uuid ?? "")).filter(Boolean));
    }
    const ids = Array.isArray(data?.plane_uuids) ? data.plane_uuids : [];
    return new Set(ids.map(String));
  } catch (error) {
    console.warn("[map] не удалось загрузить AOG", error);
    return new Set();
  }
}

export async function loadAogAircraft() {
  try {
    const data = await apiRequest("/api/tasks/aog");
    if (Array.isArray(data?.aircraft)) {
      return data.aircraft.map((row) => ({
        id: String(row.id ?? row.plane_uuid ?? ""),
        point: asPoint(row.point),
      })).filter((row) => row.id);
    }
    return (Array.isArray(data?.plane_uuids) ? data.plane_uuids : []).map((id) => ({ id: String(id), point: null }));
  } catch (error) {
    console.warn("[aog]", error);
    return [];
  }
}

export async function listMaps() {
  return apiRequest("/api/maps");
}

export async function setActiveMap(id) {
  return apiRequest("/api/maps/active", { method: "PUT", body: { id } });
}

export async function getActiveMap() {
  return apiRequest("/api/maps/active");
}

export async function refreshTaskRoute(id) {
  return apiRequest(`/api/tasks/${encodeURIComponent(id)}/route`, { method: "POST" });
}

export async function postTrackPoint({ obj, lat, lon, t = new Date().toISOString(), acc = null, spd = null }) {
  return apiRequest("/api/v1/track", {
    method: "POST",
    auth: false,
    body: { obj, lat, lon, t, acc, spd },
  });
}

export async function loadBusyEngineerIds() {
  try {
    const data = asJson(await apiRequest("/api/geo/activeEngineers"));
    return idSet(data?.active_engineers);
  } catch (error) {
    if (statusOf(error) !== 404) {
      console.warn("[map] не удалось загрузить занятых инженеров", error);
      return new Set();
    }
    try {
      const data = asJson(await apiRequest("/api/simulate/active_engineers"));
      const ids = Array.isArray(data)
        ? data
        : (data?.active_engineers || data?.engineers || data?.ids || []);
      return idSet(ids);
    } catch (fallbackError) {
      console.warn("[map] не удалось загрузить занятых инженеров", fallbackError);
      return new Set();
    }
  }
}

function mapTransport(list) {
  if (!Array.isArray(list)) return [];
  return list
    .filter((item) => item.vehicle_type !== "Aircraft" && item.vehicle_type !== "aircraft")
    .map((item) => ({
      id: String(item.id),
      vehicle_type: item.vehicle_type,
      label: vehicleLabel(item.vehicle_type),
      point: asPoint(item.point),
    }))
    .filter((item) => item.point);
}

export async function loadTransport() {
  try {
    const data = asJson(await apiRequest("/api/geo/getTransportPositions?limit=1000"));
    return mapTransport(data?.transport);
  } catch (error) {
    if (statusOf(error) !== 404) {
      console.warn("[map] не удалось загрузить спецтранспорт", error);
      return [];
    }
    try {
      const list = asJson(await apiRequest("/api/simulate/get_transport_positions"));
      return mapTransport(list);
    } catch (fallbackError) {
      console.warn("[map] не удалось загрузить спецтранспорт", fallbackError);
      return [];
    }
  }
}

export function findTransportOnRoute(route, fleet) {
  if (!route || route.length < 3 || !fleet.length) return null;
  const middle = route.slice(1, -1);
  for (const vehicle of fleet) {
    const hit = middle.find(([x, y]) => x === vehicle.point[0] && y === vehicle.point[1]);
    if (hit) return vehicle;
  }
  return null;
}

export async function loadAlerts() {
  const data = await apiRequest("/api/alerts");
  return Array.isArray(data?.alerts) ? data.alerts : [];
}

export async function fetchJournalCsv() {
  const csv = await apiRequest("/api/journal.csv");
  return typeof csv === "string" ? csv : "";
}

export async function downloadJournalCsv() {
  // BOM чтобы Excel открыл кириллицу; имя с датой для сдачи жюри.
  const text = await fetchJournalCsv();
  const blob = new Blob(["\uFEFF" + text], { type: "text/csv;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
  link.download = `journal-${stamp}.csv`;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}

export async function reassignRequest(id, engineerId, reason) {
  return apiRequest(`/api/requests/${encodeURIComponent(id)}/reassign`, {
    method: "POST",
    body: { engineer_id: engineerId, reason },
  });
}

export async function queueRequest(id) {
  return apiRequest(`/api/requests/${encodeURIComponent(id)}/queue`, { method: "POST" });
}

export async function createDefect(text, ataChapter) {
  return apiRequest("/api/defects", {
    method: "POST",
    body: { text, ata_chapter: ataChapter },
  });
}

export async function loadMovements(limit = 20) {
  const data = await apiRequest(`/api/aircraft/movements?limit=${limit}`);
  return Array.isArray(data?.movements) ? data.movements : [];
}

export async function departAircraft(id) {
  return apiRequest("/api/aircraft/depart", {
    method: "POST",
    body: { id },
  });
}

export async function arriveAircraft(point, id = null) {
  const cell = cellPair(point);
  return apiRequest("/api/aircraft/arrive", {
    method: "POST",
    body: {
      id: typeof id === "string" && UUID_RE.test(id) ? id : null,
      point: cell || point,
    },
  });
}

export async function updateEngineerPosition(id, batch) {
  const samples = positionBatch(batch);
  if (!samples.length) throw new Error("Нет точки для позиции инженера.");
  try {
    return await apiRequest("/api/geo/updateEngineerPosition", {
      method: "POST",
      body: { id, batch: samples },
    });
  } catch (error) {
    if (statusOf(error) !== 404) throw error;
    const newPoint = pointFromUpdate(samples);
    if (!newPoint) throw new Error("Нет точки для позиции инженера.");
    return apiRequest("/api/simulate/update_engineer_position", {
      method: "POST",
      body: { id, new_point: newPoint },
    });
  }
}

export async function updateTransportPosition(id, vehicleType, newPoint) {
  const body = { id, vehicle_type: vehicleType, new_point: cellPair(newPoint) || newPoint };
  try {
    return await apiRequest("/api/geo/updateTransportPosition", {
      method: "POST",
      body,
    });
  } catch (error) {
    if (statusOf(error) !== 404) throw error;
    return apiRequest("/api/simulate/update_transport_position", {
      method: "POST",
      body,
    });
  }
}

export async function getRoute(startPoint, endPoint) {
  const data = asJson(await apiRequest("/api/getRoute", {
    method: "POST",
    body: { start_point: startPoint, end_point: endPoint },
  }));
  const route = normalizeRoute(Array.isArray(data) ? data : data?.route);
  const distance = !Array.isArray(data) && Number.isFinite(Number(data?.distance))
    ? Number(data.distance)
    : Math.max(0, route.length - 1);
  if (data && typeof data === "object" && !Array.isArray(data)) return { ...data, route, distance };
  return { route, distance };
}

/** Цепочка getRoute через 1+ промежуточных точек (п.6 ТЗ). */
export async function getRouteVia(points) {
  const stops = (Array.isArray(points) ? points : [])
    .map(asPoint)
    .filter(Boolean);
  if (stops.length < 2) return { route: [], distance: 0, legs: 0 };
  if (stops.length === 2) {
    const one = await getRoute(stops[0], stops[1]);
    return { ...one, legs: 1 };
  }
  const route = [];
  let distance = 0;
  for (let i = 0; i < stops.length - 1; i += 1) {
    const leg = await getRoute(stops[i], stops[i + 1]);
    const part = normalizeRoute(leg.route);
    distance += Number.isFinite(Number(leg.distance))
      ? Number(leg.distance)
      : Math.max(0, part.length - 1);
    if (!part.length) continue;
    if (!route.length) route.push(...part);
    else route.push(...part.slice(1));
  }
  return { route, distance, legs: stops.length - 1 };
}

export async function trainClassifier() {
  return apiRequest("/api/classify/train", { method: "POST" });
}

export function assignErrorMessage(err) {
  const raw = String(err?.message || err || "");
  if (/no suitable engineer available/i.test(raw)) return "На данную исправность отсутствует подходящий специалист";
  if (/engineer was not found/i.test(raw)) return "На данную исправность отсутствует подходящий специалист";
  if (/no available transport/i.test(raw)) return "Нет свободного спецтранспорта нужного типа для этой неисправности.";
  if (/aircraft not found/i.test(raw)) return "Самолёт не найден в Redis. Список бортов берётся из /api/aircraft.";
  if (/not implemented|todo/i.test(raw) || (/other/i.test(raw) && /500/.test(raw))) return "Тип «Другое» сервер пока не назначает.";
  if (/only dispatchers/i.test(raw)) return "Назначать может только диспетчер. Перелогиньтесь под ролью Dispatcher.";
  if (/request not found/i.test(raw)) return "Заявка не найдена. Обновите очередь ЦПУ.";
  if (/already assigned|already has an active task/i.test(raw)) return "У инженера уже есть активная задача.";
  if (/invalid refresh|unauthorized|session/i.test(raw) && /401|token/i.test(raw)) {
    return "Сессия истекла. Войдите снова.";
  }
  return raw || "Неизвестная ошибка назначения";
}
