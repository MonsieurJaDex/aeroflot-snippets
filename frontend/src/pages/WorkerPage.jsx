import { useEffect, useRef, useState } from "react";
import { Navigate, useNavigate } from "react-router-dom";
import L from "leaflet";
import NetBanner from "../components/NetBanner.jsx";
import StatusChain from "../components/StatusChain.jsx";
import { engineerTypeLabel, issueLabel } from "../lib/catalog.js";
import { formatEta } from "../lib/format.js";
import { softBeep } from "../lib/beep.js";
import { copyText } from "../lib/copyText.js";
import { enqueueEngineerPoint, flushEngineerOutbox, pendingCount } from "../lib/geoOutbox.js";
import { updateEngineerPosition } from "../lib/live.js";
import { apiRequest, clearSession, getSession, hydrateProfile } from "../lib/session.js";
import { whenVisible } from "../lib/visibility.js";
import { CELL_METERS, pointAt, totalMeters, WALK_MPS } from "../lib/walk.js";
import {
  buildTileGrid,
  centerToLatLng,
  loadApronMap,
  loadTilesetImages,
  renderCanvas,
} from "../lib/apronMap.js";

function planeLabel(task) {
  const point = Array.isArray(task.plane_point) ? `${task.plane_point[0]}, ${task.plane_point[1]}` : "—";
  const id = task.plane_uuid ? task.plane_uuid.slice(0, 8) : "без uuid";
  return `${id} · ${point}`;
}

const CHAIN_BY_STATUS = {
  crew_remark: "замечание",
  unassigned: "не назначен",
  assigned: "назначен",
  accepted: "принял",
  in_progress: "в работе",
  done: "выполнено",
  completed: "выполнено",
};

function chainStep(task, fallback) {
  if (!task) return fallback;
  return CHAIN_BY_STATUS[task.status] || "в работе";
}

function etaMinutes(seconds) {
  const value = Number(seconds);
  if (!Number.isFinite(value) || value < 0) return null;
  return Math.round(value / 60);
}

// datetime-local в RFC3339; пустая строка уходит как null.
function deferUntilIso(value) {
  const raw = String(value || "").trim();
  if (!raw) return null;
  const parsed = new Date(raw);
  if (Number.isNaN(parsed.getTime())) return raw;
  return parsed.toISOString();
}

function sameTask(prev, next) {
  if (!prev || !next) return false;
  return prev.id === next.id
    && prev.issue === next.issue
    && prev.description === next.description
    && prev.plane_uuid === next.plane_uuid
    && prev.status === next.status
    && prev.eta_seconds === next.eta_seconds
    && prev.arrived === next.arrived
    && prev.tools_picked === next.tools_picked
    && prev.mel_category === next.mel_category
    && prev.ata_chapter === next.ata_chapter
    && JSON.stringify(prev.route) === JSON.stringify(next.route)
    && JSON.stringify(prev.plane_point) === JSON.stringify(next.plane_point);
}

