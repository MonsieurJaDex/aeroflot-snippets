import { apiRequest } from "./session.js";
import { vehicleLabel } from "./catalog.js";

const names = new Map();

function asPoint(point) {
  if (!Array.isArray(point) || point.length < 2) return null;
  return [Number(point[0]), Number(point[1])];
}

export async function engineerName(id) {
  const key = String(id);
  if (names.has(key)) return names.get(key);
  try {
    const data = await apiRequest(`/api/auth/getEngineerName/${key}`, { auth: false });
    const name = data?.name || `Инженер ${key.slice(0, 8)}`;
    names.set(key, name);
    return name;
  } catch {
    const fallback = `Инженер ${key.slice(0, 8)}`;
    names.set(key, fallback);
    return fallback;
  }
}

export async function loadStaff() {
  let engineers = [];
  try {
    engineers = await apiRequest("/api/geo/getEngineersPositions");
  } catch (error) {
    console.warn("[map] не удалось загрузить список сотрудников", error);
    return { plotted: [], missing: [] };
  }
  if (!Array.isArray(engineers)) return { plotted: [], missing: [] };

  const plotted = [];
  const missing = [];
  await Promise.all(engineers.map(async (entry) => {
    const id = String(entry.id);
    const name = await engineerName(id);
    const point = asPoint(entry.position);
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
    const data = await apiRequest("/api/aircraft");
    const list = Array.isArray(data?.aircraft) ? data.aircraft : [];
    return list
      .map((item) => ({ id: String(item.id), point: asPoint(item.point) }))
      .filter((item) => item.point);
  } catch (error) {
    console.warn("[map] не удалось загрузить воздушные суда", error);
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

export async function postTrackPoint({ obj, lat, lon, t = new Date().toISOString(), acc = null, spd = null }) {
  return apiRequest("/api/v1/track", {
    method: "POST",
    auth: false,
    body: { obj, lat, lon, t, acc, spd },
  });
}

export async function loadBusyEngineerIds() {
  try {
    const data = await apiRequest("/api/geo/activeEngineers");
    const ids = data?.active_engineers || [];
    return new Set(ids.map(String));
  } catch (error) {
    console.warn("[map] не удалось загрузить занятых инженеров", error);
    return new Set();
  }
}

export async function loadTransport() {
  try {
    const list = await apiRequest("/api/geo/getTransportPositions");
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
  } catch (error) {
    console.warn("[map] не удалось загрузить спецтранспорт", error);
    return [];
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

export async function downloadJournalCsv() {
  // BOM чтобы Excel открыл кириллицу; имя с датой для сдачи жюри.
  const csv = await apiRequest("/api/journal.csv");
  const text = typeof csv === "string" ? csv : "";
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
  const body = { point };
  if (id) body.id = id;
  return apiRequest("/api/aircraft/arrive", {
    method: "POST",
    body,
  });
}

export async function updateEngineerPosition(id, batch) {
  return apiRequest("/api/geo/updateEngineerPosition", {
    method: "POST",
    auth: false,
    body: { id, batch },
  });
}

export async function updateTransportPosition(id, vehicleType, newPoint) {
  return apiRequest("/api/geo/updateTransportPosition", {
    method: "POST",
    auth: false,
    body: { id, vehicle_type: vehicleType, new_point: newPoint },
  });
}

export async function getRoute(startPoint, endPoint) {
  return apiRequest("/api/getRoute", {
    method: "POST",
    body: { start_point: startPoint, end_point: endPoint },
  });
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
  return raw || "Неизвестная ошибка назначения";
}
