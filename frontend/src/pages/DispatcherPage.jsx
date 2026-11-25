import { useDeferredValue, useEffect, useRef, useState } from "react";
import { Navigate, useNavigate } from "react-router-dom";
import L from "leaflet";
import {
  ISSUE_REQUIRED_VEHICLE,
  engineerTypeForIssue,
  engineerTypeLabel,
  issueGroups,
  issueLabel,
  vehicleLabel,
} from "../lib/catalog.js";
import {
  assignErrorMessage,
  assignLiveRequest,
  engineerName,
  findTransportOnRoute,
  loadAircraft,
  loadAogIds,
  loadBusyEngineerIds,
  arriveAircraft,
  createDefect,
  departAircraft,
  downloadJournalCsv,
  fetchJournalCsv,
  getRoute,
  getRouteVia,
  listMaps,
  loadAlerts,
  loadAogAircraft,
  loadMovements,
  loadStaff,
  loadTransport,
  normalizeRoute,
  forceVehicle,
  pauseAgent,
  postTrackPoint,
  queueRequest,
  shiftTaskTimer,
  reassignRequest,
  setActiveMap,
  trainClassifier,
  updateEngineerPosition,
  updateTransportPosition,
} from "../lib/live.js";
import { classifyRemark } from "../lib/mccApi.js";
import { softBeep } from "../lib/beep.js";
import { copyText } from "../lib/copyText.js";
import { formatPollMs, relativeRu } from "../lib/format.js";
import NetBanner from "../components/NetBanner.jsx";
import { listMcc, markMccAssigned, markMccQueued, MCC_QUEUE_KEY } from "../lib/mccQueue.js";
import { apiRequest, clearSession, getSession, hydrateProfile } from "../lib/session.js";
import { whenVisible } from "../lib/visibility.js";
import { expandRouteOnRoads, pointAt, totalMeters, VEHICLE_MPS, WALK_MPS } from "../lib/walk.js";
import {
  buildTileGrid,
  buildWalkMask,
  cellToLatLng,
  centerToLatLng,
  clickToCell,
  addPlaceLabels,
  routeSpansMap,
  loadApronMap,
  loadTilesetImages,
  renderCanvas,
  roadHint,
} from "../lib/apronMap.js";
import { apronOverlayUrl } from "../lib/apronSvg.js";

const GROUPS = issueGroups();
const FIRST_ISSUE = GROUPS[0][1][0];
const ISSUE_LABELS = new Map(GROUPS.flatMap(([, values]) => values).map((value) => [value, issueLabel(value)]));
const LEGEND = [
  ["#0c7a56", "инженер свободен"],
  ["#c5161c", "инженер занят"],
  ["#003a70", "спецтранспорт"],
  ["#00529b", "воздушное судно"],
  ["#c5161c", "AOG"],
  ["#c5161c", "маршрут"],
];

const FACTOR_RU = {
  eta: "путь",
  late: "опоздание",
  load: "загрузка",
  aog_bonus: "AOG",
  arrival_bonus: "прилёт",
  urgency_bonus: "срочность",
  mel_bonus: "MEL",
};

function factorLine(row) {
  const factors = Array.isArray(row?.factors) ? row.factors : [];
  return factors
    .filter((item) => Number.isFinite(Number(item?.seconds)) && Math.round(Number(item.seconds)) !== 0)
    .map((item) => {
      const sec = Math.round(Number(item.seconds));
      const title = FACTOR_RU[item.name] || item.name;
      return `${title} ${sec > 0 ? "+" : ""}${sec} с`;
    })
    .join(", ");
}

async function mapCandidate(row) {
  return {
    engineerId: String(row.engineer_id ?? ""),
    name: await engineerName(row.engineer_id),
    eligible: Boolean(row.eligible),
    reason: row.reason == null ? "" : String(row.reason),
    etaSeconds: row.eta_seconds,
    usesVehicle: Boolean(row.uses_vehicle),
    factors: factorLine(row),
  };
}

function proposedEngineer(candidates, chosenId) {
  if (!Array.isArray(candidates) || !candidates.length) return null;
  const eligible = candidates.find((row) => row.eligible);
  if (eligible) return eligible;
  const chosen = chosenId ? candidates.find((row) => row.engineerId === chosenId) : null;
  return chosen || null;
}

function freshInbox() {
  // В очереди только ещё не назначенные; assigned — только после успешного /api/assign.
  return listMcc()
    .filter((item) => item.status !== "assigned")
    .sort((a, b) => {
      const urgent = Number(b.urgency === "urgent") - Number(a.urgency === "urgent");
      if (urgent) return urgent;
      return String(b.createdAt || "").localeCompare(String(a.createdAt || ""));
    });
}

function etaNote(data) {
  const seconds = Number(data?.eta_seconds);
  if (!Number.isFinite(seconds) || seconds <= 0) return "";
  return " · ETA " + Math.max(0, Math.round(seconds / 60)) + " мин";
}

function marginMinutes(seconds) {
  if (seconds == null || Number.isNaN(Number(seconds))) return null;
  return Math.round(Number(seconds) / 60);
}

const DEMO_STAND_ID = "demo-stand";
const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const STANDS_KEY = "oto-demo-stands";
const VEHICLE_IDS_KEY = "oto-demo-vehicle-ids";
const CLICK_VEHICLES = ["FuelTruck", "OilCart", "MaintenanceLift", "BorescopeCart", "AvionicsTestSet"];

function readClickMode() {
  const saved = localStorage.getItem("oto-click-mode");
  return saved === "engineer" || saved === "transport" || saved === "stand" ? saved : "stand";
}

function readDemoStands() {
  try {
    const raw = JSON.parse(localStorage.getItem(STANDS_KEY) || "[]");
    const rows = Array.isArray(raw) ? raw : [];
    return rows
      .map((row) => ({
        id: String(row?.id || DEMO_STAND_ID),
        point: Array.isArray(row?.point) && row.point.length >= 2
          ? [Number(row.point[0]), Number(row.point[1])]
          : null,
      }))
      .filter((row) => row.point && Number.isFinite(row.point[0]) && Number.isFinite(row.point[1]));
  } catch {
    return [];
  }
}

function writeDemoStand(point, id) {
  const row = { id: String(id || DEMO_STAND_ID), point: [Number(point[0]), Number(point[1])] };
  const next = [row, ...readDemoStands().filter((item) => item.id !== row.id)].slice(0, 8);
  localStorage.setItem(STANDS_KEY, JSON.stringify(next));
  return next;
}

function demoVehicleId(type) {
  let bag = {};
  try {
    const parsed = JSON.parse(localStorage.getItem(VEHICLE_IDS_KEY) || "{}");
    if (parsed && typeof parsed === "object") bag = parsed;
  } catch { /* пусто */ }
  if (!bag[type]) {
    bag[type] = crypto.randomUUID();
    localStorage.setItem(VEHICLE_IDS_KEY, JSON.stringify(bag));
  }
  return bag[type];
}

function mergePlanes(apiList, stands) {
  const extra = stands.filter((stand) => !apiList.some((plane) => plane.id === stand.id));
  return apiList.length ? [...apiList, ...extra] : stands;
}

