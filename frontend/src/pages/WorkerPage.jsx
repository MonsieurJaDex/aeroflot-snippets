import { useEffect, useRef, useState } from "react";
import { Navigate, useNavigate } from "react-router-dom";
import L from "leaflet";
import NetBanner from "../components/NetBanner.jsx";
import StatusChain from "../components/StatusChain.jsx";
import { engineerTypeLabel, issueLabel } from "../lib/catalog.js";
import { formatEta, relativeRu } from "../lib/format.js";
import { softBeep } from "../lib/beep.js";
import { copyText } from "../lib/copyText.js";
import { enqueueAction, flushActions, pendingActionCount } from "../lib/actionOutbox.js";
import { enqueueEngineerPoint, enqueueGpsFix, flushEngineerOutbox, pendingCount } from "../lib/geoOutbox.js";
import { cellAfterAck } from "../lib/gpsSync.js";
import { getRoute, loadStaff, normalizeRoute, postTrackPoint, refreshTaskRoute, updateEngineerPosition } from "../lib/live.js";
import { apiRequest, clearSession, getSession, hydrateProfile } from "../lib/session.js";
import { apronOverlayUrl } from "../lib/apronSvg.js";
import { whenVisible } from "../lib/visibility.js";
import { CELL_METERS, expandRouteOnRoads, pointAt, totalMeters, turnHint, WALK_MPS } from "../lib/walk.js";
import {
  buildTileGrid,
  buildWalkMask,
  centerToLatLng,
  addPlaceLabels,
  routeSpansMap,
  gpsToCell,
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

const GPS_ACTIVE_STATUSES = new Set(["accepted", "in_progress"]);

function taskAllowsGps(t) {
  return Boolean(t?.id && GPS_ACTIVE_STATUSES.has(t.status));
}

function chainStep(task, fallback) {
  if (!task) return fallback;
  return CHAIN_BY_STATUS[task.status] || "в работе";
}

const ETA_SLA_SEC = 15 * 60;

function etaMinutes(seconds) {
  const value = Number(seconds);
  if (!Number.isFinite(value) || value < 0) return null;
  return Math.round(value / 60);
}

function taskSlaRisk(task, etaSec) {
  if (!task) return false;
  const sec = etaSec ?? Number(task.eta_seconds);
  if (Number.isFinite(sec) && sec > ETA_SLA_SEC) return true;
  return Boolean(
    task.sla_exceeded
    ?? task.time_limit_exceeded
    ?? task.sla_alerted
    ?? task.sla_breached,
  );
}

function slaNormHint(etaSec) {
  const sec = Number(etaSec);
  if (!Number.isFinite(sec) || sec < 0) return null;
  if (sec <= ETA_SLA_SEC) {
    return `норма 15 мин · осталось ${Math.max(1, Math.ceil(sec / 60))} мин`;
  }
  return `сверх нормы 15 мин · +${Math.ceil((sec - ETA_SLA_SEC) / 60)} мин`;
}

// datetime-local в RFC3339; пустая строка уходит как null.
function deferUntilIso(value) {
  const raw = String(value || "").trim();
  if (!raw) return null;
  const parsed = new Date(raw);
  if (Number.isNaN(parsed.getTime())) return raw;
  return parsed.toISOString();
}

// Статус только из is_accepted: этот сервер task.status не отдаёт.
function normalizeCurrentTask(body, prev = null) {
  if (body == null || typeof body !== "object" || !body.id) return null;
  const rawPlane = Array.isArray(body.plane_point) ? body.plane_point : null;
  const planePoint = rawPlane
    ? [Number(rawPlane[0]), Number(rawPlane[1])]
    : null;
  const accepted = Boolean(body.is_accepted);
  const knownStatus = new Set(["crew_remark", "unassigned", "assigned", "accepted", "in_progress", "done", "completed", "waiting_part"]);
  let route = normalizeRoute(body.route);
  if (route.length < 2 && prev && String(prev.id) === String(body.id)) {
    const kept = normalizeRoute(prev.route);
    if (kept.length >= 2) route = kept;
  }
  return {
    ...body,
    description: typeof body.description === "string" ? body.description : "",
    issue: body.issue == null || typeof body.issue === "object" ? "" : body.issue,
    is_accepted: accepted,
    status: knownStatus.has(body.status) ? body.status : (accepted ? "in_progress" : "assigned"),
    plane_point: planePoint && Number.isFinite(planePoint[0]) && Number.isFinite(planePoint[1]) ? planePoint : null,
    route,
  };
}

function endpointMissing(error) {
  return Number(error?.status) === 404;
}

function offlineError(error) {
  const status = Number(error?.status) || 0;
  return status === 0 || status === 408;
}

function queueDepth() {
  return pendingCount() + pendingActionCount();
}

function patchTaskRouteEta(prev, taskId, data) {
  if (!prev || String(prev.id) !== String(taskId) || !data || typeof data !== "object") return prev;
  if (data.route == null && data.eta_seconds == null) return prev;
  const next = { ...prev };
  if (data.route != null) next.route = data.route;
  if (data.eta_seconds != null) next.eta_seconds = data.eta_seconds;
  return next;
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
  const [routeHint, setRouteHint] = useState("");
  const [selfText, setSelfText] = useState("ВЫ: НЕТ ТОЧКИ");
  const [busyAction, setBusyAction] = useState("");
  const [statusStep, setStatusStep] = useState("не назначен");
  const [melCategory, setMelCategory] = useState(() => localStorage.getItem("oto-mel-cat") || "A");
  const [deferUntil, setDeferUntil] = useState("");
  const [outboxCount, setOutboxCount] = useState(() => queueDepth());
  const [etaLeft, setEtaLeft] = useState(null);
  const [pollStale, setPollStale] = useState(false);
  const lastSelfRef = useRef(null);
  const lastPollAt = useRef(Date.now());
  const etaAnchor = useRef({ id: null, at: 0, base: null });
  const etaArrivedBeep = useRef("");
  const staleBeepAt = useRef(0);
  const lastRouteRefresh = useRef(0);
  const statusNoteAt = useRef(0);
  const lastSpokenHint = useRef("");
  const lastGeoAt = useRef(0);
  const lastRouteCellRef = useRef(null);
  const gpsAckSeq = useRef(0);
  const [gpsManual, setGpsManual] = useState(() => localStorage.getItem("oto-gps-manual") === "1");
  const [gpsWatching, setGpsWatching] = useState(false);
  const [gpsAcc, setGpsAcc] = useState(null);
  const [gpsFix, setGpsFix] = useState(null);
  const [gpsLink, setGpsLink] = useState("search");
  const [gpsNet, setGpsNet] = useState(null);
  const gpsCellRef = useRef(null);
  const gpsFixRef = useRef(null);
  const gpsOutsideNoted = useRef(false);
  const lastTrackAt = useRef(0);
  const gpsWatchId = useRef(null);
  const gpsTrackNoted = useRef(false);
  const gpsAppliedNoted = useRef(false);
  const gpsCoarseNoted = useRef(false);
  const gpsLiveRef = useRef(false);
  const routeFillKey = useRef("");
  const [selfCell, setSelfCell] = useState(null);

  taskRef.current = task;

  async function applyRouteRefresh(taskId) {
    const data = await refreshTaskRoute(taskId);
    if (data && typeof data === "object") {
      setTask((prev) => patchTaskRouteEta(prev, taskId, data));
    }
    return refreshTask();
  }

  useEffect(() => {
    if (!allowed) return undefined;
    let cancelled = false;
    hydrateProfile()
      .then((next) => {
        if (!cancelled && next) setProfile(next);
      })
      .catch((error) => console.warn("[worker] профиль", error));

    async function loadTask() {
      const started = Date.now();
      try {
        const next = await apiRequest("/api/tasks/current");
        if (cancelled) return;
        const normalized = normalizeCurrentTask(next, taskRef.current);
        setTask((prev) => {
          const merged = normalizeCurrentTask(next, prev);
          if (!merged && !prev) return prev;
          if (merged && sameTask(prev, merged)) return prev;
          return merged;
        });
        lastPollAt.current = Date.now();
        setPollStale(false);
        // Не затирать строку 404, если её поставили пока этот опрос уже шёл.
        setNotice((current) => {
          if (statusNoteAt.current > started || gpsCellRef.current || gpsFixRef.current || gpsLiveRef.current) return current;
          if (!normalized) return "Ожидание назначения от диспетчера.";
          if (normalized.status === "assigned") return "Назначение получено. Примите или отклоните задачу.";
          if (normalized.status === "accepted") return "Задача принята. Отметьте инструмент и идите по маршруту.";
          if (normalized.status === "done" || normalized.status === "completed") return "Задача выполнена.";
          return "Задача активна. На карте — ваша точка и маршрут. Завершите, когда работы закончены.";
        });
      } catch (error) {
        if (cancelled) return;
        if (error.status === 404) {
          lastPollAt.current = Date.now();
          setPollStale(false);
          setTask(null);
          if (!gpsCellRef.current && !gpsFixRef.current && !gpsLiveRef.current) {
            setNotice("Ожидание назначения от диспетчера.");
          }
          return;
        }
        setNotice(`Не удалось получить назначение: ${error.message}`);
      }
    }

    async function refreshSelf() {
      const current = getSession();
      if (!current?.userId) return;
      try {
        const staff = await loadStaff();
        if (cancelled) return;
        const mine = (staff?.plotted || []).find((entry) => String(entry.id) === String(current.userId));
        if (!mine?.point) return;
        lastSelfRef.current = mine.point;
        setSelfCell((prev) => (prev && prev[0] === mine.point[0] && prev[1] === mine.point[1] ? prev : mine.point));
        mapApi.current.showSelfPoint?.(mine.point, { live: true });
      } catch (error) {
        console.warn("[worker] своя позиция", error);
      }
    }

    async function flushOutbox() {
      const current = getSession();
      if (!current?.userId) return;
      try {
        const sent = await flushEngineerOutbox(async (row) => {
          if (String(row.id) !== String(current.userId)) throw new Error("чужой инженер");
          if (row.kind === "gps") {
            await postTrackPoint({
              obj: row.id,
              lat: row.lat,
              lon: row.lon,
              t: row.t,
              acc: row.acc,
              spd: row.spd,
            });
            return;
          }
          await updateEngineerPosition(current.userId, [[row.point, row.t]]);
        });
        const sentActions = await flushActions((row) => apiRequest(row.path, {
          method: row.method || "POST",
          body: row.body || undefined,
        }));
        setOutboxCount(queueDepth());
        if (sent > 0) {
          flash(`Дослано геоточек: ${sent}`);
          softBeep(640, 100);
          const taskId = taskRef.current?.id;
          if (taskId) {
            lastRouteRefresh.current = Date.now();
            try {
              await applyRouteRefresh(taskId);
            } catch (error) {
              console.warn("[worker] route refresh after outbox", error);
            }
          }
        }
        if (sentActions > 0 && !cancelled) {
          flash(`Дослано действий: ${sentActions}`);
          try {
            const next = await apiRequest("/api/tasks/current");
            if (!cancelled) setTask(normalizeCurrentTask(next, taskRef.current));
          } catch (error) {
            console.warn("[worker] task after actions", error);
          }
        }
      } catch (error) {
        console.warn("[worker] outbox", error);
      }
    }

    loadTask();
    refreshSelf();
    async function pushLiveGeo() {
      const current = getSession();
      if (!current?.userId) return;
      // Живой спутник: клетку не шлём, маршрут — только если клетка сменилась.
      if (gpsFixRef.current?.good) {
        await refreshSelf();
        const point = lastSelfRef.current;
        const prev = lastRouteCellRef.current;
        const changed = Array.isArray(point) && (!Array.isArray(prev) || prev[0] !== point[0] || prev[1] !== point[1]);
        const taskId = taskRef.current?.id;
        if (changed && taskId && Date.now() - lastRouteRefresh.current > 15000) {
          lastRouteRefresh.current = Date.now();
          const snapshot = [point[0], point[1]];
          try {
            await applyRouteRefresh(taskId);
            lastRouteCellRef.current = snapshot;
          } catch (error) {
            console.warn("[worker] route refresh", error);
          }
        }
        return;
      }
      const point = lastSelfRef.current;
      if (!Array.isArray(point)) return;
      const t = Math.floor(Date.now() / 1000);
      try {
        await updateEngineerPosition(current.userId, [[point, t]]);
        lastGeoAt.current = Date.now();
        setOutboxCount(queueDepth());
        // п.8: после новой геоточки пересчитываем маршрут на сервере (не чаще 15 с).
        const taskId = taskRef.current?.id;
        if (taskId && Date.now() - lastRouteRefresh.current > 15000) {
          lastRouteRefresh.current = Date.now();
          try {
            await applyRouteRefresh(taskId);
          } catch (error) {
            console.warn("[worker] route refresh", error);
          }
        }
      } catch {
        enqueueEngineerPoint(current.userId, point, t);
        setOutboxCount(queueDepth());
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
      if (!queueDepth()) return;
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
    if (!allowed) return undefined;
    const wantGps = gpsManual || taskAllowsGps(task) || window.matchMedia("(pointer: coarse)").matches;

    function stopGpsWatch() {
      gpsLiveRef.current = false;
      if (gpsWatchId.current != null) {
        navigator.geolocation.clearWatch(gpsWatchId.current);
        gpsWatchId.current = null;
      }
      setGpsWatching(false);
      setGpsLink("search");
      setGpsNet(null);
      setGpsFix(null);
      setGpsAcc(null);
    }

    if (!wantGps) {
      stopGpsWatch();
      return undefined;
    }
    if (!navigator.geolocation) {
      setNotice("Браузер не поддерживает GPS.");
      return undefined;
    }

    const onFix = (pos) => {
      const { latitude: lat, longitude: lon, accuracy: acc } = pos.coords;
      const accuracy = Number.isFinite(acc) ? acc : null;
      const age = Number.isFinite(pos.timestamp) ? Date.now() - pos.timestamp : 0;
      // Кэш и сеть (часто Шереметьево ±км). На карту только свежий спутник.
      if (age > 15000) return;
      const coarse = accuracy == null || accuracy > 80;
      if (coarse) {
        if (gpsFixRef.current?.good) return;
        const meters = accuracy == null ? null : Math.round(accuracy);
        setGpsNet(meters);
        setGpsLink("net");
        if (!gpsCoarseNoted.current) {
          gpsCoarseNoted.current = true;
          statusNoteAt.current = Date.now();
          setNotice(`Сеть ±${meters ?? "—"} м (${lat.toFixed(5)}, ${lon.toFixed(5)}). Это не GPS. На iPhone: Настройки → Конфиденциальность → Службы геолокации → Safari → Точная геопозиция.`);
        }
        return;
      }
      setGpsNet(null);
      setGpsAcc(accuracy);
      setGpsLink("live");
      const speed = Number(pos.coords.speed);
      const spd = Number.isFinite(speed) && speed >= 0 ? speed : null;
      const fix = { lat, lon, good: true, acc: accuracy, spd };
      gpsFixRef.current = fix;
      setGpsFix(fix);
      setGpsWatching(true);
      gpsCoarseNoted.current = false;
      const project = mapApi.current.project;
      if (project) {
        const cell = project(lat, lon);
        if (cell) {
          gpsOutsideNoted.current = false;
          gpsCellRef.current = cell;
        } else {
          gpsCellRef.current = null;
          gpsAppliedNoted.current = false;
          if (!gpsOutsideNoted.current) {
            gpsOutsideNoted.current = true;
            statusNoteAt.current = Date.now();
            setNotice(`GPS ${lat.toFixed(5)}, ${lon.toFixed(5)} ±${Math.round(accuracy)} м. Вне полигона ВДНХ, на карту не ставим.`);
          }
        }
      }
      const now = Date.now();
      if (now - lastTrackAt.current < 2000) return;
      lastTrackAt.current = now;
      const userId = getSession()?.userId;
      if (!userId) return;
      const localCell = Array.isArray(gpsCellRef.current)
        ? [gpsCellRef.current[0], gpsCellRef.current[1]]
        : null;
      const previousCell = Array.isArray(lastSelfRef.current)
        ? [lastSelfRef.current[0], lastSelfRef.current[1]]
        : null;
      const t = Math.floor(pos.timestamp / 1000);
      const iso = new Date(pos.timestamp).toISOString();
      const seq = ++gpsAckSeq.current;
      const applyTrackDecision = (decision) => {
        if (decision?.writeLegacy && localCell) {
          updateEngineerPosition(userId, [[localCell, t]]).then(() => {
            lastGeoAt.current = Date.now();
            setGpsLink("legacy");
            setOutboxCount(queueDepth());
          }).catch((fallbackError) => {
            enqueueEngineerPoint(userId, localCell, t);
            setOutboxCount(queueDepth());
            console.warn("[worker] позиция", fallbackError);
          });
        }
        if (!Array.isArray(decision?.cell)) return;
        const next = [Number(decision.cell[0]), Number(decision.cell[1])];
        if (!Number.isFinite(next[0]) || !Number.isFinite(next[1])) return;
        lastSelfRef.current = next;
        mapApi.current.showSelfPoint?.(next, { live: true, follow: true });
        if (!gpsAppliedNoted.current) {
          gpsAppliedNoted.current = true;
          statusNoteAt.current = Date.now();
          setNotice(`На карте: ${next[0]}, ${next[1]}`);
        }
      };
      postTrackPoint({
        obj: userId,
        lat,
        lon,
        t: iso,
        acc: accuracy,
        spd,
      }).then(async (ack) => {
        if (seq !== gpsAckSeq.current) return;
        lastGeoAt.current = Date.now();
        setOutboxCount(queueDepth());
        let serverCell = null;
        if (ack?.applied) {
          try {
            const staff = await loadStaff();
            if (seq !== gpsAckSeq.current) return;
            const mine = (staff?.plotted || []).find((entry) => String(entry.id) === String(userId));
            if (mine?.point) serverCell = [mine.point[0], mine.point[1]];
          } catch (error) {
            console.warn("[worker] своя клетка", error);
          }
        }
        if (seq !== gpsAckSeq.current) return;
        const decision = cellAfterAck({
          applied: ack?.applied,
          httpStatus: 200,
          serverCell,
          localCell,
          previousCell,
        });
        applyTrackDecision(decision);
        if (!decision?.writeLegacy) setGpsLink(ack?.applied === false ? "held" : "live");
      }).catch((error) => {
        if (seq !== gpsAckSeq.current) return;
        const status = Number(error?.status) || 0;
        if (status === 404) {
          const decision = cellAfterAck({
            applied: false,
            httpStatus: 404,
            serverCell: null,
            localCell,
            previousCell,
          });
          applyTrackDecision(decision);
          return;
        }
        enqueueGpsFix(userId, { lat, lon, t: iso, acc: accuracy, spd });
        setGpsLink("queue");
        setOutboxCount(queueDepth());
        console.warn("[worker] трек", error);
        if (gpsCellRef.current || gpsTrackNoted.current) return;
        gpsTrackNoted.current = true;
        statusNoteAt.current = Date.now();
        setNotice("GPS-точка не ушла на сервер, лежит в очереди.");
      });
    };

    const onErr = (error) => {
      if (error.code === 1) {
        stopGpsWatch();
        setGpsAcc(null);
        setNotice("GPS: доступ к геолокации запрещён. Включите в настройках браузера или коснитесь «GPS».");
        flash("GPS: нет разрешения", "err");
        return;
      }
      if (gpsFixRef.current?.good) return;
      statusNoteAt.current = Date.now();
      setNotice(error.code === 3
        ? "Ждём спутник. Грубая точка сети не используется."
        : "GPS пока недоступен. Поиск спутника не останавливаем.");
    };

    gpsLiveRef.current = true;
    setGpsWatching(true);
    setGpsLink((prev) => (prev === "live" || prev === "held" || prev === "legacy" || prev === "queue" ? prev : "search"));
    gpsWatchId.current = navigator.geolocation.watchPosition(onFix, onErr, {
      enableHighAccuracy: true,
      maximumAge: 0,
      timeout: 60000,
    });
    setGpsWatching(true);

    return () => stopGpsWatch();
  }, [allowed, gpsManual, task?.id, task?.status]);

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
      window.queueMicrotask?.(() => {
        document.getElementById("accept-task")?.scrollIntoView({ block: "nearest", behavior: "smooth" });
      });
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

  useEffect(() => {
    if (!pollStale) {
      if (staleBeepAt.current) softBeep(640, 80);
      staleBeepAt.current = 0;
      return;
    }
    const now = Date.now();
    if (now - staleBeepAt.current < 45000 && staleBeepAt.current) return;
    staleBeepAt.current = now;
    softBeep(300, 180);
  }, [pollStale]);

  function flash(text, kind = "ok") {
    setToastKind(kind);
    setToast(text);
  }

  useEffect(() => {
    if (!routeHint || routeHint === lastSpokenHint.current) return undefined;
    if (typeof window === "undefined" || !window.speechSynthesis) return undefined;
    lastSpokenHint.current = routeHint;
    const timer = window.setTimeout(() => {
      try {
        window.speechSynthesis.cancel();
        const utter = new SpeechSynthesisUtterance(routeHint);
        utter.lang = "ru-RU";
        utter.rate = 1.05;
        window.speechSynthesis.speak(utter);
      } catch { /* TTS optional */ }
    }, 400);
    return () => clearTimeout(timer);
  }, [routeHint]);

  useEffect(() => {
    const out = outboxCount ? ` · гео ${outboxCount}` : "";
    document.title = task
      ? `ОТО · задача · ETA ${formatEta(etaLeft) || "…"}${out}`
      : outboxCount
        ? `ОТО · инженер · гео ${outboxCount}`
        : "ОТО · инженер свободен";
    return () => { document.title = "Аэрофлот ОТО"; };
  }, [task, etaLeft, outboxCount]);

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
      if (window.matchMedia("(max-width: 860px), (pointer: coarse)").matches) return;
      if (event.key === "Escape") {
        setToast("");
        return;
      }
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
      if (event.key === "t" || event.key === "T" || event.key === "е" || event.key === "Е") {
        if (taskRef.current && !taskRef.current.tools_picked) {
          event.preventDefault();
          document.getElementById("tools-task")?.click();
        }
      }
      if (event.key === "c" || event.key === "C" || event.key === "с" || event.key === "С") {
        if (taskRef.current) {
          event.preventDefault();
          document.getElementById("complete-task")?.click();
        }
      }
      if (event.key === "w" || event.key === "W" || event.key === "ц" || event.key === "Ц") {
        if (taskRef.current) {
          event.preventDefault();
          document.getElementById("waiting-part")?.click();
        }
      }
      if (event.key === "g" || event.key === "G" || event.key === "п" || event.key === "П") {
        event.preventDefault();
        document.getElementById("push-geo")?.click();
      }
      if (event.key === "m" || event.key === "M" || event.key === "ь" || event.key === "Ь") {
        if (taskRef.current) {
          event.preventDefault();
          document.getElementById("save-mel")?.click();
        }
      }
      if (event.key === "o" || event.key === "O" || event.key === "щ" || event.key === "Щ") {
        if (taskRef.current) {
          event.preventDefault();
          document.getElementById("aog-task")?.click();
        }
      }
      if (event.key === "f" || event.key === "F" || event.key === "а" || event.key === "А") {
        event.preventDefault();
        document.getElementById("refresh-task")?.click();
      }
      if (event.key === "u" || event.key === "U" || event.key === "г" || event.key === "Г") {
        if (taskRef.current) {
          event.preventDefault();
          document.getElementById("refresh-route")?.click();
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
      etaArrivedBeep.current = "";
    }
    const tick = () => {
      const { at, base } = etaAnchor.current;
      if (base == null) return;
      const left = Math.max(0, base - (Date.now() - at) / 1000);
      setEtaLeft(left);
      if (left === 0 && etaArrivedBeep.current !== task.id) {
        etaArrivedBeep.current = task.id;
        flash("ETA: вы на месте");
        softBeep(880, 160);
        try { navigator.vibrate?.(100); } catch { /* телефон */ }
      }
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
    let walkMask = null;

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
        const bounds = L.latLngBounds(latlngs);
        if (routeSpansMap(tmj, bounds)) bag.map.fitBounds(bounds, { padding: [36, 36], maxZoom: 2 });
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

    function clearSelf() {
      selfPoint = null;
      if (selfMarker && bag.map) {
        bag.map.removeLayer(selfMarker);
        selfMarker = null;
      }
      setSelfText("ВЫ: ВНЕ КАРТЫ");
    }

    function showSelfPoint(point, opts = {}) {
      const next = [Number(point[0]), Number(point[1])];
      if (!Number.isFinite(next[0]) || !Number.isFinite(next[1])) return;
      // Живой GPS ставит точку даже во время хода.
      if (!opts.live && opts.fromServer && walk && !walk.done) return;
      const same = selfPoint && selfPoint[0] === next[0] && selfPoint[1] === next[1];
      selfPoint = next;
      lastSelfRef.current = next;
      if (same && selfMarker && bag.map?.hasLayer(selfMarker)) return;
      placeSelfMarker();
      if (opts.follow && selfMarker && bag.map) bag.map.panTo(selfMarker.getLatLng());
      else if (!walk) focus();
    }

    function tick(now) {
      bag.frame = 0;
      if (!walk || walk.done) return;
      const traveled = ((now - walk.startedAt) / 1000) * WALK_MPS;
      const sample = pointAt(walk.centers, traveled);
      if (!sample) return;
      const atCenter = sample.done ? walk.centers.at(-1) : sample.point;
      const at = centerToLatLng(tmj, atCenter);
      if (!gpsFixRef.current?.good) {
        selfPoint = [Math.floor(atCenter[0]), Math.floor(atCenter[1])];
        ensureSelfMarker(at);
        setSelfText(`ВЫ: ${selfPoint[0]}, ${selfPoint[1]}`);
      }
      if (sample.done) {
        // Линию не снимаем: остаётся точка у ВС вместе с маркером стоянки.
        walk.line.setLatLngs([at, walk.dest.getLatLng()]);
        walk.done = true;
        setDistance("НА МЕСТЕ");
        setRouteHint("Вы в зоне обслуживания ВС");
        softBeep(720, 120);
        return;
      }
      const rest = walk.centers.slice(sample.nextIndex).map((center) => centerToLatLng(tmj, center));
      walk.line.setLatLngs([at, ...rest]);
      if (!bag.map.hasLayer(walk.line)) walk.line.addTo(bag.map);
      const leftMeters = Math.max(0, totalMeters(walk.centers) - traveled);
      const cellsLeft = Math.ceil(leftMeters / CELL_METERS - 1e-6);
      setDistance(`${cellsLeft} КЛЕТОК`);
      setRouteHint(turnHint(walk.centers, sample.nextIndex));
      bag.frame = requestAnimationFrame(tick);
    }

    function renderRoute(nextTask) {
      const route = nextTask ? normalizeRoute(nextTask.route) : [];
      if (!bag.map || !tmj) return;
      if (!nextTask || route.length < 2) {
        stopWalk({ keepSelf: true });
        const px = Number(nextTask?.plane_point?.[0]);
        const py = Number(nextTask?.plane_point?.[1]);
        if (nextTask && Number.isFinite(px) && Number.isFinite(py)) {
          const end = [px + 0.5, py + 0.5];
          walk = {
            taskId: nextTask.id,
            routeSig: JSON.stringify(route),
            centers: [],
            startedAt: performance.now(),
            done: true,
            line: null,
            dest: L.marker(centerToLatLng(tmj, end), { icon: mapIcon("worker-map-stand") })
              .bindTooltip(`ВС ${planeLabel(nextTask)}`, { permanent: true, direction: "top", className: "map-label" })
              .addTo(bag.map),
          };
          setDistance("НЕТ МАРШРУТА");
          setRouteHint("Маршрут до ВС строится");
        } else {
          setDistance("НЕТ МАРШРУТА");
          setRouteHint("");
        }
        if (selfPoint) placeSelfMarker();
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
      // Только по дорожным клеткам TMJ — без диагоналей сквозь стены.
      const roadRoute = expandRouteOnRoads(route, walkMask);
      if (roadRoute.length < 2) {
        setDistance("НЕТ МАРШРУТА");
        setRouteHint("Маршрут до ВС строится");
        focus();
        return;
      }
      const centers = roadRoute.map(([x, y]) => [x + 0.5, y + 0.5]);
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
      selfPoint = [Number(roadRoute[0][0]), Number(roadRoute[0][1])];
      ensureSelfMarker(centerToLatLng(tmj, centers[0]));
      setSelfText(`ВЫ: ${selfPoint[0]}, ${selfPoint[1]}`);
      setDistance(`${Math.max(0, roadRoute.length - 1)} КЛЕТОК`);
      setRouteHint(turnHint(centers, 1));
      if (routeSpansMap(tmj, walk.line.getBounds())) {
        bag.map.fitBounds(walk.line.getBounds(), { padding: [36, 36], maxZoom: 2 });
      }
      bag.frame = requestAnimationFrame(tick);
    }

    (async () => {
      tmj = await loadApronMap();
      if (cancelled || !mapNode.current) return;
      walkMask = buildWalkMask(tmj);
      const grid = buildTileGrid(tmj);
      const tilesets = await loadTilesetImages();
      if (cancelled || !mapNode.current) return;
      const map = L.map(mapNode.current, {
        crs: L.CRS.Simple,
        zoomControl: false,
        attributionControl: false,
        minZoom: -4,
        maxZoom: 6,
        zoomSnap: 0,
      });
      bag.map = map;
      L.control.zoom({ position: "bottomright" }).addTo(map);
      const bounds = [[0, 0], [tmj.height * tmj.tileheight, tmj.width * tmj.tilewidth]];
      const overlay = L.imageOverlay(apronOverlayUrl(tmj) || renderCanvas(tmj, grid, tilesets).toDataURL("image/png"), bounds).addTo(map);
      addPlaceLabels(map, tmj);
      // zoomSnap 0: полигон вписывается в прямоугольник контейнера целиком.
      const apronPad = { padding: [16, 16], animate: false };
      const lngSpan = Math.abs(bounds[1][1] - bounds[0][1]);
      const latSpan = Math.abs(bounds[1][0] - bounds[0][0]);
      let settled = false;
      let userMoved = false;
      let lastFit = { target: bounds, options: apronPad };

      function stampOverlay() {
        const img = overlay.getElement();
        const zoom = map.getZoom();
        if (!img || !Number.isFinite(zoom)) return;
        img.style.setProperty("width", `${lngSpan * Math.pow(2, zoom)}px`, "important");
        img.style.setProperty("height", `${latSpan * Math.pow(2, zoom)}px`, "important");
        img.style.setProperty("max-width", "none", "important");
        img.style.setProperty("max-height", "none", "important");
        img.style.objectFit = "fill";
      }

      function applyFit(target, options = {}) {
        const node = map.getContainer();
        const pad = options.padding || [0, 0];
        const padX = Number(pad[0]) || 0;
        const padY = Number(pad[1] ?? pad[0]) || 0;
        const w = node.clientWidth - padX * 2;
        const h = node.clientHeight - padY * 2;
        if (w < 8 || h < 8) return false;
        const box = L.latLngBounds(target);
        const lng = Math.abs(box.getEast() - box.getWest());
        const lat = Math.abs(box.getNorth() - box.getSouth());
        if (lng < 1 || lat < 1) return false;
        const cap = typeof options.maxZoom === "number" ? options.maxZoom : map.getMaxZoom();
        const zoom = Math.max(map.getMinZoom(), Math.min(cap, Math.log2(Math.min(w / lng, h / lat))));
        if (map._loaded) map.invalidateSize(false);
        map.setView(box.getCenter(), zoom, { animate: false });
        stampOverlay();
        return true;
      }

      function isApronTarget(target) {
        const box = L.latLngBounds(target);
        return Math.abs(Math.abs(box.getEast() - box.getWest()) - lngSpan) < 1
          && Math.abs(Math.abs(box.getNorth() - box.getSouth()) - latSpan) < 1;
      }

      map.fitBounds = (target, options) => {
        const opts = options || {};
        const next = isApronTarget(target) && !opts.padding ? { ...opts, padding: [16, 16] } : opts;
        lastFit = { target, options: next };
        if (applyFit(target, next)) settled = true;
        return map;
      };
      map.on("dragstart", () => { userMoved = true; });
      map.on("zoomstart", (event) => { if (event.originalEvent) userMoved = true; });
      map.on("zoomend", stampOverlay);
      overlay.on("add load", stampOverlay);

      function ensureFitted() {
        if (userMoved || settled) return;
        if (applyFit(lastFit.target, lastFit.options)) settled = true;
      }

      function apronUnderfilled() {
        const node = map.getContainer();
        const zoom = map.getZoom();
        if (!Number.isFinite(zoom) || node.clientWidth < 32 || node.clientHeight < 32) return false;
        const limit = Math.min((node.clientWidth - 32) / lngSpan, (node.clientHeight - 32) / latSpan);
        return Math.pow(2, zoom) < limit * 0.92;
      }

      map.fitBounds(bounds, apronPad);
      bag.fitFrame = requestAnimationFrame(() => {
        if (cancelled || userMoved) return;
        if (settled && !isApronTarget(lastFit.target)) return;
        if (!settled || apronUnderfilled()) {
          settled = false;
          ensureFitted();
        }
      });
      // Клики по карте не имитируют GPS — полевой трек отдельно через GPS Logger.
      const onResize = () => {
        map.invalidateSize(false);
        ensureFitted();
      };
      window.addEventListener("resize", onResize);
      const boxObserver = new ResizeObserver(() => ensureFitted());
      boxObserver.observe(map.getContainer());
      bag.cleanupResize = () => {
        cancelAnimationFrame(bag.fitFrame);
        boxObserver.disconnect();
        window.removeEventListener("resize", onResize);
      };
      mapApi.current = {
        renderRoute,
        showSelfPoint,
        clearSelf,
        stopWalk,
        project(lat, lon) { return gpsToCell(tmj, lat, lon); },
      };
      const pending = gpsFixRef.current;
      if (pending?.good) {
        const cell = gpsToCell(tmj, pending.lat, pending.lon);
        gpsCellRef.current = cell || null;
      }
      renderRoute(taskRef.current);
      // Маркер — клетка сервера, не локальный floor(gps).
      if (gpsFixRef.current?.good && Array.isArray(lastSelfRef.current)) {
        showSelfPoint(lastSelfRef.current, { live: true });
      }
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

  useEffect(() => {
    if (!task?.id || !selfCell) return undefined;
    if (normalizeRoute(task.route).length >= 2) return undefined;
    const end = task.plane_point;
    if (!Array.isArray(end) || !Number.isFinite(end[0]) || !Number.isFinite(end[1])) return undefined;
    if (selfCell[0] === end[0] && selfCell[1] === end[1]) return undefined;
    const key = `${task.id}|${selfCell[0]},${selfCell[1]}|${end[0]},${end[1]}`;
    if (routeFillKey.current === key) return undefined;
    routeFillKey.current = key;
    let cancelled = false;
    const taskId = task.id;
    getRoute(selfCell, end)
      .then((data) => {
        if (cancelled) return;
        const built = normalizeRoute(data?.route);
        if (built.length < 2) return;
        setTask((prev) => {
          if (!prev || String(prev.id) !== String(taskId)) return prev;
          if (normalizeRoute(prev.route).length >= 2) return prev;
          return { ...prev, route: built };
        });
      })
      .catch((error) => {
        if (!cancelled) console.warn("[worker] getRoute", error);
      });
    return () => { cancelled = true };
  }, [task?.id, task?.route, task?.plane_point, selfCell]);

  if (!allowed) return <Navigate to="/" replace />;

  const skill = engineerTypeLabel(profile?.engineerType) || "Инженер ОТО";
  const initials = String(profile?.name || "")
    .split(" ")
    .map((part) => part[0])
    .join("")
    .slice(0, 2)
    .toUpperCase() || "--";
  const route = task ? normalizeRoute(task.route) : [];
  const routeText = !task ? "" : route.length < 2 ? "Маршрут до ВС строится." : `Маршрут: ${Math.max(0, route.length - 1)} клеток`;
  const step = chainStep(task, statusStep);
  const minutes = task ? etaMinutes(task.eta_seconds) : null;
  const etaClock = formatEta(etaLeft);
  const etaForSla = etaLeft != null ? etaLeft : (task?.eta_seconds != null ? Number(task.eta_seconds) : null);
  const slaRisk = task ? taskSlaRisk(task, etaForSla) : false;
  const slaHint = slaRisk && etaForSla != null ? slaNormHint(etaForSla) : null;
  const canDecide = task?.status === "assigned";

  function noteMissing(error, missingLine, failLine) {
    statusNoteAt.current = Date.now();
    if (endpointMissing(error)) {
      setNotice(missingLine);
      return;
    }
    setNotice(`${failLine}: ${error?.message || "ошибка"}`);
    flash(failLine, "err");
    softBeep(320, 200);
  }

  async function refreshTask() {
    try {
      const next = await apiRequest("/api/tasks/current");
      const normalized = normalizeCurrentTask(next, taskRef.current);
      setTask(normalized);
      return normalized;
    } catch (error) {
      if (endpointMissing(error)) {
        setTask(null);
        return null;
      }
      throw error;
    }
  }

  async function runTaskAction(path, options, onQueued) {
    try {
      const data = await apiRequest(path, options);
      return { queued: false, data };
    } catch (error) {
      if (!offlineError(error)) throw error;
      enqueueAction({ path, method: options?.method || "POST", body: options?.body ?? null });
      onQueued?.();
      setOutboxCount(queueDepth());
      flash("Нет сети. Действие в очереди и уйдёт само.");
      softBeep(420, 140);
      return { queued: true, data: null };
    }
  }

  async function acceptTask() {
    setBusyAction("accept");
    try {
      const result = await runTaskAction("/api/tasks/current/accept", { method: "POST" }, () => {
        setTask((prev) => (prev ? { ...prev, status: "in_progress", is_accepted: true } : prev));
      });
      if (result.queued) return;
      await refreshTask();
      flash("Задача принята");
      softBeep(720, 120);
      try { navigator.vibrate?.(80); } catch { /* телефон */ }
    } catch (error) {
      setNotice(`Не удалось принять задачу: ${error.message}`);
      flash("Ошибка принятия", "err");
      softBeep(320, 200);
    } finally {
      setBusyAction("");
    }
  }

  async function rejectTask() {
    if (!window.confirm("Отклонить назначение? Задача вернётся диспетчеру.")) return;
    setBusyAction("reject");
    try {
      const result = await runTaskAction("/api/tasks/current/reject", { method: "POST" });
      if (result.queued) {
        setTask(null);
        setStatusStep("не назначен");
        return;
      }
      setTask(null);
      setStatusStep("не назначен");
      setNotice("Задача отклонена. Ожидание нового назначения.");
      flash("Задача отклонена", "err");
      softBeep(380, 180);
    } catch (error) {
      noteMissing(error, "Отклонение недоступно.", "Ошибка отклонения");
    } finally {
      setBusyAction("");
    }
  }

  async function pickTools() {
    setBusyAction("tools");
    try {
      const result = await runTaskAction("/api/tasks/current/tools", { method: "POST" }, () => {
        setTask((prev) => (prev ? { ...prev, tools_picked: true } : prev));
      });
      if (result.queued) return;
      setTask((prev) => (prev ? { ...prev, tools_picked: true } : prev));
      flash("Инструмент получен");
      softBeep(640, 100);
    } catch (error) {
      noteMissing(error, "Отметка инструмента недоступна.", "Ошибка инструмента");
    } finally {
      setBusyAction("");
    }
  }

  async function waitForPart() {
    if (!window.confirm("Отметить ожидание запчасти? Задача уйдёт из текущих.")) return;
    setBusyAction("waiting");
    try {
      const result = await runTaskAction("/api/tasks/current/waiting-part", { method: "POST" });
      if (result.queued) {
        setTask(null);
        setStatusStep("не назначен");
        return;
      }
      setTask(null);
      setStatusStep("не назначен");
      setNotice("Ожидание запчасти. Задача возвращена в очередь назначения.");
      flash("Ожидание запчасти");
      softBeep(480, 140);
    } catch (error) {
      noteMissing(error, "Ожидание запчасти недоступно.", "Ошибка ожидания");
    } finally {
      setBusyAction("");
    }
  }

  async function saveMel() {
    setBusyAction("mel");
    try {
      const category = melCategory;
      const result = await runTaskAction("/api/tasks/current/mel", {
        method: "POST",
        body: { category, defer_until: deferUntilIso(deferUntil) },
      }, () => {
        setTask((prev) => (prev ? { ...prev, mel_category: category } : prev));
      });
      if (result.queued) return;
      setTask((prev) => (prev ? { ...prev, mel_category: category } : prev));
      flash(`MEL ${category} сохранён`);
      softBeep(660, 100);
    } catch (error) {
      noteMissing(error, "MEL недоступен.", "Ошибка MEL");
    } finally {
      setBusyAction("");
    }
  }

  async function completeTask() {
    if (task?.status && task.status !== "in_progress" && task.status !== "accepted") {
      statusNoteAt.current = Date.now();
      setNotice("Сначала примите задачу.");
      flash("Сначала примите задачу", "err");
      softBeep(320, 200);
      return;
    }
    if (!task?.tools_picked && !window.confirm("Инструмент ещё не отмечен. Всё равно завершить?")) return;
    if (!window.confirm("Завершить задачу? Отменить будет нельзя.")) return;
    setBusyAction("complete");
    try {
      const result = await runTaskAction("/api/tasks/current/complete", { method: "POST" });
      if (result.queued) {
        setStatusStep("выполнено");
        setTask(null);
        return;
      }
      flash("Задача завершена");
      softBeep(880, 160);
      setStatusStep("выполнено");
      setTask(null);
    } catch (error) {
      noteMissing(error, "Завершение недоступно.", "Ошибка завершения");
    } finally {
      setBusyAction("");
    }
  }

  async function toggleAog() {
    const next = !aogOn;
    if (!window.confirm(next ? "Поставить AOG на текущий борт?" : "Снять AOG с борта?")) return;
    setBusyAction("aog");
    try {
      const result = await runTaskAction("/api/tasks/current/aog", {
        method: "PUT",
        body: { aog: next },
      }, () => setAogOn(next));
      if (result.queued) return;
      const data = result.data;
      setAogOn(Boolean(data?.aog));
      flash(data?.aog ? "Самолёт отмечен как AOG" : "AOG снят", data?.aog ? "err" : "ok");
      softBeep(data?.aog ? 420 : 700, 140);
      try { navigator.vibrate?.(data?.aog ? [80, 40, 80] : 40); } catch { /* телефон */ }
    } catch (error) {
      noteMissing(error, "AOG недоступен.", "Ошибка AOG");
    } finally {
      setBusyAction("");
    }
  }

  async function refreshRouteFromServer() {
    if (!task?.id) return;
    setBusyAction("route");
    try {
      lastRouteRefresh.current = Date.now();
      await applyRouteRefresh(task.id);
      flash("Маршрут пересчитан на сервере");
      softBeep(600, 100);
    } catch (error) {
      noteMissing(error, "Пересчёт маршрута недоступен.", "Ошибка маршрута");
    } finally {
      setBusyAction("");
    }
  }

  function toggleGpsManual() {
    setGpsManual((prev) => {
      const next = !prev;
      localStorage.setItem("oto-gps-manual", next ? "1" : "0");
      flash(next ? "GPS включён" : "GPS выключен");
      softBeep(next ? 620 : 400, 90);
      return next;
    });
  }

  async function pushSelfPosition() {
    const current = getSession();
    const fix = gpsFixRef.current;
    if (fix?.good) {
      if (!current?.userId) {
        setNotice("Нет своей точки для отправки.");
        flash("Нет точки", "err");
        return;
      }
      setBusyAction("geo");
      const t = new Date().toISOString();
      const acc = Number.isFinite(fix.acc) ? fix.acc : null;
      const spd = Number.isFinite(fix.spd) && fix.spd >= 0 ? fix.spd : null;
      try {
        try {
          await postTrackPoint({ obj: current.userId, lat: fix.lat, lon: fix.lon, t, acc, spd });
        } catch (error) {
          if (Number(error?.status) === 404 && Array.isArray(lastSelfRef.current)) {
            const unix = Math.floor(Date.now() / 1000);
            await updateEngineerPosition(current.userId, [[lastSelfRef.current, unix]]);
          } else {
            enqueueGpsFix(current.userId, { lat: fix.lat, lon: fix.lon, t, acc, spd });
            setOutboxCount(queueDepth());
            setNotice(`Сеть недоступна, точка в очереди (${pendingCount()}). ${error.message}`);
            flash("Гео в очередь", "err");
            return;
          }
        }
        setOutboxCount(queueDepth());
        flash("Позиция отправлена");
        softBeep(500, 90);
        if (task?.id) {
          lastRouteRefresh.current = Date.now();
          try {
            await applyRouteRefresh(task.id);
            flash("Маршрут обновлён после гео");
          } catch (error) {
            console.warn("[worker] route after geo", error);
          }
        }
      } catch (error) {
        const point = lastSelfRef.current;
        const unix = Math.floor(Date.now() / 1000);
        if (Array.isArray(point)) enqueueEngineerPoint(current.userId, point, unix);
        setOutboxCount(queueDepth());
        setNotice(`Сеть недоступна, точка в очереди (${pendingCount()}). ${error.message}`);
        flash("Гео в очередь", "err");
      } finally {
        setBusyAction("");
      }
      return;
    }
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
      setOutboxCount(queueDepth());
      flash("Позиция отправлена");
      softBeep(500, 90);
      if (task?.id) {
        lastRouteRefresh.current = Date.now();
        try {
          await applyRouteRefresh(task.id);
          flash("Маршрут обновлён после гео");
        } catch (error) {
          console.warn("[worker] route after geo", error);
        }
      }
    } catch (error) {
      enqueueEngineerPoint(current.userId, point, t);
      setOutboxCount(queueDepth());
      setNotice(`Сеть недоступна, точка в очереди (${pendingCount()}). ${error.message}`);
      flash("Гео в очередь", "err");
    } finally {
      setBusyAction("");
    }
  }

  return (
    <main id="app-main" className="worker-page" tabIndex={-1}>
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
                    softBeep(ok ? 600 : 320, ok ? 80 : 160);
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
                softBeep(360, 100);
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
          {slaRisk ? <span className="worker-task-badge sla" title={slaHint || undefined}>&gt;15 мин / риск SLA</span> : null}
          {task?.arrived ? <span className="worker-task-badge">Прибыл</span> : null}
          {pollStale ? (
            <span className="worker-task-badge stale" title={lastGeoAt.current ? `Последняя гео ${new Date(lastGeoAt.current).toLocaleTimeString("ru-RU")}` : ""}>
              Нет связи{lastGeoAt.current ? ` · гео ${relativeRu(lastGeoAt.current, Date.now())}` : ""}
            </span>
          ) : null}
          {gpsWatching ? <span className="worker-task-badge gps">GPS</span> : null}
        </div>

        <StatusChain current={step} />
        <details className="shortcut-help">
          <summary>Горячие клавиши</summary>
          <ul>
            <li><kbd>A</kbd> — принять</li>
            <li><kbd>R</kbd> — отклонить</li>
            <li><kbd>T</kbd> — инструмент получен</li>
            <li><kbd>C</kbd> — завершить</li>
            <li><kbd>W</kbd> — ожидание запчасти</li>
            <li><kbd>G</kbd> — отправить гео</li>
            <li><kbd>M</kbd> — сохранить MEL</li>
            <li><kbd>O</kbd> — AOG</li>
            <li><kbd>F</kbd> — обновить задачу</li>
            <li><kbd>U</kbd> — обновить маршрут</li>
            <li><kbd>Esc</kbd> — закрыть тост</li>
          </ul>
        </details>

        {gpsWatching ? (
          <section className={`gps-panel ${gpsLink}`} role="status">
            <div className="gps-panel-main">
              <span className="gps-dot" aria-hidden="true" />
              <div>
                <strong>{gpsLink === "net" ? "Сеть" : gpsFix ? "Спутник" : "Ждём спутник"}</strong>
                <p>
                  {gpsFix
                    ? `${gpsFix.lat.toFixed(5)}, ${gpsFix.lon.toFixed(5)}`
                    : gpsLink === "net"
                      ? `±${gpsNet ?? "—"} м. На карту не ставим.`
                      : "Точка хуже 80 м не используется."}
                </p>
              </div>
            </div>
            <div className="gps-panel-meta">
              {gpsFix && gpsAcc != null ? <span>±{Math.round(gpsAcc)} м</span> : null}
              <span>
                {{
                  live: "На сервере",
                  held: "Не сдвинули",
                  queue: "В очереди",
                  legacy: "Старый сервер",
                  net: "Ждём спутник",
                  search: "Ищем спутник",
                }[gpsLink]}
              </span>
            </div>
          </section>
        ) : null}

        <section className="worker-map-card">
          <div className="section-heading">
            <p className="section-label">Маршрут на перроне</p>
            <span className="task-state">{selfText}</span>
            <span className="task-state">{distance}</span>
          </div>
          {routeHint ? <p className="fault-hint route-hint" role="status">{routeHint}</p> : null}
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
                  <div className="task-id-row">
                    <div style={{ flex: "1 1 16rem" }}>
                      <strong>{issueLabel(task.issue) || "Заявка"}</strong><br />
                      {task.description || "Описание не указано"}<br />
                      ВС {planeLabel(task)}
                    </div>
                    {canDecide ? (
                      <div className="mcc-list-actions">
                        <button id="accept-task" className="worker-action worker-action-accept" type="button" disabled={busyAction === "accept"} onClick={acceptTask} title="A" style={{ flex: "0 0 auto" }}>
                          {busyAction === "accept" ? "…" : <>Принять <span className="hotkey-mark">· A</span></>}
                        </button>
                        <button id="reject-task" className="worker-action worker-action-danger" type="button" disabled={busyAction === "reject"} onClick={rejectTask} title="R" style={{ flex: "0 0 auto" }}>
                          {busyAction === "reject" ? "…" : <>Отклонить <span className="hotkey-mark">· R</span></>}
                        </button>
                      </div>
                    ) : null}
                  </div>
                  {routeText}<br />
                  ETA: {etaClock || (minutes != null ? `${minutes} мин` : "—")}
                  {etaLeft === 0 ? " · на месте" : ""}
                  <br />Норматив 15 мин от фиксации заявки до борта. Прогноз обновляется по ходу.
                  {slaRisk ? (
                    <>
                      <br />
                      <strong className="sla-inline">&gt;15 мин / риск SLA</strong>
                      {slaHint ? <> · {slaHint}</> : null}
                    </>
                  ) : null}
                  <br />
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
                        softBeep(ok ? 600 : 320, ok ? 80 : 160);
                      }}
                    >
                      Копировать ID
                    </button>
                    {task.description ? (
                      <button
                        type="button"
                        className="ghost-copy"
                        onClick={async () => {
                          const ok = await copyText(task.description);
                          flash(ok ? "Описание скопировано" : "Не удалось скопировать", ok ? "ok" : "err");
                          softBeep(ok ? 560 : 320, ok ? 80 : 160);
                        }}
                      >
                        Копировать описание
                      </button>
                    ) : null}
                  </div>
                  <p className="section-label">MEL</p>
                  <select
                    aria-label="Категория MEL"
                    value={melCategory}
                    onChange={(event) => {
                      const next = event.target.value;
                      setMelCategory(next);
                      localStorage.setItem("oto-mel-cat", next);
                    }}
                  >
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
              ) : (
                <>
                  Назначения ждут.<br />
                  Диспетчер ещё не передал заявку. Опрос идёт автоматически.<br />
                  <span className="shortcut-inline"><kbd>F</kbd> — принудительно обновить · <kbd>G</kbd> — отправить гео.</span>
                </>
              )}
            </div>
          </article>
        </section>
        <section className="notice-card" role={/не удалось|нет связи|ошибк/i.test(notice) ? "alert" : "status"}>
          <div>{notice}</div>
        </section>
      </div>
      <div className="worker-dock" role="group" aria-label="Действия по задаче">
        <button id="save-mel" className="worker-action worker-action-secondary" type="button" hidden={!task} disabled={busyAction === "mel"} onClick={saveMel} title="M">
          {busyAction === "mel" ? "…" : <>Сохранить MEL <span className="hotkey-mark">· M</span></>}
        </button>
        <button id="tools-task" className="worker-action worker-action-secondary" type="button" hidden={!task} disabled={!task || task.tools_picked || busyAction === "tools"} onClick={pickTools} title="T">
          {task?.tools_picked ? "Инструмент ✓" : busyAction === "tools" ? "…" : <>Инструмент <span className="hotkey-mark">· T</span></>}
        </button>
        <button id="waiting-part" className="worker-action worker-action-warning" type="button" hidden={!task} disabled={busyAction === "waiting"} onClick={waitForPart} title="W">
          {busyAction === "waiting" ? "…" : <>Запчасть <span className="hotkey-mark">· W</span></>}
        </button>
        <button id="complete-task" className="worker-action worker-action-primary" type="button" hidden={!task} disabled={busyAction === "complete"} onClick={completeTask} title="C">
          {busyAction === "complete" ? "…" : <>Завершить <span className="hotkey-mark">· C</span></>}
        </button>
        <button id="aog-task" className="worker-action worker-action-danger" type="button" hidden={!task} disabled={busyAction === "aog"} onClick={toggleAog} title="O">
          {busyAction === "aog" ? "…" : aogOn ? <>Снять AOG <span className="hotkey-mark">· O</span></> : <>Поставить AOG <span className="hotkey-mark">· O</span></>}
        </button>
        <button id="refresh-route" className="worker-action worker-action-secondary" type="button" hidden={!task} disabled={busyAction === "route"} onClick={refreshRouteFromServer} title="U">
          {busyAction === "route" ? "…" : <>Маршрут <span className="hotkey-mark">· U</span></>}
        </button>
        <button
          id="refresh-task"
          className="worker-action worker-action-secondary"
          type="button"
          disabled={busyAction === "refresh"}
          title="F"
          onClick={async () => {
            setBusyAction("refresh");
            try {
              await refreshTask();
              flash("Задача обновлена");
              softBeep(560, 90);
            } catch (error) {
              setNotice(`Не удалось обновить: ${error.message}`);
              flash("Ошибка обновления", "err");
              softBeep(320, 200);
            } finally {
              setBusyAction("");
            }
          }}
        >
          {busyAction === "refresh" ? "…" : <>Обновить <span className="hotkey-mark">· F</span></>}
        </button>
        <button
          id="toggle-gps"
          className={gpsManual || gpsWatching ? "worker-action worker-action-primary" : "worker-action worker-action-secondary"}
          type="button"
          onClick={toggleGpsManual}
          title="Полевой GPS · точка уходит на /api/v1/track"
        >
          {gpsManual || gpsWatching ? "GPS · вкл" : "GPS"}
        </button>
        <button id="push-geo" className="worker-action worker-action-secondary" type="button" disabled={busyAction === "geo"} onClick={pushSelfPosition} title="G">
          {busyAction === "geo" ? "…" : outboxCount ? `Очередь ${outboxCount}` : <>Гео <span className="hotkey-mark">· G</span></>}
        </button>
      </div>
      <div
        className={toastKind === "err" ? "worker-toast err" : "worker-toast"}
        role="status"
        aria-live="polite"
        hidden={!toast}
        onClick={() => setToast("")}
        title="Esc или клик — закрыть"
      >{toast}</div>
    </main>
  );
}