export default function WorkerPage() {
  const navigate = useNavigate();
  const initial = getSession();
  const allowed = initial?.role === "Engineer";
  const mapNode = useRef(null);
  const mapApi = useRef({});
  const taskRef = useRef(null);
  const seenTask = useRef(null);
  const [profile, setProfile] = useState(initial);
  const [task, setTask] = useState(null);
  const [aogOn, setAogOn] = useState(false);
  const [notice, setNotice] = useState("Ожидание назначения от диспетчера. Позиция на карте — из геоданных сервера.");
  const [toast, setToast] = useState("");
  const [toastKind, setToastKind] = useState("ok");
  const [distance, setDistance] = useState("НЕТ МАРШРУТА");
  const [selfText, setSelfText] = useState("ВЫ: НЕТ ТОЧКИ");
  const [busyAction, setBusyAction] = useState("");
  const [statusStep, setStatusStep] = useState("не назначен");
  const [melCategory, setMelCategory] = useState("A");
  const [deferUntil, setDeferUntil] = useState("");
  const [outboxCount, setOutboxCount] = useState(() => pendingCount());
  const [etaLeft, setEtaLeft] = useState(null);
  const [pollStale, setPollStale] = useState(false);
  const lastSelfRef = useRef(null);
  const lastPollAt = useRef(Date.now());
  const etaAnchor = useRef({ id: null, at: 0, base: null });

  taskRef.current = task;

  useEffect(() => {
    if (!allowed) return undefined;
    let cancelled = false;
    hydrateProfile()
      .then((next) => {
        if (!cancelled && next) setProfile(next);
      })
      .catch((error) => console.warn("[worker] профиль", error));

    async function loadTask() {
      try {
        const next = await apiRequest("/api/tasks/current");
        if (cancelled) return;
        const normalized = next && next.id ? next : null;
        setTask((prev) => {
          if (!normalized && !prev) return prev;
          if (normalized && sameTask(prev, normalized)) return prev;
          return normalized;
        });
        lastPollAt.current = Date.now();
        setPollStale(false);
        setNotice(normalized
          ? normalized.status === "assigned"
            ? "Назначение получено. Примите или отклоните задачу."
            : "Задача активна. На карте — ваша точка и маршрут. Завершите, когда работы закончены."
          : "Ожидание назначения от диспетчера. Позиция на карте — из геоданных сервера.");
      } catch (error) {
        if (cancelled) return;
        if (error.status === 404) {
          lastPollAt.current = Date.now();
          setPollStale(false);
          setTask(null);
          setNotice("Ожидание назначения от диспетчера. Позиция на карте — из геоданных сервера.");
          return;
        }
        setNotice(`Не удалось получить назначение: ${error.message}`);
      }
    }

    async function refreshSelf() {
      const current = getSession();
      if (!current?.userId) return;
      try {
        const engineers = await apiRequest("/api/geo/getEngineersPositions");
        if (cancelled || !Array.isArray(engineers)) return;
        const mine = engineers.find((entry) => String(entry.id) === String(current.userId));
        if (!mine || !Array.isArray(mine.position)) return;
        // Пока идёт ход по маршруту, точку не дёргаем с Redis (там часто уже стоим у ВС).
        lastSelfRef.current = mine.position;
        mapApi.current.showSelfPoint?.(mine.position, { fromServer: true });
        setNotice((prev) => (prev.startsWith("Нет связи")
          ? "Связь восстановлена. Позиция снова с сервера."
          : prev));
      } catch (error) {
        console.warn("[worker] своя позиция", error);
        if (!cancelled) setNotice("Нет связи с геосервером. Показана последняя известная позиция.");
      }
    }

    async function flushOutbox() {
      const current = getSession();
      if (!current?.userId) return;
      try {
        const sent = await flushEngineerOutbox(async (row) => {
          await updateEngineerPosition(row.id, [[row.point, row.t]]);
        });
        setOutboxCount(pendingCount());
        if (sent) flash(`Дослано геоточек: ${sent}`);
      } catch (error) {
        console.warn("[worker] outbox", error);
      }
    }

    loadTask();
    refreshSelf();
    async function pushLiveGeo() {
      const current = getSession();
      const point = lastSelfRef.current;
      if (!current?.userId || !Array.isArray(point)) return;
      const t = Math.floor(Date.now() / 1000);
      try {
        await updateEngineerPosition(current.userId, [[point, t]]);
        setOutboxCount(pendingCount());
      } catch {
        enqueueEngineerPoint(current.userId, point, t);
        setOutboxCount(pendingCount());
      }
    }

    const staleTimer = window.setInterval(() => {
      if (!cancelled) setPollStale(Date.now() - lastPollAt.current > 20000);
    }, 4000);
    const stopPoll = whenVisible(() => {
      if (cancelled) return;
      loadTask();
      refreshSelf();
      flushOutbox();
      pushLiveGeo();
    }, 5000);
    const onOnline = () => flushOutbox();
    const onLeave = (event) => {
      if (!pendingCount()) return;
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("online", onOnline);
    window.addEventListener("beforeunload", onLeave);
    return () => {
      cancelled = true;
      stopPoll();
      clearInterval(staleTimer);
      window.removeEventListener("online", onOnline);
      window.removeEventListener("beforeunload", onLeave);
    };
  }, [allowed]);

  useEffect(() => {
    const id = task?.id || null;
    if (id && id !== seenTask.current) {
      flash("Новое назначение получено");
      setAogOn(false);
      setStatusStep("не назначен");
      setDeferUntil("");
      softBeep(740, 220);
      try { navigator.vibrate?.(140); } catch { /* телефон */ }
      try {
        if (typeof Notification !== "undefined" && Notification.permission === "granted") {
          new Notification("ОТО · новое назначение", { body: "Примите или отклоните задачу.", tag: "oto-task" });
        } else if (typeof Notification !== "undefined" && Notification.permission === "default") {
          Notification.requestPermission().catch(() => {});
        }
      } catch { /* desktop notify */ }
    }
    seenTask.current = id;
  }, [task?.id]);

  useEffect(() => {
    const category = task?.mel_category;
    if (category === "A" || category === "B" || category === "C" || category === "D") {
      setMelCategory(category);
    } else {
      setMelCategory("A");
    }
  }, [task?.id, task?.mel_category]);

  useEffect(() => {
    if (!toast) return undefined;
    const timer = window.setTimeout(() => {
      setToast("");
      setToastKind("ok");
    }, 4500);
    return () => clearTimeout(timer);
  }, [toast]);

  function flash(text, kind = "ok") {
    setToastKind(kind);
    setToast(text);
  }

  useEffect(() => {
    document.title = task
      ? `ОТО · задача · ETA ${formatEta(etaLeft) || "…"}`
      : "ОТО · инженер свободен";
    return () => { document.title = "Аэрофлот ОТО"; };
  }, [task, etaLeft]);

  // Экран не гаснет, пока есть активная задача на телефоне инженера.
  useEffect(() => {
    if (!task) return undefined;
    let lock = null;
    async function grab() {
      try {
        lock = await navigator.wakeLock?.request("screen");
      } catch { /* не везде есть */ }
    }
    grab();
    const onVis = () => {
      if (document.visibilityState === "visible") grab();
    };
    document.addEventListener("visibilitychange", onVis);
    return () => {
      document.removeEventListener("visibilitychange", onVis);
      lock?.release?.().catch(() => {});
    };
  }, [task?.id]);

  useEffect(() => {
    if (!allowed) return undefined;
    function onKey(event) {
      if (event.target && /^(INPUT|TEXTAREA|SELECT)$/.test(event.target.tagName)) return;
      if (event.key === "a" || event.key === "A" || event.key === "ф" || event.key === "Ф") {
        if (taskRef.current?.status === "assigned") {
          event.preventDefault();
          document.getElementById("accept-task")?.click();
        }
      }
      if (event.key === "r" || event.key === "R" || event.key === "к" || event.key === "К") {
        if (taskRef.current?.status === "assigned") {
          event.preventDefault();
          document.getElementById("reject-task")?.click();
        }
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [allowed]);

  useEffect(() => {
    if (!task || task.eta_seconds == null) {
      setEtaLeft(null);
      etaAnchor.current = { id: null, at: 0, base: null };
      return undefined;
    }
    if (etaAnchor.current.id !== task.id || etaAnchor.current.base !== task.eta_seconds) {
      etaAnchor.current = { id: task.id, at: Date.now(), base: Number(task.eta_seconds) };
    }
    const tick = () => {
      const { at, base } = etaAnchor.current;
      if (base == null) return;
      const left = Math.max(0, base - (Date.now() - at) / 1000);
      setEtaLeft(left);
    };
    tick();
    const timer = window.setInterval(tick, 1000);
    return () => clearInterval(timer);
  }, [task]);

  useEffect(() => {
    if (!allowed) return undefined;
    let cancelled = false;
    const bag = { map: null, frame: 0, cleanupResize: null };
    let walk = null;
    let selfMarker = null;
    let selfPoint = null;
    let tmj = null;

    function mapIcon(className) {
      return L.divIcon({ className, html: "", iconSize: [18, 18], iconAnchor: [9, 9] });
    }

    function stopWalk({ keepSelf = true } = {}) {
      if (bag.frame) cancelAnimationFrame(bag.frame);
      bag.frame = 0;
      if (walk && bag.map) {
        if (walk.line) bag.map.removeLayer(walk.line);
        if (walk.dest) bag.map.removeLayer(walk.dest);
      }
      walk = null;
      if (!keepSelf && selfMarker && bag.map) {
        bag.map.removeLayer(selfMarker);
        selfMarker = null;
      }
    }

    function focus() {
      if (!bag.map || !tmj) return;
      const latlngs = [];
      if (walk?.line) latlngs.push(...walk.line.getLatLngs());
      if (walk?.dest) latlngs.push(walk.dest.getLatLng());
      if (selfMarker) latlngs.push(selfMarker.getLatLng());
      if (latlngs.length >= 2) {
        bag.map.fitBounds(L.latLngBounds(latlngs), { padding: [36, 36], maxZoom: 2 });
        return;
      }
      if (selfMarker) {
        bag.map.setView(selfMarker.getLatLng(), 1);
        return;
      }
      bag.map.fitBounds([[0, 0], [tmj.height * tmj.tileheight, tmj.width * tmj.tilewidth]]);
    }

    function ensureSelfMarker(at) {
      if (!bag.map || !tmj) return;
      if (!selfMarker) {
        selfMarker = L.marker(at, {
          icon: mapIcon("worker-map-self"),
          zIndexOffset: 700,
        }).bindTooltip("Вы", { permanent: true, direction: "right", className: "map-label" }).addTo(bag.map);
        return;
      }
      if (!bag.map.hasLayer(selfMarker)) selfMarker.addTo(bag.map);
      selfMarker.setLatLng(at);
    }

    function placeSelfMarker() {
      if (!selfPoint || !bag.map || !tmj) return;
      ensureSelfMarker(centerToLatLng(tmj, [selfPoint[0] + 0.5, selfPoint[1] + 0.5]));
      setSelfText(`ВЫ: ${selfPoint[0]}, ${selfPoint[1]}`);
    }

    function showSelfPoint(point, opts = {}) {
      const next = [Number(point[0]), Number(point[1])];
      if (!Number.isFinite(next[0]) || !Number.isFinite(next[1])) return;
      // Ход 5 км/ч ведёт «где я» по линии; серверная точка её не перебивает.
      if (opts.fromServer && walk && !walk.done) return;
      const same = selfPoint && selfPoint[0] === next[0] && selfPoint[1] === next[1];
      selfPoint = next;
      lastSelfRef.current = next;
      if (same && selfMarker && bag.map?.hasLayer(selfMarker)) return;
      placeSelfMarker();
      if (!walk) focus();
    }

    function tick(now) {
      bag.frame = 0;
      if (!walk || walk.done) return;
      const traveled = ((now - walk.startedAt) / 1000) * WALK_MPS;
      const sample = pointAt(walk.centers, traveled);
      if (!sample) return;
      const atCenter = sample.done ? walk.centers.at(-1) : sample.point;
      const at = centerToLatLng(tmj, atCenter);
      selfPoint = [Math.floor(atCenter[0]), Math.floor(atCenter[1])];
      ensureSelfMarker(at);
      setSelfText(`ВЫ: ${selfPoint[0]}, ${selfPoint[1]}`);
      if (sample.done) {
        // Линию не снимаем: остаётся точка у ВС вместе с маркером стоянки.
        walk.line.setLatLngs([at, walk.dest.getLatLng()]);
        walk.done = true;
        setDistance("НА МЕСТЕ");
        return;
      }
      const rest = walk.centers.slice(sample.nextIndex).map((center) => centerToLatLng(tmj, center));
      walk.line.setLatLngs([at, ...rest]);
      if (!bag.map.hasLayer(walk.line)) walk.line.addTo(bag.map);
      const leftMeters = Math.max(0, totalMeters(walk.centers) - traveled);
      const cellsLeft = Math.ceil(leftMeters / CELL_METERS - 1e-6);
      setDistance(`${cellsLeft} КЛЕТОК`);
      bag.frame = requestAnimationFrame(tick);
    }

    function renderRoute(nextTask) {
      const route = nextTask && Array.isArray(nextTask.route) ? nextTask.route : [];
      if (!bag.map || !tmj) return;
      if (!nextTask || route.length < 2) {
        stopWalk({ keepSelf: true });
        if (nextTask && Array.isArray(nextTask.plane_point)) {
          const end = [nextTask.plane_point[0] + 0.5, nextTask.plane_point[1] + 0.5];
          walk = {
            taskId: nextTask.id,
            routeSig: JSON.stringify(route),
            centers: [end],
            startedAt: performance.now(),
            done: true,
            line: L.polyline([centerToLatLng(tmj, end)], {
              color: "#c5161c",
              weight: 5,
              opacity: 0.95,
              lineJoin: "round",
            }).addTo(bag.map),
            dest: L.marker(centerToLatLng(tmj, end), { icon: mapIcon("worker-map-stand") })
              .bindTooltip(`ВС ${planeLabel(nextTask)}`, { permanent: true, direction: "top", className: "map-label" })
              .addTo(bag.map),
          };
          ensureSelfMarker(centerToLatLng(tmj, end));
          selfPoint = [Number(nextTask.plane_point[0]), Number(nextTask.plane_point[1])];
          setSelfText(`ВЫ: ${selfPoint[0]}, ${selfPoint[1]}`);
          setDistance("УЖЕ НА МЕСТЕ");
        } else {
          setDistance(nextTask ? "УЖЕ НА МЕСТЕ" : "НЕТ МАРШРУТА");
        }
        focus();
        return;
      }
      const routeSig = JSON.stringify(route);
      // Тот же task id, но новый route с сервера — пересобираем линию.
      if (walk && walk.taskId === nextTask.id && walk.routeSig === routeSig) {
        if (walk.line && !bag.map.hasLayer(walk.line)) walk.line.addTo(bag.map);
        if (walk.dest && !bag.map.hasLayer(walk.dest)) walk.dest.addTo(bag.map);
        if (selfMarker && !bag.map.hasLayer(selfMarker)) selfMarker.addTo(bag.map);
        return;
      }
      stopWalk({ keepSelf: true });
      const centers = route.map(([x, y]) => [x + 0.5, y + 0.5]);
      walk = {
        taskId: nextTask.id,
        routeSig,
        centers,
        startedAt: performance.now(),
        done: false,
        line: L.polyline(centers.map((center) => centerToLatLng(tmj, center)), {
          color: "#c5161c",
          weight: 5,
          opacity: 0.95,
          lineJoin: "round",
        }).addTo(bag.map),
        dest: L.marker(centerToLatLng(tmj, centers.at(-1)), { icon: mapIcon("worker-map-stand") })
          .bindTooltip(`ВС ${planeLabel(nextTask)}`, { permanent: true, direction: "top", className: "map-label" })
          .addTo(bag.map),
      };
      selfPoint = [Number(route[0][0]), Number(route[0][1])];
      ensureSelfMarker(centerToLatLng(tmj, centers[0]));
      setSelfText(`ВЫ: ${selfPoint[0]}, ${selfPoint[1]}`);
      setDistance(`${Math.max(0, route.length - 1)} КЛЕТОК`);
      bag.map.fitBounds(walk.line.getBounds(), { padding: [36, 36], maxZoom: 2 });
      bag.frame = requestAnimationFrame(tick);
    }

    (async () => {
      tmj = await loadApronMap();
      if (cancelled || !mapNode.current) return;
      const grid = buildTileGrid(tmj);
      const tilesets = await loadTilesetImages();
      if (cancelled || !mapNode.current) return;
      const map = L.map(mapNode.current, {
        crs: L.CRS.Simple,
        zoomControl: false,
        attributionControl: false,
        minZoom: -1,
        maxZoom: 3,
      });
      bag.map = map;
      L.control.zoom({ position: "bottomright" }).addTo(map);
      const bounds = [[0, 0], [tmj.height * tmj.tileheight, tmj.width * tmj.tilewidth]];
      L.imageOverlay(renderCanvas(tmj, grid, tilesets).toDataURL("image/png"), bounds).addTo(map);
      map.fitBounds(bounds);
      // Клики по карте не имитируют GPS — полевой трек отдельно через GPS Logger.
      const onResize = () => map.invalidateSize();
      window.addEventListener("resize", onResize);
      bag.cleanupResize = () => window.removeEventListener("resize", onResize);
      mapApi.current = { renderRoute, showSelfPoint, stopWalk };
      renderRoute(taskRef.current);
      requestAnimationFrame(() => map.invalidateSize());
    })().catch((error) => console.warn("[worker] фон карты не загружен", error));

    return () => {
      cancelled = true;
      stopWalk({ keepSelf: false });
      bag.cleanupResize?.();
      bag.map?.remove();
      mapApi.current = {};
    };
  }, [allowed]);

  useEffect(() => {
    mapApi.current.renderRoute?.(task);
  }, [task]);

  if (!allowed) return <Navigate to="/" replace />;

  const skill = engineerTypeLabel(profile?.engineerType) || "Инженер ОТО";
  const initials = String(profile?.name || "")
    .split(" ")
    .map((part) => part[0])
    .join("")
    .slice(0, 2)
    .toUpperCase() || "--";
  const route = Array.isArray(task?.route) ? task.route : [];
  const routeText = !task ? "" : route.length < 2 ? "Вы уже у воздушного судна." : `Маршрут: ${Math.max(0, route.length - 1)} клеток`;
  const step = chainStep(task, statusStep);
  const minutes = task ? etaMinutes(task.eta_seconds) : null;
  const etaClock = formatEta(etaLeft);
  const canDecide = task?.status === "assigned";

  async function refreshTask() {
    try {
      const next = await apiRequest("/api/tasks/current");
      const normalized = next && next.id ? next : null;
      setTask(normalized);
      return normalized;
    } catch (error) {
      if (error.status === 404) {
        setTask(null);
        return null;
      }
      throw error;
    }
  }

  async function acceptTask() {
    setBusyAction("accept");
    try {
      await apiRequest("/api/tasks/current/accept", { method: "POST" });
      await refreshTask();
      flash("Задача принята");
      try { navigator.vibrate?.(80); } catch { /* телефон */ }
    } catch (error) {
      setNotice(`Не удалось принять задачу: ${error.message}`);
      flash("Ошибка принятия", "err");
    } finally {
      setBusyAction("");
    }
  }

  async function rejectTask() {
    if (!window.confirm("Отклонить назначение? Задача вернётся диспетчеру.")) return;
    setBusyAction("reject");
    try {
      await apiRequest("/api/tasks/current/reject", { method: "POST" });
      setTask(null);
      setStatusStep("не назначен");
      setNotice("Задача отклонена. Ожидание нового назначения.");
      flash("Задача отклонена", "err");
    } catch (error) {
      setNotice(`Не удалось отклонить задачу: ${error.message}`);
      flash("Ошибка отклонения", "err");
    } finally {
      setBusyAction("");
    }
  }

  async function pickTools() {
    setBusyAction("tools");
    try {
      await apiRequest("/api/tasks/current/tools", { method: "POST" });
      setTask((prev) => (prev ? { ...prev, tools_picked: true } : prev));
      flash("Инструмент получен");
      softBeep(640, 100);
    } catch (error) {
      setNotice(`Не удалось отметить инструмент: ${error.message}`);
      flash("Ошибка инструмента", "err");
    } finally {
      setBusyAction("");
    }
  }

  async function waitForPart() {
    if (!window.confirm("Отметить ожидание запчасти? Задача уйдёт из текущих.")) return;
    setBusyAction("waiting");
    try {
      await apiRequest("/api/tasks/current/waiting-part", { method: "POST" });
      setTask(null);
      setStatusStep("не назначен");
      setNotice("Ожидание запчасти. Задача возвращена в очередь назначения.");
      flash("Ожидание запчасти");
    } catch (error) {
      setNotice(`Не удалось отметить ожидание запчасти: ${error.message}`);
    } finally {
      setBusyAction("");
    }
  }

  async function saveMel() {
    setBusyAction("mel");
    try {
      const category = melCategory;
      await apiRequest("/api/tasks/current/mel", {
        method: "POST",
        body: { category, defer_until: deferUntilIso(deferUntil) },
      });
      setTask((prev) => (prev ? { ...prev, mel_category: category } : prev));
      flash(`MEL ${category} сохранён`);
    } catch (error) {
      setNotice(`Не удалось сохранить MEL: ${error.message}`);
    } finally {
      setBusyAction("");
    }
  }

  async function completeTask() {
    if (!task?.tools_picked && !window.confirm("Инструмент ещё не отмечен. Всё равно завершить?")) return;
    if (!window.confirm("Завершить задачу? Отменить будет нельзя.")) return;
    setBusyAction("complete");
    try {
      await apiRequest("/api/tasks/current/complete", { method: "POST" });
      flash("Задача завершена");
      softBeep(880, 160);
      setStatusStep("выполнено");
      setTask(null);
    } catch (error) {
      setNotice(`Не удалось завершить задачу: ${error.message}`);
    } finally {
      setBusyAction("");
    }
  }

  async function toggleAog() {
    const next = !aogOn;
    if (!window.confirm(next ? "Поставить AOG на текущий борт?" : "Снять AOG с борта?")) return;
    setBusyAction("aog");
    try {
      const data = await apiRequest("/api/tasks/current/aog", {
        method: "PUT",
        body: { aog: next },
      });
      setAogOn(Boolean(data?.aog));
      flash(data?.aog ? "Самолёт отмечен как AOG" : "AOG снят", data?.aog ? "err" : "ok");
      try { navigator.vibrate?.(data?.aog ? [80, 40, 80] : 40); } catch { /* телефон */ }
    } catch (error) {
      setNotice(`Не удалось обновить AOG: ${error.message}`);
    } finally {
      setBusyAction("");
    }
  }

  async function refreshRouteFromServer() {
    if (!task?.id) return;
    setBusyAction("route");
    try {
      await apiRequest(`/api/tasks/${encodeURIComponent(task.id)}/route`, { method: "POST" });
      await refreshTask();
      flash("Маршрут пересчитан на сервере");
      softBeep(600, 100);
    } catch (error) {
      setNotice(`Не удалось обновить маршрут: ${error.message}`);
      flash("Ошибка маршрута", "err");
      softBeep(320, 200);
    } finally {
      setBusyAction("");
    }
  }

  async function pushSelfPosition() {
    const current = getSession();
    const point = lastSelfRef.current;
    if (!current?.userId || !Array.isArray(point)) {
      setNotice("Нет своей точки для отправки.");
      flash("Нет точки", "err");
      return;
    }
    setBusyAction("geo");
    const t = Math.floor(Date.now() / 1000);
    try {
      await updateEngineerPosition(current.userId, [[point, t]]);
      setOutboxCount(pendingCount());
      flash("Позиция отправлена");
      softBeep(500, 90);
    } catch (error) {
      enqueueEngineerPoint(current.userId, point, t);
      setOutboxCount(pendingCount());
      setNotice(`Сеть недоступна, точка в очереди (${pendingCount()}). ${error.message}`);
      flash("Гео в очередь", "err");
    } finally {
      setBusyAction("");
    }
  }

  return (
    <main className="worker-page">
      <NetBanner />
      <div className="worker-shell">
        <header className="worker-header">
          <div>
            <p className="eyebrow">АЭРОФЛОТ · ОТО</p>
            <h1>Инженер</h1>
          </div>
          <div className="worker-account">
            <div className="account-avatar">{initials}</div>
            <div className="account-copy">
              <p className="section-label">Учетная запись</p>
              <div className="account-name">{profile?.name || "—"}</div>
              <div className="account-role">{skill}{profile?.email ? ` · ${profile.email}` : ""}</div>
              {profile?.userId ? (
                <button
                  type="button"
                  className="ghost-copy"
                  style={{ marginTop: 6 }}
                  onClick={async () => {
                    const ok = await copyText(profile.userId);
                    flash(ok ? "UUID скопирован" : "Не удалось скопировать", ok ? "ok" : "err");
                  }}
                >
                  UUID · {String(profile.userId).slice(0, 8)}
                </button>
              ) : null}
            </div>
            <button
              type="button"
              className="role-link"
              onClick={() => {
                if (task && !window.confirm("Есть активная задача. Выйти?")) return;
                clearSession();
                navigate("/");
              }}
            >
              Выйти
            </button>
          </div>
        </header>

        <div className="worker-status-bar" aria-live="polite">
          <span className={task ? "worker-status busy" : "worker-status free"}>{task ? "Занят" : "Свободен"}</span>
          <span className="worker-task-badge">{task ? step : "Ожидание"}</span>
          {aogOn ? <span className="worker-task-badge aog">AOG</span> : null}
          {etaLeft === 0 ? <span className="worker-task-badge">На месте</span>
            : etaClock ? <span className="worker-task-badge">ETA {etaClock}</span>
              : minutes != null ? <span className="worker-task-badge">ETA {minutes} мин</span> : null}
          {task?.arrived ? <span className="worker-task-badge">Прибыл</span> : null}
          {pollStale ? <span className="worker-task-badge stale">Нет связи</span> : null}
        </div>

        <StatusChain current={step} />
        {canDecide ? (
          <details className="shortcut-help">
            <summary>Горячие клавиши</summary>
            <ul>
              <li><kbd>A</kbd> — принять</li>
              <li><kbd>R</kbd> — отклонить</li>
            </ul>
          </details>
        ) : null}

        <section className="worker-map-card">
          <div className="section-heading">
            <p className="section-label">Маршрут на перроне</p>
            <span className="task-state">{selfText}</span>
            <span className="task-state">{distance}</span>
          </div>
          <div id="worker-map" ref={mapNode} />
        </section>

        <section className="worker-grid">
          <article className="worker-profile">
            <p className="section-label">Профиль</p>
            <div className="worker-skill">{engineerTypeLabel(profile?.engineerType) || "Специализация не указана"}</div>
          </article>
          <article className="assignment-card">
            <div className="section-heading">
              <p className="section-label">Текущее назначение</p>
              <span className="task-state">{!task ? "ОЖИДАНИЕ" : step}</span>
            </div>
            <div className="task-content">
              {task ? (
                <>
                  <strong>ВС {planeLabel(task)}</strong><br />
                  {issueLabel(task.issue)}<br />
                  {task.description || ""}<br />
                  {routeText}<br />
                  ETA: {etaClock || (minutes != null ? `${minutes} мин` : "—")}<br />
                  {task.tools_picked ? "Инструмент получен" : "Инструмент не получен"}
                  {task.ata_chapter ? <><br />ATA {task.ata_chapter}</> : null}
                  {task.mel_category ? <><br />MEL {task.mel_category}</> : null}
                  {task.arrived ? <><br />Прибыл к воздушному судну</> : null}
                  <div className="task-id-row">
                    <span className="muted-id">#{String(task.id).slice(0, 8)}</span>
                    <button
                      type="button"
                      className="ghost-copy"
                      onClick={async () => {
                        const ok = await copyText(task.id);
                        flash(ok ? "ID задачи скопирован" : "Не удалось скопировать", ok ? "ok" : "err");
                      }}
                    >
                      Копировать ID
                    </button>
                  </div>
                  <p className="section-label">MEL</p>
                  <select aria-label="Категория MEL" value={melCategory} onChange={(event) => setMelCategory(event.target.value)}>
                    <option value="A">A</option>
                    <option value="B">B</option>
                    <option value="C">C</option>
                    <option value="D">D</option>
                  </select>
                  <p className="section-label">Отложить до</p>
                  <input
                    aria-label="Отложить до"
                    type="datetime-local"
                    min={new Date(Date.now() - new Date().getTimezoneOffset() * 60000).toISOString().slice(0, 16)}
                    value={deferUntil}
                    onChange={(event) => setDeferUntil(event.target.value)}
                  />
                </>
              ) : "Новых заявок нет."}
            </div>
          </article>
        </section>
        <section className="notice-card" role={/не удалось|нет связи|ошибк/i.test(notice) ? "alert" : "status"}>
          <div>{notice}</div>
        </section>
      </div>
      <div className="worker-dock" role="group" aria-label="Действия по задаче">
        <button id="accept-task" className="worker-action worker-action-accept" type="button" hidden={!canDecide} disabled={busyAction === "accept"} onClick={acceptTask} title="A">
          {busyAction === "accept" ? "…" : "Принять · A"}
        </button>
        <button id="reject-task" className="worker-action worker-action-danger" type="button" hidden={!canDecide} disabled={busyAction === "reject"} onClick={rejectTask} title="R">
          {busyAction === "reject" ? "…" : "Отклонить · R"}
        </button>
        <button id="save-mel" className="worker-action worker-action-secondary" type="button" hidden={!task} disabled={busyAction === "mel"} onClick={saveMel}>
          {busyAction === "mel" ? "…" : "Сохранить MEL"}
        </button>
        <button id="tools-task" className="worker-action worker-action-secondary" type="button" hidden={!task} disabled={!task || task.tools_picked || busyAction === "tools"} onClick={pickTools}>
          {task?.tools_picked ? "Инструмент ✓" : busyAction === "tools" ? "…" : "Инструмент получен"}
        </button>
        <button id="waiting-part" className="worker-action worker-action-warning" type="button" hidden={!task} disabled={busyAction === "waiting"} onClick={waitForPart}>
          {busyAction === "waiting" ? "…" : "Ожидание запчасти"}
        </button>
        <button id="complete-task" className="worker-action worker-action-primary" type="button" hidden={!task} disabled={busyAction === "complete"} onClick={completeTask}>
          {busyAction === "complete" ? "…" : "Завершить задачу"}
        </button>
        <button id="aog-task" className="worker-action worker-action-danger" type="button" hidden={!task?.plane_uuid} disabled={busyAction === "aog"} onClick={toggleAog}>
          {busyAction === "aog" ? "…" : aogOn ? "Снять AOG" : "Поставить AOG"}
        </button>
        <button id="refresh-route" className="worker-action worker-action-secondary" type="button" hidden={!task} disabled={busyAction === "route"} onClick={refreshRouteFromServer}>
          {busyAction === "route" ? "…" : "Обновить маршрут"}
        </button>
        <button
          id="refresh-task"
          className="worker-action worker-action-secondary"
          type="button"
          disabled={busyAction === "refresh"}
          onClick={async () => {
            setBusyAction("refresh");
            try {
              await refreshTask();
              flash("Задача обновлена");
            } catch (error) {
              setNotice(`Не удалось обновить: ${error.message}`);
              flash("Ошибка обновления", "err");
            } finally {
              setBusyAction("");
            }
          }}
        >
          {busyAction === "refresh" ? "…" : "Обновить задачу"}
        </button>
        <button id="push-geo" className="worker-action worker-action-secondary" type="button" disabled={busyAction === "geo"} onClick={pushSelfPosition}>
          {busyAction === "geo" ? "…" : outboxCount ? `Отправить гео · очередь ${outboxCount}` : "Отправить гео"}
        </button>
      </div>
      <div className={toastKind === "err" ? "worker-toast err" : "worker-toast"} role="status" aria-live="polite" hidden={!toast}>{toast}</div>
    </main>
  );
}