export default function DispatcherPage() {
  const navigate = useNavigate();
  const session = getSession();
  const allowed = session?.role === "Dispatcher";
  const mapNode = useRef(null);
  const actions = useRef({});
  const formRef = useRef({});
  const clickRef = useRef({});
  const layersRef = useRef((() => {
    try {
      const saved = JSON.parse(localStorage.getItem("oto-map-layers") || "null");
      if (saved && typeof saved === "object") {
        return {
          staff: saved.staff !== false,
          transport: saved.transport !== false,
          stands: saved.stands !== false,
          route: saved.route !== false,
        };
      }
    } catch { /* дефолт */ }
    return { staff: true, transport: true, stands: true, route: true };
  })());
  const lastAuto = useRef("");
  const skipAuto = useRef(false);
  const cardPreview = useRef("");
  const alertCountRef = useRef(0);
  const inboxCountRef = useRef(0);

  const [aircraft, setAircraft] = useState([]);
  const [demoStands, setDemoStands] = useState(readDemoStands);
  const [clickMode, setClickMode] = useState(readClickMode);
  const [staffRows, setStaffRows] = useState([]);
  const [clickEngineerId, setClickEngineerId] = useState(() => localStorage.getItem("oto-click-eng") || "");
  const [clickVehicleType, setClickVehicleType] = useState(() => {
    const saved = localStorage.getItem("oto-click-vehicle");
    return CLICK_VEHICLES.includes(saved) ? saved : "FuelTruck";
  });
  const [aogIds, setAogIds] = useState(() => new Set());
  const [planeId, setPlaneId] = useState(() => readDemoStands()[0]?.id || "");
  const [issue, setIssue] = useState(() => {
    const saved = localStorage.getItem("oto-last-issue");
    return saved && ISSUE_LABELS.has(saved) ? saved : FIRST_ISSUE;
  });
  const [description, setDescription] = useState(() => sessionStorage.getItem("oto-disp-draft") || "");
  const [hint, setHint] = useState("");
  const [live, setLive] = useState("Загрузка бортов, инженеров и техники…");
  // #6: причина сбоя загрузки списка инженеров (пустая строка — сбоя нет).
  const [staffError, setStaffError] = useState("");
  const [result, setResult] = useState({ type: "info", text: "Клик по карте ставит стоянку, инженера или технику. Затем назначьте." });
  const [inbox, setInbox] = useState(freshInbox);
  const [activeMcc, setActiveMcc] = useState(null);
  const [tileInfo, setTileInfo] = useState("Кликни по карте, чтобы выбрать клетку.");
  const [pending, setPending] = useState(false);
  const [alerts, setAlerts] = useState([]);
  const [alertsNote, setAlertsNote] = useState("");
  const [journalNote, setJournalNote] = useState("");
  const [journalPending, setJournalPending] = useState(false);
  const [journalPreview, setJournalPreview] = useState("");
  const [journalPreviewErr, setJournalPreviewErr] = useState("");
  const [reassignId, setReassignId] = useState(() => sessionStorage.getItem("oto-reassign-id") || "");
  const [reassignEngineer, setReassignEngineer] = useState(() => sessionStorage.getItem("oto-reassign-eng") || "");
  const [reassignReason, setReassignReason] = useState(() => sessionStorage.getItem("oto-reassign-reason") || "");
  const [reassignNote, setReassignNote] = useState("");
  const [reassignPending, setReassignPending] = useState(false);
  const [queueNote, setQueueNote] = useState("");
  const [queuePending, setQueuePending] = useState("");
  const [defectText, setDefectText] = useState(() => sessionStorage.getItem("oto-defect-text") || "");
  const [defectAta, setDefectAta] = useState(() => sessionStorage.getItem("oto-defect-ata") || "");
  const [defectNote, setDefectNote] = useState("");
  const [defectPending, setDefectPending] = useState(false);
  const [classifyHint, setClassifyHint] = useState("");
  const [classifyPending, setClassifyPending] = useState(false);
  const [trainNote, setTrainNote] = useState("");
  const [trainPending, setTrainPending] = useState(false);
  const [departNote, setDepartNote] = useState("");
  const [departPending, setDepartPending] = useState(false);
  const [arriveNote, setArriveNote] = useState("");
  const [arrivePending, setArrivePending] = useState(false);
  const [routeNote, setRouteNote] = useState("");
  const [routePending, setRoutePending] = useState(false);
  const [fleet, setFleet] = useState([]);
  const [transportId, setTransportId] = useState(() => sessionStorage.getItem("oto-transport-id") || "");
  const [transportX, setTransportX] = useState(() => sessionStorage.getItem("oto-transport-x") || "");
  const [transportY, setTransportY] = useState(() => sessionStorage.getItem("oto-transport-y") || "");
  const [transportNote, setTransportNote] = useState("");
  const [transportPending, setTransportPending] = useState(false);
  const [mapCatalog, setMapCatalog] = useState([]);
  const [activeMapId, setActiveMapId] = useState("");
  const [mapNote, setMapNote] = useState("");
  const [mapPending, setMapPending] = useState(false);
  const [aogList, setAogList] = useState([]);
  const [trackObj, setTrackObj] = useState(() => localStorage.getItem("oto-track-obj") || "e-01");
  const [trackLat, setTrackLat] = useState(() => localStorage.getItem("oto-track-lat") || "55.8304");
  const [trackLon, setTrackLon] = useState(() => localStorage.getItem("oto-track-lon") || "37.6310");
  const [trackAcc, setTrackAcc] = useState(() => localStorage.getItem("oto-track-acc") || "8");
  const [trackNote, setTrackNote] = useState("");
  const [examNote, setExamNote] = useState("");
  const [examBusy, setExamBusy] = useState("");
  const [pauseObj, setPauseObj] = useState("");
  const [forceId, setForceId] = useState("");
  const [timerId, setTimerId] = useState("");
  const [timerDelta, setTimerDelta] = useState("-300");
  const [trackPending, setTrackPending] = useState(false);
  const [movements, setMovements] = useState([]);
  const [layersOn, setLayersOn] = useState(layersRef.current);
  const [planeFilter, setPlaneFilter] = useState("");
  const deferredPlaneFilter = useDeferredValue(planeFilter);
  const [staleLive, setStaleLive] = useState(false);
  const [crewNote, setCrewNote] = useState("");
  const [pollMs, setPollMs] = useState(null);
  const [assignHistory, setAssignHistory] = useState(() => {
    try {
      const rows = JSON.parse(sessionStorage.getItem("oto-assign-history") || "[]");
      return Array.isArray(rows) ? rows : [];
    } catch {
      return [];
    }
  });
  const [urgentOnly, setUrgentOnly] = useState(() => localStorage.getItem("oto-urgent-only") === "1");
  const lastLiveAt = useRef(Date.now());
  const staleBeepAt = useRef(0);

  formRef.current = { issue, description, planeId, activeMcc };
  clickRef.current = { mode: clickMode, engineerId: clickEngineerId, vehicleType: clickVehicleType, planeId };
  layersRef.current = layersOn;

  useEffect(() => {
    localStorage.setItem("oto-map-layers", JSON.stringify(layersOn));
  }, [layersOn]);

  useEffect(() => {
    if (inbox.length > inboxCountRef.current) softBeep(660, 160);
    inboxCountRef.current = inbox.length;
  }, [inbox.length]);

  useEffect(() => {
    localStorage.setItem("oto-last-issue", issue);
    const vehicleType = ISSUE_REQUIRED_VEHICLE[issue];
    const vehicleText = vehicleType ? `Спецтранспорт: ${vehicleLabel(vehicleType)}.` : "Спецтранспорт не требуется.";
    setHint(`Нужный специалист: ${engineerTypeForIssue(issue)}. ${vehicleText}`);
    if (skipAuto.current) {
      skipAuto.current = false;
      return;
    }
    const nextAuto = `Обнаружена неисправность: ${issueLabel(issue).toLowerCase()}. Требуется осмотр и устранение.`;
    setDescription((current) => {
      if (!current.trim() || current === lastAuto.current) {
        lastAuto.current = nextAuto;
        sessionStorage.setItem("oto-disp-draft", nextAuto);
        return nextAuto;
      }
      return current;
    });
  }, [issue]);

  useEffect(() => {
    const onStorage = (event) => {
      if (event.key === MCC_QUEUE_KEY) setInbox(freshInbox());
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);

  const [clock, setClock] = useState(() => new Date().toLocaleTimeString("ru-RU"));
  const [nowTick, setNowTick] = useState(() => Date.now());

  useEffect(() => {
    if (!allowed) return undefined;
    const timer = window.setInterval(() => {
      hydrateProfile().catch(() => {});
    }, 60000);
    return () => clearInterval(timer);
  }, [allowed]);

  useEffect(() => {
    if (!allowed) return undefined;
    loadJournalPreview();
    return undefined;
  }, [allowed]);

  useEffect(() => {
    const timer = window.setInterval(() => {
      setClock(new Date().toLocaleTimeString("ru-RU"));
      setNowTick(Date.now());
    }, 1000);
    return () => clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!allowed) return undefined;
    const timer = window.setInterval(() => {
      const stale = Date.now() - lastLiveAt.current > 20000;
      setStaleLive(stale);
      if (stale) {
        const now = Date.now();
        if (now - staleBeepAt.current > 45000) {
          staleBeepAt.current = now;
          softBeep(300, 180);
        }
      }
    }, 4000);
    return () => clearInterval(timer);
  }, [allowed]);

  useEffect(() => {
    const q = deferredPlaneFilter.trim().toLowerCase();
    const pool = aircraft.length ? aircraft : demoStands;
    if (!q || !pool.length) return;
    const list = pool.filter((plane) => {
      const hay = `${plane.id} ${plane.point?.[0]} ${plane.point?.[1]}`.toLowerCase();
      return hay.includes(q);
    });
    if (list.length && !list.some((plane) => plane.id === planeId)) {
      setPlaneId(list[0].id);
    }
  }, [deferredPlaneFilter, aircraft, demoStands, planeId]);

  useEffect(() => {
    const bits = ["ОТО · смена"];
    if (inbox.length) bits.push(`очередь ${inbox.length}`);
    if (alerts.length) bits.push(`риски ${alerts.length}`);
    if (aogList.length) bits.push(`AOG ${aogList.length}`);
    document.title = bits.join(" · ");
    return () => { document.title = "Аэрофлот ОТО"; };
  }, [alerts.length, inbox.length, aogList.length]);

  useEffect(() => {
    if (!allowed) return undefined;
    function onKey(event) {
      if (window.matchMedia("(max-width: 860px), (pointer: coarse)").matches) return;
      if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
        event.preventDefault();
        const form = formRef.current;
        if (!form?.planeId) return;
        actions.current.assign?.();
      }
      if (event.key === "Escape") {
        setActiveMcc(null);
        setPlaneFilter("");
        setHint("");
        setRouteNote("");
        setDepartNote("");
        setArriveNote("");
        setDefectNote("");
        setTrainNote("");
        setReassignNote("");
        setQueueNote("");
        setAlertsNote("");
        setJournalNote("");
        setTrackNote("");
        setTransportNote("");
        setMapNote("");
        setClassifyHint("");
      }
      if ((event.ctrlKey || event.metaKey) && (event.key === "k" || event.key === "K" || event.key === "л" || event.key === "Л")) {
        event.preventDefault();
        document.getElementById("plane-filter")?.focus();
      }
      if ((event.ctrlKey || event.metaKey) && (event.key === "u" || event.key === "U" || event.key === "г" || event.key === "Г")) {
        event.preventDefault();
        setUrgentOnly((prev) => {
          const next = !prev;
          localStorage.setItem("oto-urgent-only", next ? "1" : "0");
          softBeep(next ? 700 : 480, 80);
          return next;
        });
      }
      if ((event.ctrlKey || event.metaKey) && (event.key === "j" || event.key === "J" || event.key === "о" || event.key === "О")) {
        event.preventDefault();
        document.getElementById("download-journal")?.click();
      }
      if (!event.ctrlKey && !event.metaKey && !event.altKey && /^[1-4]$/.test(event.key)) {
        if (event.target && /^(INPUT|TEXTAREA|SELECT)$/.test(event.target.tagName)) return;
        const names = ["staff", "transport", "stands", "route"];
        const name = names[Number(event.key) - 1];
        if (!name) return;
        event.preventDefault();
        setLayersOn((prev) => {
          const next = { ...prev, [name]: !prev[name] };
          actions.current.setLayer?.(name, next[name]);
          softBeep(next[name] ? 620 : 400, 70);
          return next;
        });
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [allowed]);

  useEffect(() => {
    if (!allowed) return undefined;
    let cancelled = false;
    // Риск норматива и прибытия ВС, пока диспетчер на экране.
    async function pullSide() {
      try {
        const [list, moves, transport, maps, aog] = await Promise.all([
          loadAlerts(),
          loadMovements(12),
          loadTransport(),
          listMaps().catch(() => null),
          loadAogAircraft(),
        ]);
        if (cancelled) return;
        const named = await Promise.all(list.map(async (item) => {
          const id = item.alternative_engineer_id;
          if (!id) return item;
          const name = await engineerName(id);
          const message = String(item.message || "").split(String(id)).join(name);
          return { ...item, message, alternativeName: name };
        }));
        if (cancelled) return;
        setAlerts(named);
        if (list.length > alertCountRef.current) softBeep(880, 180);
        alertCountRef.current = list.length;
        setMovements(moves);
        setFleet(transport);
        setAogList(aog);
        if (maps) {
          setMapCatalog(Array.isArray(maps.maps) ? maps.maps : []);
          setActiveMapId((prev) => prev || maps.active_id || "");
        }
        if (!transportId && transport[0]?.id) {
          setTransportId(transport[0].id);
          sessionStorage.setItem("oto-transport-id", transport[0].id);
          if (transport[0].point) {
            setTransportX(String(transport[0].point[0]));
            setTransportY(String(transport[0].point[1]));
            sessionStorage.setItem("oto-transport-x", String(transport[0].point[0]));
            sessionStorage.setItem("oto-transport-y", String(transport[0].point[1]));
          }
        }
        setAlertsNote("");
      } catch (error) {
        console.warn("[alerts/movements]", error);
        if (!cancelled) setAlertsNote("Список рисков или движений недоступен.");
      }
    }
    const stop = whenVisible(() => {
      if (!cancelled) pullSide();
    }, 15000);
    return () => {
      cancelled = true;
      stop();
    };
  }, [allowed]);

  useEffect(() => {
    if (!allowed) return undefined;
    let cancelled = false;
    const walks = new Map();
    const rides = new Map();
    const bag = { map: null, frame: 0, timer: 0, cleanupResize: null };
    let fleet = [];
    let aircraftNow = [];
    let agents = [];
    let busyIds = new Set();
    let lastAog = new Set();
    let showRoute = layersRef.current.route;

    (async () => {
      const tmj = await loadApronMap();
      if (cancelled || !mapNode.current) return;
      const walkMask = buildWalkMask(tmj);
      const grid = buildTileGrid(tmj);
      const tilesets = await loadTilesetImages();
      if (cancelled || !mapNode.current) return;

      const bounds = [[0, 0], [tmj.height * tmj.tileheight, tmj.width * tmj.tilewidth]];
      const map = L.map(mapNode.current, {
        crs: L.CRS.Simple,
        minZoom: -4,
        maxZoom: 6,
        zoomSnap: 0.25,
        attributionControl: false,
      });
      bag.map = map;
      L.imageOverlay(apronOverlayUrl(tmj) || renderCanvas(tmj, grid, tilesets).toDataURL("image/png"), bounds).addTo(map);
      addPlaceLabels(map, tmj);
      let apronFitted = false;
      const fitApron = () => {
        const node = mapNode.current;
        if (!node || node.clientWidth < 2 || node.clientHeight < 2) return;
        map.invalidateSize();
        if (apronFitted) return;
        map.fitBounds(bounds, { padding: [16, 16] });
        apronFitted = true;
      };
      fitApron();
      requestAnimationFrame(fitApron);

      const standsLayer = L.layerGroup().addTo(map);
      const staffLayer = L.layerGroup().addTo(map);
      const transportLayer = L.layerGroup().addTo(map);
      const routeExtrasLayer = L.layerGroup().addTo(map);
      const layerOf = { staff: staffLayer, transport: transportLayer, stands: standsLayer, route: routeExtrasLayer };
      for (const [name, layer] of Object.entries(layerOf)) {
        if (!layersRef.current[name]) map.removeLayer(layer);
      }

      const onResize = () => {
        map.invalidateSize();
        if (!apronFitted) fitApron();
      };
      window.addEventListener("resize", onResize);
      bag.cleanupResize = () => window.removeEventListener("resize", onResize);

      function staffMarker(latlng, busy, label) {
        return L.marker(latlng, {
          icon: L.divIcon({
            className: `agent-marker ${busy ? "agent-busy" : "agent-free"}`,
            html: "",
            iconSize: [28, 28],
            iconAnchor: [14, 14],
          }),
        }).bindTooltip(label, { direction: "top" }).addTo(staffLayer);
      }

      function stopWalk(id) {
        const walk = walks.get(id);
        if (!walk) return;
        if (walk.line) map.removeLayer(walk.line);
        if (walk.marker) staffLayer.removeLayer(walk.marker);
        walks.delete(id);
      }

      function cellCenters(route) {
        return route.map(([x, y]) => [x + 0.5, y + 0.5]);
      }

      function startWalk(id, name, gridRoute) {
        const key = String(id);
        stopWalk(key);
        const roadRoute = expandRouteOnRoads(normalizeRoute(gridRoute), walkMask);
        const centers = roadRoute.map(([x, y]) => [x + 0.5, y + 0.5]);
        if (centers.length < 2) return;
        const line = L.polyline(centers.map((center) => centerToLatLng(tmj, center)), {
          color: "#c5161c",
          weight: 5,
          opacity: 0.9,
          dashArray: "10 8",
        });
        if (showRoute) line.addTo(map);
        const marker = staffMarker(centerToLatLng(tmj, centers[0]), true, `${name} · идёт к ВС`);
        walks.set(key, { centers, startedAt: performance.now(), line, marker, name });
      }

      function rideCell(ride) {
        const sample = pointAt(ride.centers, ((performance.now() - ride.startedAt) / 1000) * ride.mps);
        if (!sample) return ride.target;
        return [Math.floor(sample.point[0]), Math.floor(sample.point[1])];
      }

      function beginGlide(vehicle, from, to, marker) {
        const road = expandRouteOnRoads([from, to], walkMask);
        const path = road.length >= 2 ? road : [from, to];
        const centers = cellCenters(path);
        const meters = totalMeters(centers);
        const natural = meters / VEHICLE_MPS;
        const duration = Math.min(Math.max(natural, 0.35), 4);
        rides.set(String(vehicle.id), {
          centers,
          target: to,
          mps: duration > 0 ? meters / duration : VEHICLE_MPS,
          startedAt: performance.now(),
          marker,
          label: vehicle.label,
          done: false,
        });
        marker.setTooltipContent(`${vehicle.label} · едет`);
        marker.addTo(transportLayer);
      }

      function tick(now) {
        for (const walk of walks.values()) {
          if (walk.done || !walk.line) continue;
          const traveled = ((now - walk.startedAt) / 1000) * WALK_MPS;
          const sample = pointAt(walk.centers, traveled);
          if (!sample) continue;
          // followGps: маркер держит клетку с сервера, линия хода остаётся.
          if (!walk.followGps) {
            walk.marker.setLatLng(centerToLatLng(tmj, sample.done ? walk.centers.at(-1) : sample.point));
          }
          if (sample.done) {
            if (map.hasLayer(walk.line)) map.removeLayer(walk.line);
            walk.line = null;
            walk.marker.setTooltipContent(`${walk.name} · на месте`);
            walk.done = true;
            continue;
          }
          const rest = walk.centers.slice(sample.nextIndex).map((center) => centerToLatLng(tmj, center));
          walk.line.setLatLngs([centerToLatLng(tmj, sample.point), ...rest]);
        }
        for (const ride of rides.values()) {
          if (ride.done) continue;
          const sample = pointAt(ride.centers, ((now - ride.startedAt) / 1000) * ride.mps);
          if (!sample) continue;
          ride.marker.setLatLng(centerToLatLng(tmj, sample.point));
          if (!sample.done) continue;
          ride.done = true;
          ride.marker.setTooltipContent(ride.label);
        }
        bag.frame = requestAnimationFrame(tick);
      }
      bag.frame = requestAnimationFrame(tick);

      function renderAircraftMarkers(list, aog) {
        standsLayer.clearLayers();
        for (const plane of list) {
          const isAog = aog.has(plane.id);
          L.marker(cellToLatLng(tmj, plane.point[0], plane.point[1]), {
            icon: L.divIcon({
              className: isAog ? "aircraft-marker aog" : "aircraft-marker",
              html: "",
              iconSize: [16, 16],
              iconAnchor: [8, 8],
            }),
          }).bindTooltip(`${isAog ? "AOG · " : ""}ВС ${plane.id.slice(0, 8)}`, {
            permanent: true,
            direction: "top",
            className: "map-label",
            offset: [0, -6],
          }).addTo(standsLayer);
        }
      }

      function renderStaffMarkers() {
        staffLayer.clearLayers();
        const seen = new Set();
        for (const agent of agents) {
          const walk = walks.get(String(agent.id));
          if (walk) {
            const cell = agent.point;
            if (Array.isArray(cell) && cell.length >= 2 && Number.isFinite(cell[0]) && Number.isFinite(cell[1])) {
              walk.marker.setLatLng(cellToLatLng(tmj, cell[0], cell[1]));
              walk.followGps = true;
            }
            walk.marker.addTo(staffLayer);
            seen.add(String(agent.id));
            continue;
          }
          const statusText = agent.busy ? "занят на задаче" : "свободен";
          staffMarker(cellToLatLng(tmj, agent.point[0], agent.point[1]), agent.busy, `${agent.name} · ${statusText}`);
        }
        for (const [id, walk] of walks) {
          if (!seen.has(id)) walk.marker.addTo(staffLayer);
        }
      }

      const transportMarkers = new Map();

      function renderTransportMarkers() {
        const seen = new Set();
        for (const vehicle of fleet) {
          const id = String(vehicle.id);
          seen.add(id);
          const cell = [Math.floor(vehicle.point[0]), Math.floor(vehicle.point[1])];
          const held = transportMarkers.get(id);
          if (!held) {
            const marker = L.marker(cellToLatLng(tmj, cell[0], cell[1]), {
              icon: L.divIcon({
                className: "transport-marker",
                html: "",
                iconSize: [18, 18],
                iconAnchor: [9, 9],
              }),
            }).bindTooltip(vehicle.label, {
              permanent: true,
              direction: "top",
              className: "map-label",
              offset: [0, -8],
            }).addTo(transportLayer);
            transportMarkers.set(id, { marker, cell });
            continue;
          }
          const moved = held.cell[0] !== cell[0] || held.cell[1] !== cell[1];
          if (moved) {
            const ride = rides.get(id);
            const from = ride && !ride.done ? rideCell(ride) : held.cell;
            beginGlide(vehicle, from, cell, held.marker);
            held.cell = cell;
            continue;
          }
          held.marker.addTo(transportLayer);
        }
        for (const [id, held] of transportMarkers) {
          if (seen.has(id)) continue;
          transportLayer.removeLayer(held.marker);
          transportMarkers.delete(id);
          rides.delete(id);
        }
      }

      async function refresh() {
        const started = performance.now();
        const staff = await loadStaff();
        if (cancelled) return;
        agents = staff.plotted;
        setStaffError(staff.error?.message || "");
        aircraftNow = await loadAircraft();
        const standsNow = readDemoStands();
        const shown = mergePlanes(aircraftNow, standsNow);
        const aog = await loadAogIds();
        fleet = await loadTransport();
        busyIds = await loadBusyEngineerIds();
        if (cancelled) return;
        for (const agent of agents) agent.busy = busyIds.has(String(agent.id));
        lastAog = aog;
        const rows = [
          ...staff.plotted.map((agent) => ({ id: agent.id, name: agent.name, point: agent.point })),
          ...staff.missing.map((agent) => ({ id: agent.id, name: agent.name, point: null })),
        ];
        setStaffRows(rows);
        setClickEngineerId((prev) => (rows.some((row) => row.id === prev) ? prev : rows[0]?.id || ""));
        setAircraft(aircraftNow);
        setDemoStands(standsNow);
        setAogIds(aog);
        setPlaneId((prev) => (shown.some((plane) => plane.id === prev) ? prev : shown[0]?.id || ""));
        renderAircraftMarkers(shown, aog);
        renderStaffMarkers();
        renderTransportMarkers();
        const gap = staff.missing.length ? `, без координат: ${staff.missing.length}` : "";
        lastLiveAt.current = Date.now();
        setStaleLive(false);
        setPollMs(Math.round(performance.now() - started));
        const free = agents.filter((agent) => !agent.busy).length;
        setCrewNote(`Свободно ${free} / ${agents.length}`);
        const standNote = aircraftNow.length ? "" : ` · стоянок: ${standsNow.length}`;
        setLive(`Бортов: ${aircraftNow.length}${standNote} · инженеров на карте: ${agents.length}${gap} · техники: ${fleet.length} · AOG: ${aog.size}`);
      }

      async function placeClick(cell) {
        const point = [cell.col, cell.row];
        const mode = clickRef.current.mode;
        if (mode === "engineer") {
          const id = clickRef.current.engineerId;
          if (!id) return "Выберите инженера в списке";
          const sessionId = getSession()?.userId;
          const own = Boolean(sessionId) && String(sessionId) === String(id);
          const stamp = Math.floor(Date.now() / 1000);
          try {
            await updateEngineerPosition(own ? sessionId : id, [[point, stamp]]);
          } catch (error) {
            // Чужого инженера правит только он сам (403). /api/simulate удалён из
            // бэкенда, поэтому запасного адреса больше нет — показываем причину.
            if (own || Number(error?.status) !== 403) throw error;
            throw new Error("Точку чужого инженера ставит только он сам (403).");
          }
          await refresh();
          return `Инженер · [${point[0]}, ${point[1]}]`;
        }
        if (mode === "transport") {
          const type = CLICK_VEHICLES.includes(clickRef.current.vehicleType) ? clickRef.current.vehicleType : "FuelTruck";
          const id = demoVehicleId(type);
          await updateTransportPosition(id, type, point);
          fleet = await loadTransport();
          setFleet(fleet);
          renderTransportMarkers();
          return `${vehicleLabel(type)} · [${point[0]}, ${point[1]}]`;
        }
        const known = readDemoStands();
        const selected = clickRef.current.planeId;
        const id = known.some((stand) => stand.id === selected) ? selected : (known[0]?.id || DEMO_STAND_ID);
        const stands = writeDemoStand(point, id);
        setDemoStands(stands);
        setPlaneId(id);
        renderAircraftMarkers(mergePlanes(aircraftNow, stands), lastAog);
        return `Стоянка ${String(id).slice(0, 8)} · [${point[0]}, ${point[1]}]`;
      }

      map.on("click", (event) => {
        const cell = clickToCell(tmj, event.latlng.lat, event.latlng.lng);
        if (!cell) {
          setTileInfo("Клик вне карты.");
          return;
        }
        // Клетка клика — в форму сдвига техники (не GPS Logger).
        setTransportX(String(cell.col));
        setTransportY(String(cell.row));
        sessionStorage.setItem("oto-transport-x", String(cell.col));
        sessionStorage.setItem("oto-transport-y", String(cell.row));
        const cells = grid[cell.row][cell.col];
        const hintText = roadHint(cells);
        const lines = cells.map((item) => {
          const flip = item.flippedH || item.flippedV || item.flippedD
            ? ` (flip h:${item.flippedH} v:${item.flippedV} d:${item.flippedD})`
            : "";
          return `${item.layerName}: gid=${item.gid}${flip}`;
        });
        const detail = cells.length
          ? `Point [${cell.col}, ${cell.row}]${hintText}\n${lines.join("\n")}`
          : `Point [${cell.col}, ${cell.row}]${hintText}`;
        setTileInfo(detail);
        placeClick(cell).then((note) => {
          if (note) setTileInfo(`${note}\n${detail}`);
        }).catch((error) => {
          setTileInfo(`${detail}\n${error.message || "Не удалось поставить точку"}`);
        });
      });

      actions.current = {
        setLayer(name, on) {
          const layer = layerOf[name];
          if (!layer) return;
          if (name === "route") showRoute = on;
          if (on) layer.addTo(map);
          else map.removeLayer(layer);
          if (name !== "route") return;
          for (const walk of walks.values()) {
            if (!walk.line) continue;
            if (on) walk.line.addTo(map);
            else map.removeLayer(walk.line);
          }
        },
        previewRoute(gridRoute, label = "Проба getRoute") {
          routeExtrasLayer.clearLayers();
          const route = expandRouteOnRoads(normalizeRoute(gridRoute), walkMask);
          if (route.length < 2) return null;
          const centers = route.map(([x, y]) => [x + 0.5, y + 0.5]);
          const line = L.polyline(centers.map((center) => centerToLatLng(tmj, center)), {
            color: "#00529b",
            weight: 4,
            opacity: 0.85,
            dashArray: "8 6",
          }).addTo(routeExtrasLayer);
          line.bindTooltip(label, { sticky: true });
          if (!showRoute) map.removeLayer(routeExtrasLayer);
          else if (routeSpansMap(tmj, line.getBounds())) map.fitBounds(line.getBounds(), { padding: [80, 80], maxZoom: 3 });
          return line;
        },
        async assign(engineerId) {
          const form = formRef.current;
          const standsNow = readDemoStands();
          const stand = standsNow.find((item) => item.id === form.planeId)
            || (!aircraftNow.length ? standsNow[0] : null);
          const plane = stand?.point
            ? stand
            : (aircraftNow.find((item) => item.id === form.planeId) || null);
          if (!plane?.point) {
            setResult({ type: "info", text: "Поставьте стоянку ВС кликом по карте." });
            return;
          }
          if (navigator.onLine === false) {
            setResult({ type: "error", text: "Нет сети — назначение невозможно, пока не появится связь." });
            softBeep(280, 220);
            return;
          }
          const issueValue = form.issue;
          const text = String(form.description || "").trim() || "Замечание";
          const busyBefore = new Set(busyIds);
          setPending(true);
          setResult({ type: "info", text: "Поиск свободного инженера и спецтранспорта..." });
          try {
            const mccItem = form.activeMcc
              ? listMcc().find((item) => item.id === form.activeMcc)
              : null;
            const requestId = mccItem?.serverId && mccItem.status !== "assigned"
              ? String(mccItem.serverId)
              : "";
            const planeUuid = [plane.id, mccItem?.planeUuid]
              .map((value) => String(value || "").trim())
              .find((value) => UUID_RE.test(value)) || "";
            const manualId = typeof engineerId === "string" && UUID_RE.test(engineerId.trim())
              ? engineerId.trim()
              : "";
            const response = await assignLiveRequest({
              text,
              point: plane.point,
              planeUuid,
              issue: issueValue,
              engineerId: manualId,
              requestId,
            });
            if (mccItem && requestId) markMccQueued(mccItem.id);
            const gridRoute = normalizeRoute(response.route);
            const travelMin = marginMinutes(response.time ?? response.eta_seconds) ?? 0;
            // Статус «назначена» только после HTTP 200, не по клику и не по очереди.
            if (form.activeMcc) {
              markMccAssigned(form.activeMcc);
              setActiveMcc(null);
              setInbox(freshInbox());
            }
            const assignedName = await engineerName(response.engineer_uuid);
            routeExtrasLayer.clearLayers();
            const pickup = findTransportOnRoute(gridRoute, fleet);
            startWalk(response.engineer_uuid, assignedName, gridRoute);
            const routeLine = walks.get(String(response.engineer_uuid))?.line || null;
            if (pickup) {
              L.marker(cellToLatLng(tmj, pickup.point[0], pickup.point[1]), {
                icon: L.divIcon({
                  className: "transport-pickup-marker",
                  html: "",
                  iconSize: [16, 16],
                  iconAnchor: [8, 8],
                }),
              }).bindTooltip(`Заезд: ${pickup.label}`, {
                permanent: true,
                direction: "top",
                className: "map-label",
                offset: [0, -8],
              }).addTo(routeExtrasLayer);
            }
            if (!showRoute) map.removeLayer(routeExtrasLayer);
            await refresh();
            const skipped = [...busyBefore].filter((id) => id !== String(response.engineer_uuid)).length;
            // На старом ответе candidates нет — строки не собираем.
            const chosenId = String(response.engineer_uuid ?? "");
            const rawCandidates = Array.isArray(response.candidates) ? response.candidates : [];
            const candidates = await Promise.all(rawCandidates.map((row) => mapCandidate(row)));
            candidates.sort((a, b) => {
              if (a.engineerId === chosenId) return -1;
              if (b.engineerId === chosenId) return 1;
              if (a.eligible !== b.eligible) return Number(b.eligible) - Number(a.eligible);
              return Number(a.etaSeconds ?? 1e9) - Number(b.etaSeconds ?? 1e9);
            });
            setResult({
              type: "ok",
              name: assignedName,
              plane: plane.id.slice(0, 8),
              issue: issueLabel(issueValue),
              onSite: gridRoute.length < 2,
              cells: Math.max(0, gridRoute.length - 1),
              minutes: travelMin,
              pickup: pickup ? pickup.label : "",
              needed: pickup ? "" : (ISSUE_REQUIRED_VEHICLE[issueValue] ? vehicleLabel(ISSUE_REQUIRED_VEHICLE[issueValue]) : ""),
              skipped,
              sla: Boolean(response.time_limit_exceeded ?? response.sla_exceeded),
              chosenId,
              candidates,
              candidateMs: response.candidate_ms,
            });
            softBeep(response.time_limit_exceeded || response.sla_exceeded ? 420 : 880, 160);
            sessionStorage.removeItem("oto-disp-draft");
            {
              const nextAuto = `Обнаружена неисправность: ${issueLabel(issueValue).toLowerCase()}. Требуется осмотр и устранение.`;
              lastAuto.current = nextAuto;
              sessionStorage.setItem("oto-disp-draft", nextAuto);
              setDescription(nextAuto);
            }
            {
              const entry = {
                at: new Date().toISOString(),
                name: assignedName,
                plane: plane.id.slice(0, 8),
                planeId: plane.id,
                issue: issueValue,
                description: String(form.description || "").trim(),
                minutes: travelMin,
                sla: Boolean(response.time_limit_exceeded ?? response.sla_exceeded),
              };
              setAssignHistory((prev) => {
                const next = [entry, ...prev].slice(0, 8);
                sessionStorage.setItem("oto-assign-history", JSON.stringify(next));
                return next;
              });
            }
            if (routeLine && routeSpansMap(tmj, routeLine.getBounds())) map.fitBounds(routeLine.getBounds(), { padding: [80, 80], maxZoom: 3 });
          } catch (error) {
            setResult({ type: "error", text: `Ошибка: ${assignErrorMessage(error)}` });
          } finally {
            setPending(false);
          }
        },
      };

      await refresh();
      if (cancelled) return;
      bag.timer = window.setInterval(() => {
        refresh().catch((error) => console.warn("[map] обновление", error));
      }, 5000);
    })().catch((error) => {
      if (!cancelled) setTileInfo(`Ошибка: ${error.message}`);
    });

    return () => {
      cancelled = true;
      cancelAnimationFrame(bag.frame);
      clearInterval(bag.timer);
      bag.cleanupResize?.();
      bag.map?.remove();
      actions.current = {};
    };
  }, [allowed]);

  if (!allowed) return <Navigate to="/" replace />;

  const planeOptions = mergePlanes(aircraft, demoStands);
  const selectedStand = planeOptions.find((plane) => plane.id === planeId) || null;

  function chooseMcc(item) {
    const text = String(item.description || "").trim();
    const stand = readDemoStands().find((row) => row.id === item.planeUuid);
    skipAuto.current = true;
    softBeep(item.urgency === "urgent" ? 780 : 560, 100);
    setActiveMcc(item.id);
    if (stand) setPlaneId(stand.id);
    if (item.issue) setIssue(item.issue);
    setDescription(text);
    sessionStorage.setItem("oto-disp-draft", text);
    if (item.serverId) setReassignId(String(item.serverId));
    cardPreview.current = item.id;
    setResult({
      type: "info",
      text: item.serverId
        ? "Заявка открыта. Считаю, кого назначить."
        : "Замечание подставлено в заявку. Можно назначать без очереди.",
      candidates: [],
      chosenId: "",
    });
    if (item.serverId) void loadCardCandidates(item);
    window.queueMicrotask?.(() => {
      const btn = document.getElementById("assign-button");
      btn?.scrollIntoView({ block: "nearest", behavior: "smooth" });
      btn?.focus();
    });
  }

  async function loadCardCandidates(item) {
    try {
      const data = await apiRequest("/api/assign/candidates", {
        method: "POST",
        body: { request_id: String(item.serverId) },
      });
      if (cardPreview.current !== item.id) return;
      const raw = Array.isArray(data?.candidates) ? data.candidates : [];
      const candidates = await Promise.all(raw.map((row) => mapCandidate(row)));
      if (cardPreview.current !== item.id) return;
      const proposed = proposedEngineer(candidates, "");
      setResult({
        type: "info",
        text: proposed
          ? `На карточке ${proposed.name}. Можно назначить его.`
          : "Сервер не предложил инженера.",
        candidates,
        chosenId: proposed?.engineerId || "",
      });
    } catch (error) {
      if (cardPreview.current !== item.id) return;
      const missing = Number(error?.status) === 404;
      setResult({
        type: missing ? "info" : "error",
        text: missing
          ? "Этот сервер не отдаёт рейтинг. Назначьте нижней кнопкой."
          : `Рейтинг не пришёл: ${assignErrorMessage(error)}`,
        candidates: [],
        chosenId: "",
      });
    }
  }

  function toggleLayer(name, on) {
    setLayersOn((current) => ({ ...current, [name]: on }));
    actions.current.setLayer?.(name, on);
    softBeep(on ? 620 : 400, 70);
  }

  function logout() {
    if (inbox.length && !window.confirm(`В очереди ЦПУ ещё ${inbox.length}. Выйти?`)) return;
    softBeep(360, 100);
    clearSession();
    navigate("/");
  }

  function takeAlert(item) {
    const id = String(item.request_id || "");
    const eng = item.alternative_engineer_id ? String(item.alternative_engineer_id) : "";
    setReassignId(id);
    sessionStorage.setItem("oto-reassign-id", id);
    if (eng) {
      setReassignEngineer(eng);
      sessionStorage.setItem("oto-reassign-eng", eng);
    }
    softBeep(700, 100);
    window.queueMicrotask?.(() => {
      const panel = document.getElementById("reassign-panel");
      if (panel) panel.open = true;
      document.getElementById("reassign-reason")?.focus();
    });
  }

  async function loadJournalPreview() {
    setJournalPreviewErr("");
    try {
      const text = await fetchJournalCsv();
      const lines = text.split(/\r?\n/).filter((line) => line.trim().length);
      setJournalPreview(lines.slice(-8).join("\n"));
    } catch (error) {
      setJournalPreview("");
      setJournalPreviewErr(assignErrorMessage(error));
    }
  }

  async function downloadJournal() {
    setJournalPending(true);
    setJournalNote("");
    try {
      await downloadJournalCsv();
      softBeep(520, 120);
      setJournalNote("Файл journal.csv сохранён.");
      await loadJournalPreview();
    } catch (error) {
      setJournalNote(assignErrorMessage(error));
      softBeep(320, 200);
    } finally {
      setJournalPending(false);
    }
  }

  async function submitReassign(event) {
    event.preventDefault();
    const id = reassignId.trim();
    const engineerId = reassignEngineer.trim();
    const reason = reassignReason.trim();
    if (!id || !engineerId || !reason) {
      setReassignNote("Укажите заявку, инженера и причину.");
      return;
    }
    const known = listMcc().find((item) => String(item.serverId || "") === id);
    const fromAlert = alerts.find((item) => String(item.request_id || "") === id);
    const knownStatus = known?.status || fromAlert?.status || "";
    if (knownStatus === "done" || knownStatus === "completed" || knownStatus === "rejected") {
      setReassignNote("Заявка уже закрыта, переназначить нельзя.");
      softBeep(320, 200);
      return;
    }
    const confirmed = window.confirm("Переназначить может только старший смены. Продолжить?");
    if (!confirmed) return;
    setReassignPending(true);
    setReassignNote("");
    try {
      const data = await reassignRequest(id, engineerId, reason);
      softBeep(760, 120);
      setReassignId("");
      setReassignEngineer("");
      setReassignReason("");
      sessionStorage.removeItem("oto-reassign-id");
      sessionStorage.removeItem("oto-reassign-eng");
      sessionStorage.removeItem("oto-reassign-reason");
      setReassignNote(`Переназначено: ${data?.id || id}${data?.status ? ` · ${data.status}` : ""}`);
    } catch (error) {
      setReassignNote(assignErrorMessage(error));
      softBeep(320, 200);
    } finally {
      setReassignPending(false);
    }
  }

  async function enqueueMcc(item, event) {
    event.stopPropagation();
    if (!item?.serverId) {
      setQueueNote("Нет serverId — сначала запись через ЦПУ на сервер.");
      softBeep(320, 160);
      return;
    }
    setQueuePending(item.id);
    setQueueNote("");
    try {
      const data = await queueRequest(item.serverId);
      markMccQueued(item.id);
      setInbox(freshInbox());
      softBeep(700, 100);
      setQueueNote(`В очереди: ${data?.id || item.serverId}${data?.status ? ` · ${data.status}` : ""}`);
    } catch (error) {
      setQueueNote(assignErrorMessage(error));
      softBeep(320, 200);
    } finally {
      setQueuePending("");
    }
  }

  async function submitDefect(event) {
    event.preventDefault();
    const text = defectText.trim();
    const ata = defectAta.trim();
    if (!text || !ata) {
      setDefectNote("Нужны текст дефекта и глава ATA.");
      return;
    }
    setDefectPending(true);
    setDefectNote("");
    try {
      await createDefect(text, ata);
      softBeep(640, 100);
      setDefectText("");
      setDefectAta("");
      sessionStorage.removeItem("oto-defect-text");
      sessionStorage.removeItem("oto-defect-ata");
      setDefectNote("Дефект добавлен в классификатор.");
    } catch (error) {
      setDefectNote(assignErrorMessage(error));
      softBeep(320, 200);
    } finally {
      setDefectPending(false);
    }
  }

  function movementLabel(row) {
    const id = String(row.aircraft_id || "").slice(0, 8) || "борт";
    const arrived = row.arrived_at ? new Date(row.arrived_at).toLocaleString("ru-RU") : "—";
    const left = row.departed_at ? ` · ушёл ${new Date(row.departed_at).toLocaleString("ru-RU")}` : " · на стоянке";
    const point = Array.isArray(row.point) ? ` · ${row.point[0]}, ${row.point[1]}` : "";
    return `${id} · прибыл ${arrived}${left}${point}`;
  }

  async function onClassifyDescription() {
    const text = description.trim();
    if (!text) {
      setClassifyHint("Сначала введите описание.");
      return;
    }
    setClassifyPending(true);
    setClassifyHint("");
    try {
      const data = await classifyRemark(text);
      const name = engineerTypeLabel(data.engineer_type) || data.engineer_type;
      const pct = Number(data.confidence);
      const conf = Number.isFinite(pct) ? `${Math.round((pct <= 1 ? pct * 100 : pct))}%` : "—";
      softBeep(720, 100);
      setClassifyHint(`${name}, уверенность ${conf}. Подсказка, не назначение.`);
    } catch (error) {
      setClassifyHint(assignErrorMessage(error));
      softBeep(320, 200);
    } finally {
      setClassifyPending(false);
    }
  }

  async function onTrainClassifier() {
    if (!window.confirm("Переобучить классификатор на сервере?")) return;
    setTrainPending(true);
    setTrainNote("");
    try {
      const data = await trainClassifier();
      const acc = data?.accuracy == null ? "—" : `${Math.round(Number(data.accuracy) * 100)}%`;
      softBeep(600, 140);
      setTrainNote(`Обучено: ${data?.documents ?? "?"} док. · accuracy ${acc}`);
    } catch (error) {
      setTrainNote(assignErrorMessage(error));
      softBeep(320, 200);
    } finally {
      setTrainPending(false);
    }
  }

  async function onDepartPlane() {
    if (!selectedStand?.point) {
      setDepartNote("Выберите борт.");
      return;
    }
    if (!window.confirm(`Зафиксировать вылет ${planeId.slice(0, 8)}?`)) return;
    setDepartPending(true);
    setDepartNote("");
    try {
      await departAircraft(planeId);
      softBeep(540, 100);
      setDepartNote(`Вылет зафиксирован: ${planeId.slice(0, 8)}`);
      const moves = await loadMovements(12);
      setMovements(moves);
    } catch (error) {
      setDepartNote(assignErrorMessage(error));
      softBeep(320, 200);
    } finally {
      setDepartPending(false);
    }
  }

  async function onSelectMap(event) {
    event.preventDefault();
    if (!activeMapId) {
      setMapNote("Выберите карту.");
      return;
    }
    if (!window.confirm("Сделать эту карту активной для всех?")) return;
    setMapPending(true);
    setMapNote("");
    try {
      const data = await setActiveMap(activeMapId);
      softBeep(500, 100);
      setMapNote(`Активна: ${data?.name || activeMapId} · ${data?.width || "?"}×${data?.height || "?"}`);
      if (data?.reload) {
        window.location.reload();
        return;
      }
      const maps = await listMaps();
      setMapCatalog(Array.isArray(maps.maps) ? maps.maps : []);
      setActiveMapId(maps.active_id || activeMapId);
    } catch (error) {
      setMapNote(assignErrorMessage(error));
      softBeep(320, 200);
    } finally {
      setMapPending(false);
    }
  }

  async function runExam(kind, job) {
    setExamBusy(kind);
    setExamNote("");
    try {
      const data = await job();
      const left = data?.seconds_left;
      const line = data?.paused != null
        ? `${data.obj}: ${data.paused ? "пауза" : "снова идёт"}`
        : data?.forced != null
          ? `${String(data.engineer_id || "").slice(0, 8)}: транспорт ${data.forced ? "принудительно" : "как обычно"}`
          : left != null
            ? `До конца норматива ${left} с`
            : "Готово";
      setExamNote(line);
      softBeep(640, 90);
    } catch (error) {
      setExamNote(assignErrorMessage(error));
      softBeep(320, 160);
    } finally {
      setExamBusy("");
    }
  }

  async function onSendTrack(event) {
    event.preventDefault();
    const lat = Number(trackLat);
    const lon = Number(trackLon);
    if (!trackObj.trim() || !Number.isFinite(lat) || !Number.isFinite(lon)) {
      setTrackNote("Нужны obj, lat, lon.");
      softBeep(320, 160);
      return;
    }
    if (lat < -90 || lat > 90 || lon < -180 || lon > 180) {
      setTrackNote("lat −90…90, lon −180…180.");
      softBeep(320, 160);
      return;
    }
    setTrackPending(true);
    setTrackNote("");
    try {
      const acc = Number(trackAcc);
      const ack = await postTrackPoint({
        obj: trackObj.trim(),
        lat,
        lon,
        acc: Number.isFinite(acc) && acc >= 0 ? acc : null,
      });
      localStorage.setItem("oto-track-obj", trackObj.trim());
      localStorage.setItem("oto-track-lat", String(lat));
      localStorage.setItem("oto-track-lon", String(lon));
      localStorage.setItem("oto-track-acc", trackAcc);
      softBeep(ack?.applied ? 620 : 440, 120);
      setTrackNote(ack?.applied ? `Точка легла на карту · ${ack.obj || trackObj}` : `Точка принята, позицию не сдвинули · ${ack?.obj || trackObj}`);
    } catch (error) {
      setTrackNote(assignErrorMessage(error));
      softBeep(320, 200);
    } finally {
      setTrackPending(false);
    }
  }

  async function onMoveTransport(event) {
    event.preventDefault();
    const vehicle = fleet.find((item) => item.id === transportId);
    const x = Number(transportX);
    const y = Number(transportY);
    if (!vehicle || !Number.isFinite(x) || !Number.isFinite(y)) {
      setTransportNote("Нужны техника и клетка x/y.");
      return;
    }
    setTransportPending(true);
    setTransportNote("");
    try {
      await updateTransportPosition(vehicle.id, vehicle.vehicle_type, [x, y]);
      softBeep(560, 100);
      setTransportNote(`Техника ${vehicle.label}: ${x}, ${y}`);
      const next = await loadTransport();
      setFleet(next);
    } catch (error) {
      setTransportNote(assignErrorMessage(error));
    } finally {
      setTransportPending(false);
    }
  }

  async function onPreviewRoute(withVia = false) {
    const stand = mergePlanes(aircraft, demoStands).find((item) => item.id === planeId);
    if (!stand?.point) {
      setRouteNote("Поставьте стоянку кликом по карте.");
      return;
    }
    setRoutePending(true);
    setRouteNote("");
    try {
      const [staff, fleetNow] = await Promise.all([
        loadStaff(),
        withVia ? loadTransport() : Promise.resolve(fleet),
      ]);
      const plotted = (staff.plotted || []).filter((agent) => Array.isArray(agent.point));
      const chosen = plotted.find((agent) => String(agent.id) === String(clickEngineerId));
      const startAgent = chosen || plotted[0];
      if (!startAgent?.point) {
        setRouteNote("Нет инженера с координатами для пробы маршрута.");
        return;
      }
      let route = [];
      let cells = 0;
      let label = `${startAgent.name || "инженер"} → стоянка`;
      let note = "";
      let etaPart = "";
      if (withVia) {
        const needType = ISSUE_REQUIRED_VEHICLE[issue];
        const pool = (fleetNow || []).filter((v) => Array.isArray(v.point));
        const pick = (needType
          ? pool.find((v) => String(v.vehicle_type).toLowerCase() === String(needType).toLowerCase())
          : null) || pool[0];
        if (!pick?.point) {
          setRouteNote("Нет спецтранспорта на карте для пробы через промежуточную точку.");
          return;
        }
        // Инженер идёт пешком до спецтехники, дальше едет машина: agent по плечам (ТЗ 4.1).
        const data = await getRouteVia([startAgent.point, pick.point, stand.point], ["engineer", "vehicle"]);
        route = normalizeRoute(data?.route);
        cells = Number(data?.distance);
        etaPart = etaNote(data);
        label = `${startAgent.name || "инженер"} → ${pick.label} → стоянка`;
        note = `Проба via: ${label} · ${Number.isFinite(cells) ? cells : Math.max(0, route.length - 1)} клеток · ${data?.legs || 2} плеча · OpenAPI getRoute`;
      } else {
        const data = await getRoute(startAgent.point, stand.point, "engineer");
        route = normalizeRoute(data?.route);
        cells = Number(data?.distance);
        etaPart = etaNote(data);
        note = `Проба: ${label} · ${Number.isFinite(cells) ? cells : Math.max(0, route.length - 1)} клеток · OpenAPI getRoute`;
      }
      actions.current.previewRoute?.(route, label);
      softBeep(560, 100);
      setRouteNote(note + etaPart);
    } catch (error) {
      setRouteNote(assignErrorMessage(error));
      softBeep(320, 200);
    } finally {
      setRoutePending(false);
    }
  }

  async function onArrivePlane() {
    const plane = selectedStand;
    if (!plane?.point) {
      setArriveNote("Выберите борт со стоянкой.");
      return;
    }
    if (!window.confirm(`Зафиксировать прибытие ${plane.id.slice(0, 8)}?`)) return;
    setArrivePending(true);
    setArriveNote("");
    try {
      const data = await arriveAircraft(plane.point, plane.id);
      softBeep(580, 100);
      setArriveNote(`Прибытие: ${(data?.aircraft_id || plane.id).slice(0, 8)}`);
      const moves = await loadMovements(12);
      setMovements(moves);
    } catch (error) {
      setArriveNote(assignErrorMessage(error));
      softBeep(320, 200);
    } finally {
      setArrivePending(false);
    }
  }

  return (
    <div id="app-main" className="dispatcher-page" tabIndex={-1}>
      <NetBanner />
      <div id="sidebar">
        <div className="brand-row">
          <div>
            <p className="eyebrow">АЭРОФЛОТ · ОТО</p>
            <h1>Начальник смены <span className={staleLive ? "status-dot stale" : "status-dot"} title={staleLive ? "Нет свежих данных" : "LIVE"} /></h1>
          </div>
          <button type="button" className="role-link" onClick={logout}>Выйти</button>
        </div>
        <div className="account-strip">
          <span>
            {session.name}{session.email ? ` · ${session.email}` : ""}
            {session.userId ? (
              <>
                {" · "}
                <button
                  type="button"
                  className="ghost-copy"
                  onClick={async () => {
                    const ok = await copyText(session.userId);
                    setLive(ok ? "UUID диспетчера скопирован" : live);
                  }}
                >
                  {String(session.userId).slice(0, 8)}
                </button>
              </>
            ) : null}
          </span>
          <span className={staleLive ? "live-label stale" : "live-label"}>
            {crewNote ? `${crewNote} · ` : ""}
            {pollMs != null ? `${formatPollMs(pollMs)} · ` : ""}
            {staleLive ? `STALE · ${clock}` : clock}
          </span>
        </div>
        <details className="request-panel">
          <summary>Карта локации</summary>
          <form onSubmit={onSelectMap}>
            <label>Активная карта
              <select value={activeMapId} onChange={(event) => setActiveMapId(event.target.value)}>
                {mapCatalog.length ? mapCatalog.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.name} · {item.width}×{item.height}{item.active ? " · active" : ""}
                  </option>
                )) : <option value="">Нет карт</option>}
              </select>
            </label>
            <button type="submit" className="shift-button" disabled={mapPending || !mapCatalog.length || !activeMapId}>
              {mapPending ? "Переключение…" : "Сделать активной"}
            </button>
          </form>
          {mapNote ? <p className="fault-hint">{mapNote}</p> : null}
        </details>
        <details className="request-panel" open={aogList.length > 0}>
          <summary>AOG сейчас · {aogList.length}</summary>
          <div className="mcc-inbox">
            {aogList.length ? aogList.map((row) => (
              <button
                type="button"
                key={row.id}
                className="mcc-item"
                title="Подставить борт в заявку"
                onClick={() => {
                  setPlaneId(row.id);
                  softBeep(420, 100);
                  setLive(`AOG выбран: ${row.id.slice(0, 8)}`);
                  window.queueMicrotask?.(() => {
                    document.getElementById("assign-button")?.scrollIntoView({ block: "nearest", behavior: "smooth" });
                  });
                }}
              >
                <strong>{row.id.slice(0, 8)} · AOG</strong>
                <span>{row.point ? `${row.point[0]}, ${row.point[1]}` : "без точки"}</span>
              </button>
            )) : <p className="mcc-empty">Нет AOG.</p>}
          </div>
        </details>
        <details className="request-panel" open={assignHistory.length > 0}>
          <summary>Недавние назначения · {assignHistory.length}</summary>
          <div className="mcc-inbox">
            {assignHistory.length ? assignHistory.map((row, index) => (
              <button
                type="button"
                className="mcc-item"
                key={`${row.at}-${index}`}
                title="Подставить борт и описание"
                onClick={() => {
                  if (row.planeId) setPlaneId(row.planeId);
                  if (row.issue && ISSUE_LABELS.has(row.issue)) setIssue(row.issue);
                  if (row.description) {
                    setDescription(row.description);
                    sessionStorage.setItem("oto-disp-draft", row.description);
                  }
                  softBeep(520, 80);
                }}
              >
                <strong>{row.name} · {row.plane}</strong>
                <span>{ISSUE_LABELS.get(row.issue) || row.issue} · ETA {row.minutes} мин{row.sla ? " · SLA" : ""} · {relativeRu(row.at, nowTick)}</span>
              </button>
            )) : <p className="mcc-empty">Пока пусто в этой вкладке.</p>}
          </div>
          {assignHistory.length ? (
            <button
              type="button"
              className="ghost-copy"
              onClick={() => {
                if (!window.confirm("Очистить историю назначений этой вкладки?")) return;
                sessionStorage.removeItem("oto-assign-history");
                setAssignHistory([]);
                softBeep(400, 80);
              }}
            >
              Очистить историю вкладки
            </button>
          ) : null}
        </details>
        <details className="request-panel" open={alerts.length > 0}>
          <summary>Риск норматива · {alerts.length}</summary>
          <div className="mcc-inbox">
            {alerts.length ? alerts.map((item, index) => {
              const minutes = marginMinutes(item.alternative_margin_seconds);
              const alt = item.alternativeName
                ? `Альтернатива ${item.alternativeName}`
                : item.alternative_engineer_id
                  ? `Альтернатива ${String(item.alternative_engineer_id).slice(0, 8)}`
                  : null;
              return (
                <button
                  type="button"
                  key={`${item.request_id}-${index}`}
                  className="mcc-item"
                  onClick={() => takeAlert(item)}
                >
                  <strong>{item.message}</strong>
                  {minutes == null ? null : <span>Запас альтернативы: {minutes} мин</span>}
                  {alt ? <span>{alt}</span> : null}
                  <span>Заявка {item.request_id}</span>
                </button>
              );
            }) : <p className="mcc-empty">Нет рисков норматива.</p>}
          </div>
          {alertsNote ? <p className="fault-hint">{alertsNote}</p> : null}
          <button id="download-journal" type="button" className="shift-button" disabled={journalPending} onClick={downloadJournal} title="Ctrl+J">
            {journalPending ? "Скачивание…" : "Скачать журнал · Ctrl+J"}
          </button>
          {journalNote ? <p className="fault-hint">{journalNote}</p> : null}
          {journalPreviewErr ? <p className="fault-hint">{journalPreviewErr}</p> : null}
          {journalPreview ? (
            <pre className="journal-preview" aria-label="Последние строки journal.csv">{journalPreview}</pre>
          ) : null}
        </details>
        <details className="request-panel" open>
          <summary>Заявки ЦПУ · {inbox.length}</summary>
          <label className="layer-row">
            <input
              type="checkbox"
              checked={urgentOnly}
              onChange={(event) => {
                const next = event.target.checked;
                setUrgentOnly(next);
                localStorage.setItem("oto-urgent-only", next ? "1" : "0");
              }}
            />
            Только срочные · Ctrl+U
          </label>
          <div className="mcc-inbox">
            {(() => {
              const rows = urgentOnly ? inbox.filter((item) => item.urgency === "urgent") : inbox;
              if (!rows.length) {
                return <p className="mcc-empty">{urgentOnly ? "Нет срочных замечаний." : "Нет новых замечаний от ЦПУ."}</p>;
              }
              return rows.map((item) => {
                const proposed = item.id === activeMcc
                  ? proposedEngineer(result.candidates, result.chosenId)
                  : null;
                return (
                <div
                  key={item.id}
                  className={[
                    "mcc-item",
                    item.id === activeMcc ? "selected" : "",
                    item.urgency === "urgent" ? "urgent" : "",
                  ].filter(Boolean).join(" ")}
                >
                  <button type="button" className="mcc-item-pick" onClick={() => chooseMcc(item)}>
                    <strong>{issueLabel(item.issue)}{item.urgency === "urgent" ? " · срочно" : ""}</strong>
                    <span>Борт {String(item.planeUuid).slice(0, 8)} · {item.status === "assigned" ? "назначена" : "не назначена"} · {relativeRu(item.createdAt, nowTick)}</span>
                    <span>{item.description}</span>
                  </button>
                  {proposed ? (
                    <>
                      <strong>{proposed.name}</strong>
                      <button
                        type="button"
                        className="shift-button"
                        disabled={pending}
                        onClick={() => actions.current.assign?.(proposed.engineerId)}
                      >
                        Назначить его
                      </button>
                    </>
                  ) : item.id === activeMcc ? (
                    <span>Рейтинг инженеров ещё не пришёл</span>
                  ) : null}
                  {item.serverId ? (
                    <button
                      type="button"
                      className="shift-button"
                      disabled={queuePending === item.id || item.status === "unassigned"}
                      onClick={(event) => enqueueMcc(item, event)}
                    >
                      {item.status === "unassigned" ? "Уже в очереди" : "В очередь назначения"}
                    </button>
                  ) : null}
                </div>
                );
              });
            })()}
          </div>
          {queueNote ? <p className="fault-hint">{queueNote}</p> : null}
        </details>
        <details className="request-panel">
          <summary>Движения ВС · {movements.length}</summary>
          <div className="mcc-inbox">
            {movements.length ? movements.map((row) => (
              <button
                type="button"
                className="mcc-item"
                key={row.id || `${row.aircraft_id}-${row.arrived_at}`}
                title="Подставить борт в заявку"
                onClick={() => {
                  if (!row.aircraft_id) return;
                  setPlaneId(String(row.aircraft_id));
                  softBeep(520, 80);
                  setLive(`Борт из movements: ${String(row.aircraft_id).slice(0, 8)}`);
                }}
              >
                <strong>{String(row.aircraft_id || "").slice(0, 8) || "ВС"}</strong>
                <span>{movementLabel(row)}</span>
              </button>
            )) : <p className="mcc-empty">Нет записей прибытия.</p>}
          </div>
        </details>
        <details className="request-panel">
          <summary>Классификатор</summary>
          <form onSubmit={submitDefect}>
            <label>Текст дефекта
              <input
                type="text"
                value={defectText}
                autoComplete="off"
                onChange={(event) => {
                  const next = event.target.value;
                  setDefectText(next);
                  sessionStorage.setItem("oto-defect-text", next);
                }}
              />
            </label>
            <button
              type="button"
              className="shift-button"
              disabled={!description.trim()}
              onClick={() => {
                const next = description.trim();
                setDefectText(next);
                sessionStorage.setItem("oto-defect-text", next);
              }}
            >
              Взять из описания заявки
            </button>
            <label>Глава ATA
              <input
                type="text"
                value={defectAta}
                autoComplete="off"
                onChange={(event) => {
                  const next = event.target.value;
                  setDefectAta(next);
                  sessionStorage.setItem("oto-defect-ata", next);
                }}
              />
            </label>
            <button type="submit" className="shift-button" disabled={defectPending || !defectText.trim() || !defectAta.trim()}>
              {defectPending ? "Запись…" : "Добавить дефект"}
            </button>
          </form>
          {defectNote ? <p className="fault-hint">{defectNote}</p> : null}
          <button type="button" className="shift-button" disabled={trainPending || defectPending} onClick={onTrainClassifier}>
            {trainPending ? "Обучение…" : "Переобучить классификатор"}
          </button>
          {trainNote ? <p className="fault-hint">{trainNote}</p> : null}
        </details>
        <section className="request-panel">
          <div className="section-heading"><h2>Новая заявка</h2><span className="live-label">LIVE</span></div>
          <label>Клик по карте
            <select
              value={clickMode}
              onChange={(event) => {
                const next = event.target.value;
                setClickMode(next);
                localStorage.setItem("oto-click-mode", next);
              }}
            >
              <option value="engineer">инженер</option>
              <option value="transport">транспорт</option>
              <option value="stand">стоянка ВС</option>
            </select>
          </label>
          {clickMode === "engineer" ? (
            <label>Инженер на карте
              <select
                value={staffRows.some((row) => row.id === clickEngineerId) ? clickEngineerId : ""}
                onChange={(event) => {
                  setClickEngineerId(event.target.value);
                  localStorage.setItem("oto-click-eng", event.target.value);
                }}
              >
                {staffRows.length ? staffRows.map((row) => (
                  <option key={row.id} value={row.id}>{row.name}{row.point ? "" : " · нет точки"}</option>
                )) : <option value="">Нет инженеров</option>}
              </select>
            </label>
          ) : null}
          {clickMode === "transport" ? (
            <label>Тип техники
              <select
                value={clickVehicleType}
                onChange={(event) => {
                  setClickVehicleType(event.target.value);
                  localStorage.setItem("oto-click-vehicle", event.target.value);
                }}
              >
                {CLICK_VEHICLES.map((type) => (
                  <option key={type} value={type}>{vehicleLabel(type)}</option>
                ))}
              </select>
            </label>
          ) : null}
          <label>Фильтр бортов
            <input
              id="plane-filter"
              type="search"
              value={planeFilter}
              placeholder="uuid или координата · Ctrl+K"
              autoComplete="off"
              onChange={(event) => setPlaneFilter(event.target.value)}
            />
          </label>
          {planeFilter ? (
            <button
              type="button"
              className="ghost-copy"
              onClick={() => {
                setPlaneFilter("");
                document.getElementById("plane-filter")?.focus();
              }}
            >
              Сбросить фильтр
            </button>
          ) : null}
          <label>Воздушное судно
            <select
              value={planeOptions.some((plane) => plane.id === planeId) ? planeId : ""}
              className={deferredPlaneFilter !== planeFilter ? "plane-select-pending" : undefined}
              onChange={(event) => setPlaneId(event.target.value)}
            >
              {(() => {
                const q = deferredPlaneFilter.trim().toLowerCase();
                const filtered = q
                  ? planeOptions.filter((plane) => {
                    const hay = `${plane.id} ${plane.point?.[0]} ${plane.point?.[1]}`.toLowerCase();
                    return hay.includes(q);
                  })
                  : planeOptions;
                const list = filtered.slice().sort((a, b) => Number(aogIds.has(b.id)) - Number(aogIds.has(a.id)));
                if (!list.length) return <option value="">{planeOptions.length ? "Нет совпадений" : "Стоянка с карты"}</option>;
                return list.map((plane) => {
                  const demo = !aircraft.some((item) => item.id === plane.id);
                  return (
                    <option key={plane.id} value={plane.id}>
                      {demo ? "Стоянка" : plane.id.slice(0, 8)} · {plane.point[0]}, {plane.point[1]}{aogIds.has(plane.id) ? " · AOG" : ""}
                    </option>
                  );
                });
              })()}
            </select>
          </label>
            <button
              type="button"
              className="shift-button"
              disabled={!planeId}
              onClick={async () => {
                const ok = await copyText(planeId);
                setLive(ok ? `Скопирован UUID: ${planeId}` : live);
                softBeep(ok ? 600 : 320, ok ? 80 : 160);
              }}
            >
              {planeId ? `Копировать UUID · ${planeId.slice(0, 8)}` : "Копировать UUID борта"}
            </button>
            <button
              type="button"
              className="shift-button"
              onClick={async () => {
                try {
                  const text = (await navigator.clipboard.readText()).trim();
                  const match = text.match(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/i);
                  if (!match) {
                    setLive("В буфере нет UUID борта.");
                    softBeep(320, 160);
                    return;
                  }
                  const hit = aircraft.find((plane) => plane.id.toLowerCase() === match[0].toLowerCase());
                  if (hit) {
                    setPlaneId(hit.id);
                    setPlaneFilter("");
                    setLive(`Борт из буфера: ${hit.id.slice(0, 8)}`);
                    softBeep(640, 100);
                  } else {
                    setPlaneFilter(match[0]);
                    setLive(`UUID из буфера не в /api/aircraft: ${match[0].slice(0, 8)}`);
                    softBeep(440, 120);
                  }
                } catch {
                  setLive("Нет доступа к буферу обмена.");
                  softBeep(320, 160);
                }
              }}
            >
              Вставить UUID из буфера
            </button>
          {planeId && aogIds.has(planeId) ? <p className="fault-hint stale-live">Выбранный борт в AOG.</p> : null}
          {!aircraft.length ? <p className="fault-hint">Список бортов пуст. Стоянка с карты достаточна для назначения.</p> : null}
          {staffError ? <p className="fault-hint stale-live">{staffError}</p> : null}
          <p className="fault-hint">{live}</p>
          <label>Тип неисправности
            <select value={issue} onChange={(event) => setIssue(event.target.value)}>
              {GROUPS.map(([label, values]) => {
                const visible = values.filter((value) => value !== "other");
                if (!visible.length) return null;
                return (
                  <optgroup key={label} label={label}>
                    {visible.map((value) => <option key={value} value={value}>{ISSUE_LABELS.get(value)}</option>)}
                  </optgroup>
                );
              })}
            </select>
          </label>
          <div className="fault-hint">{hint}</div>
          <label>Описание проблемы
            <textarea
              rows={3}
              value={description}
              placeholder="Например: обнаружены следы подтекания топлива"
              onChange={(event) => {
                const next = event.target.value;
                setDescription(next);
                sessionStorage.setItem("oto-disp-draft", next);
              }}
            />
          </label>
          <button type="button" className="shift-button" disabled={classifyPending || pending || !description.trim()} onClick={onClassifyDescription}>
            {classifyPending ? "Классификация…" : "Подсказать тип инженера"}
          </button>
          {classifyHint ? <p className="fault-hint">{classifyHint}</p> : null}
          <button type="button" className="shift-button" disabled={arrivePending || !selectedStand?.point || pending || departPending} onClick={onArrivePlane}>
            {arrivePending ? "Фиксация…" : "Зафиксировать прибытие борта"}
          </button>
          {arriveNote ? <p className="fault-hint">{arriveNote}</p> : null}
          <button type="button" className="shift-button" disabled={departPending || !selectedStand?.point || pending || arrivePending} onClick={onDepartPlane}>
            {departPending ? "Фиксация…" : "Зафиксировать вылет борта"}
          </button>
          {departNote ? <p className="fault-hint">{departNote}</p> : null}
          <button type="button" className="shift-button" disabled={routePending || !selectedStand?.point || pending || arrivePending || departPending} onClick={() => onPreviewRoute(false)}>
            {routePending ? "Считаем…" : "Проба маршрута getRoute"}
          </button>
          <button type="button" className="shift-button" disabled={routePending || !selectedStand?.point || pending || arrivePending || departPending} onClick={() => onPreviewRoute(true)}>
            {routePending ? "Считаем…" : "Проба через спецтранспорт (via)"}
          </button>
          {routeNote ? <p className="fault-hint">{routeNote}</p> : null}
          <button
            id="assign-button"
            type="button"
            disabled={pending || !selectedStand?.point}
            aria-busy={pending}
            title={selectedStand?.point ? "Ctrl+Enter" : "Поставьте стоянку на карте"}
            onClick={() => {
              if (pending || !selectedStand?.point) return;
              actions.current.assign?.();
            }}
          >
            {pending ? "Назначение…" : <>Назначить инженера <span className="hotkey-mark">· Ctrl+Enter</span></>}
          </button>
          <details className="shortcut-help">
            <summary>Горячие клавиши</summary>
            <ul>
              <li><kbd>Ctrl</kbd>+<kbd>Enter</kbd> — назначить инженера</li>
              <li><kbd>Ctrl</kbd>+<kbd>K</kbd> — фильтр бортов</li>
              <li><kbd>Ctrl</kbd>+<kbd>U</kbd> — только срочные</li>
              <li><kbd>Ctrl</kbd>+<kbd>J</kbd> — журнал.csv</li>
              <li><kbd>1</kbd>–<kbd>4</kbd> — слои карты</li>
              <li><kbd>Esc</kbd> — снять выбор, фильтр и подсказки</li>
            </ul>
          </details>
          {staleLive ? <p className="fault-hint stale-live">Карта давно не обновлялась — проверьте сеть или API.</p> : null}
          <div
            className={result.type === "ok" ? "assignment-result success" : result.type === "error" ? "assignment-result error" : "assignment-result"}
            role="status"
            aria-live="polite"
          >
            {result.type === "ok" ? (
              <>
                <strong>{result.name}</strong><br />
                {result.candidateMs != null ? (
                  <p className="assign-timing">Подбор кандидатов: <strong>{result.candidateMs} мс</strong></p>
                ) : null}
                Борт {result.plane} · {result.issue}<br />
                {result.onSite ? "Инженер уже у воздушного судна, маршрут пустой." : <>Маршрут: <strong>{result.cells} клеток</strong><br />В пути: <strong>{result.minutes} мин</strong></>}
                {(() => {
                  const chosen = (result.candidates || []).find((row) => row.engineerId === result.chosenId);
                  if (!chosen) return null;
                  return (
                    <>
                      {chosen.usesVehicle ? <><br />Прогноз со спецтранспортом, посадка 10 с уже внутри.</> : <><br />Прогноз пешком по скоростям графа.</>}
                      {String(chosen.reason || "").includes("нужен склад") ? <><br />В прогнозе есть заход на склад, 30 с.</> : null}
                    </>
                  );
                })()}
                {result.pickup ? <><br />Маршрут заезжает за спецтранспорт: <strong>{result.pickup}</strong>.</> : null}
                {result.needed ? <><br />Для неисправности нужен: <strong>{result.needed}</strong>.</> : null}
                {result.skipped ? <><br /><em>Занятые инженеры пропущены ({result.skipped}).</em></> : null}
                {result.sla ? <><br /><em>Время в пути больше 15 минут. · {result.minutes} мин</em></> : null}
                <button
                  type="button"
                  className="ghost-copy"
                  style={{ marginTop: 8 }}
                  onClick={async () => {
                    const line = [
                      result.name,
                      `борт ${result.plane}`,
                      result.issue,
                      result.onSite ? "на месте" : `ETA ${result.minutes} мин`,
                      result.sla ? `SLA превышен · ${result.minutes} мин` : null,
                    ].filter(Boolean).join(" · ");
                    const ok = await copyText(line);
                    setLive(ok ? "Итог назначения скопирован" : live);
                  }}
                >
                  Копировать итог
                </button>
                {Array.isArray(result.candidates) && result.candidates.length ? (
                  <button
                    type="button"
                    className="ghost-copy"
                    style={{ marginTop: 8, marginLeft: 8 }}
                    onClick={async () => {
                      const header = "engineer_id;name;eligible;reason;eta_min;vehicle;factors;chosen";
                      const rows = result.candidates.map((row) => {
                        const etaMin = row.etaSeconds == null || Number.isNaN(Number(row.etaSeconds))
                          ? ""
                          : String(Math.max(0, Math.round(Number(row.etaSeconds) / 60)));
                        return [
                          row.engineerId,
                          row.name,
                          row.eligible ? "1" : "0",
                          String(row.reason || "").replace(/;/g, ","),
                          etaMin,
                          row.usesVehicle ? "1" : "0",
                          String(row.factors || "").replace(/;/g, ","),
                          row.engineerId === result.chosenId ? "1" : "0",
                        ].join(";");
                      });
                      const ok = await copyText([header, ...rows].join("\n"));
                      setLive(ok ? "Таблица кандидатов скопирована" : live);
                    }}
                  >
                    Копировать кандидатов
                  </button>
                ) : null}
                {Array.isArray(result.candidates) && result.candidates.length ? (
                  <div className="candidates-block">
                    <p className="candidates-meta">
                      Кандидаты{result.candidateMs != null ? ` · ${result.candidateMs} мс` : ""}
                    </p>
                    <table className="candidates-table">
                      <thead>
                        <tr>
                          <th>Инженер</th>
                          <th>Допуск</th>
                          <th>Причина</th>
                          <th>ETA</th>
                          <th>Техника</th>
                          <th>Слагаемые</th>
                        </tr>
                      </thead>
                      <tbody>
                        {result.candidates.map((row) => {
                          const chosen = row.engineerId === result.chosenId;
                          const etaMin = row.etaSeconds == null || Number.isNaN(Number(row.etaSeconds))
                            ? "—"
                            : `${Math.max(0, Math.round(Number(row.etaSeconds) / 60))} мин`;
                          return (
                            <tr
                              key={row.engineerId}
                              className={[
                                chosen ? "candidate-chosen" : "",
                                row.eligible ? "" : "candidate-skip",
                              ].filter(Boolean).join(" ") || undefined}
                              title="Клик — в переназначение · двойной клик — копировать uuid"
                              onClick={() => {
                                setReassignEngineer(row.engineerId);
                                sessionStorage.setItem("oto-reassign-eng", row.engineerId);
                                setLive(`В переназначение: ${row.name}`);
                              }}
                              onDoubleClick={async () => {
                                const ok = await copyText(row.engineerId);
                                setLive(ok ? `Скопирован uuid ${row.engineerId.slice(0, 8)}` : "Не удалось скопировать uuid");
                                softBeep(ok ? 600 : 320, ok ? 80 : 160);
                              }}
                              style={{ cursor: "pointer" }}
                            >
                              <td>
                                <strong>{row.name}</strong>
                                <span className="candidate-id">{row.engineerId.slice(0, 8)}</span>
                                {chosen ? <span className="candidate-badge">выбран</span> : null}
                              </td>
                              <td>{row.eligible ? "да" : "нет"}</td>
                              <td>{row.reason || "—"}</td>
                              <td>{etaMin}</td>
                              <td>{row.usesVehicle ? "да" : "нет"}</td>
                              <td>{row.factors || "—"}</td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  </div>
                ) : null}
              </>
            ) : result.text}
          </div>
        </section>
        <details className="request-panel">
          <summary>Проверка на площадке</summary>
          <form onSubmit={(event) => {
            event.preventDefault();
            const obj = pauseObj.trim();
            if (!obj) {
              setExamNote("Нужен obj агента: e-01, v-01 или борт.");
              return;
            }
            runExam("pause", () => pauseAgent(obj, true));
          }}>
            <label>Пауза агента
              <input type="text" value={pauseObj} autoComplete="off" placeholder="e-01, v-01, RA-…" onChange={(event) => setPauseObj(event.target.value)} />
            </label>
            <button type="submit" className="shift-button" disabled={examBusy === "pause"}>Пауза</button>
            <button type="button" className="shift-button" disabled={examBusy === "resume" || !pauseObj.trim()} onClick={() => runExam("resume", () => pauseAgent(pauseObj.trim(), false))}>Снять паузу</button>
          </form>
          <form onSubmit={(event) => {
            event.preventDefault();
            const id = (forceId || clickEngineerId).trim();
            if (!id) {
              setExamNote("Выберите инженера.");
              return;
            }
            runExam("force", () => forceVehicle(id, true));
          }}>
            <label>Принудительный транспорт
              <select value={forceId || clickEngineerId} onChange={(event) => setForceId(event.target.value)}>
                <option value="">инженер</option>
                {staffRows.map((row) => (
                  <option key={row.id} value={row.id}>{row.name || row.id.slice(0, 8)}</option>
                ))}
              </select>
            </label>
            <button type="submit" className="shift-button" disabled={examBusy === "force"}>Всегда на технике</button>
            <button type="button" className="shift-button" disabled={examBusy === "unforce" || !(forceId || clickEngineerId)} onClick={() => runExam("unforce", () => forceVehicle((forceId || clickEngineerId).trim(), false))}>Снова пешком, если быстрее</button>
          </form>
          <form onSubmit={(event) => {
            event.preventDefault();
            const id = (timerId || reassignId).trim();
            const delta = Number(timerDelta);
            if (!id || !Number.isFinite(delta) || delta === 0) {
              setExamNote("Нужны id заявки и сдвиг в секундах, не ноль.");
              return;
            }
            runExam("timer", () => shiftTaskTimer(id, delta));
          }}>
            <label>Заявка
              <input type="text" value={timerId || reassignId} autoComplete="off" placeholder="id заявки" onChange={(event) => setTimerId(event.target.value)} />
            </label>
            <label>Сдвиг, с
              <input type="text" inputMode="numeric" value={timerDelta} autoComplete="off" onChange={(event) => setTimerDelta(event.target.value)} />
            </label>
            <button type="submit" className="shift-button" disabled={examBusy === "timer"}>Сдвинуть таймер</button>
            <button type="button" className="shift-button" disabled={examBusy === "timer"} onClick={() => {
              setTimerDelta("-300");
              const id = (timerId || reassignId).trim();
              if (!id) {
                setExamNote("Нужен id заявки.");
                return;
              }
              runExam("timer", () => shiftTaskTimer(id, -300));
            }}>Минус 5 мин</button>
          </form>
          {examNote ? <p className="fault-hint">{examNote}</p> : null}
        </details>
        <details className="request-panel">
          <summary>Точка GPS</summary>
          <form onSubmit={onSendTrack}>
            <label>Объект<input type="text" value={trackObj} autoComplete="off" placeholder="uuid инженера, v-01 или RA-…" onChange={(event) => setTrackObj(event.target.value)} /></label>
            <label>Широта<input type="text" inputMode="decimal" value={trackLat} autoComplete="off" onChange={(event) => setTrackLat(event.target.value)} /></label>
            <label>Долгота<input type="text" inputMode="decimal" value={trackLon} autoComplete="off" onChange={(event) => setTrackLon(event.target.value)} /></label>
            <label>Точность, м<input type="text" inputMode="decimal" value={trackAcc} autoComplete="off" onChange={(event) => setTrackAcc(event.target.value)} /></label>
            <button type="submit" className="shift-button" disabled={trackPending}>
              {trackPending ? "Отправка…" : "Отправить точку"}
            </button>
          </form>
          {trackNote ? <p className="fault-hint">{trackNote}</p> : null}
        </details>
        <details className="request-panel">
          <summary>Спецтранспорт</summary>
          <form onSubmit={onMoveTransport}>
            <label>Техника
              <select
                value={transportId}
                onChange={(event) => {
                  const id = event.target.value;
                  setTransportId(id);
                  sessionStorage.setItem("oto-transport-id", id);
                  const vehicle = fleet.find((item) => item.id === id);
                  if (vehicle?.point) {
                    setTransportX(String(vehicle.point[0]));
                    setTransportY(String(vehicle.point[1]));
                    sessionStorage.setItem("oto-transport-x", String(vehicle.point[0]));
                    sessionStorage.setItem("oto-transport-y", String(vehicle.point[1]));
                  }
                }}
              >
                {fleet.length ? fleet.map((item) => (
                  <option key={item.id} value={item.id}>{item.label} · {item.id.slice(0, 8)}</option>
                )) : <option value="">Нет техники</option>}
              </select>
            </label>
            <label>X<input type="text" inputMode="numeric" value={transportX} autoComplete="off" onChange={(event) => { const next = event.target.value; setTransportX(next); sessionStorage.setItem("oto-transport-x", next); }} /></label>
            <label>Y<input type="text" inputMode="numeric" value={transportY} autoComplete="off" onChange={(event) => { const next = event.target.value; setTransportY(next); sessionStorage.setItem("oto-transport-y", next); }} /></label>
            <button type="submit" className="shift-button" disabled={transportPending || !fleet.length}>
              {transportPending ? "Сдвиг…" : "Сдвинуть технику"}
            </button>
          </form>
          {transportNote ? <p className="fault-hint">{transportNote}</p> : null}
        </details>
        <details className="request-panel" id="reassign-panel">
          <summary>Переназначение</summary>
          <form onSubmit={submitReassign}>
            <label>Заявка
              <input
                type="text"
                value={reassignId}
                autoComplete="off"
                spellCheck={false}
                onChange={(event) => {
                  const next = event.target.value;
                  setReassignId(next);
                  sessionStorage.setItem("oto-reassign-id", next);
                }}
              />
            </label>
            <label>Инженер
              <input
                type="text"
                value={reassignEngineer}
                autoComplete="off"
                spellCheck={false}
                onChange={(event) => {
                  const next = event.target.value;
                  setReassignEngineer(next);
                  sessionStorage.setItem("oto-reassign-eng", next);
                }}
              />
            </label>
            <label>Причина
              <input
                id="reassign-reason"
                type="text"
                value={reassignReason}
                autoComplete="off"
                onChange={(event) => {
                  const next = event.target.value;
                  setReassignReason(next);
                  sessionStorage.setItem("oto-reassign-reason", next);
                }}
              />
            </label>
            <button type="submit" className="shift-button" disabled={reassignPending}>
              {reassignPending ? "Переназначение…" : "Переназначить"}
            </button>
          </form>
          {reassignNote ? <p className="fault-hint">{reassignNote}</p> : null}
        </details>
        <details className="request-panel">
          <summary>Отображение</summary>
          <div>
            <label className="layer-row"><input type="checkbox" checked={layersOn.staff} onChange={(event) => toggleLayer("staff", event.target.checked)} /> Сотрудники · 1</label>
            <label className="layer-row"><input type="checkbox" checked={layersOn.transport} onChange={(event) => toggleLayer("transport", event.target.checked)} /> Спецтранспорт · 2</label>
            <label className="layer-row"><input type="checkbox" checked={layersOn.stands} onChange={(event) => toggleLayer("stands", event.target.checked)} /> Воздушные суда · 3</label>
            <label className="layer-row"><input type="checkbox" checked={layersOn.route} onChange={(event) => toggleLayer("route", event.target.checked)} /> Маршрут · 4</label>
          </div>
        </details>
        <details className="request-panel">
          <summary>Координаты</summary>
          <div id="tile-info">{tileInfo}</div>
          <button
            type="button"
            className="ghost-copy"
            onClick={async () => {
              const ok = await copyText(tileInfo);
              setLive(ok ? "Координаты клетки скопированы" : live);
            }}
          >
            Копировать
          </button>
        </details>
        <details className="request-panel">
          <summary>Легенда</summary>
          <div>
            {LEGEND.map(([color, label]) => (
              <div className="legend-row" key={label}>
                <span className="swatch" style={{ background: color }} />
                {label}
              </div>
            ))}
          </div>
        </details>
      </div>
      <div id="map" ref={mapNode} />
    </div>
  );
}
