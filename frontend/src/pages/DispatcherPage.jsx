import { useEffect, useRef, useState } from "react";
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
  engineerName,
  findTransportOnRoute,
  loadAircraft,
  loadAogIds,
  loadBusyEngineerIds,
  arriveAircraft,
  createDefect,
  departAircraft,
  downloadJournalCsv,
  getRoute,
  listMaps,
  loadAlerts,
  loadAogAircraft,
  loadMovements,
  loadStaff,
  loadTransport,
  postTrackPoint,
  queueRequest,
  reassignRequest,
  setActiveMap,
  trainClassifier,
  updateTransportPosition,
} from "../lib/live.js";
import { classifyRemark } from "../lib/mccApi.js";
import { softBeep } from "../lib/beep.js";
import { copyText } from "../lib/copyText.js";
import { relativeRu } from "../lib/format.js";
import NetBanner from "../components/NetBanner.jsx";
import StatusChain from "../components/StatusChain.jsx";
import { listMcc, markMccAssigned, markMccQueued, MCC_QUEUE_KEY } from "../lib/mccQueue.js";
import { apiRequest, clearSession, getSession, hydrateProfile } from "../lib/session.js";
import { whenVisible } from "../lib/visibility.js";
import { pointAt, WALK_MPS } from "../lib/walk.js";
import {
  buildTileGrid,
  cellToLatLng,
  centerToLatLng,
  clickToCell,
  loadApronMap,
  loadTilesetImages,
  renderCanvas,
  roadHint,
} from "../lib/apronMap.js";

const GROUPS = issueGroups();
const FIRST_ISSUE = GROUPS[0][1][0];
const ISSUE_LABELS = new Map(GROUPS.flatMap(([, values]) => values).map((value) => [value, issueLabel(value)]));
const LEGEND = [
  ["#0c7a56", "инженер свободен"],
  ["#f47920", "инженер занят"],
  ["#f47920", "спецтранспорт"],
  ["#00529b", "воздушное судно"],
  ["#c5161c", "AOG"],
  ["#c5161c", "маршрут"],
];

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

function marginMinutes(seconds) {
  if (seconds == null || Number.isNaN(Number(seconds))) return null;
  return Math.round(Number(seconds) / 60);
}

function dispatcherStatusStep(result, inbox, activeMcc) {
  if (result?.type === "ok") return "назначен";
  if (inbox.length || activeMcc) return "не назначен";
  return "замечание";
}

export default function DispatcherPage() {
  const navigate = useNavigate();
  const session = getSession();
  const allowed = session?.role === "Dispatcher";
  const mapNode = useRef(null);
  const actions = useRef({});
  const formRef = useRef({});
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
  const alertCountRef = useRef(0);
  const inboxCountRef = useRef(0);

  const [aircraft, setAircraft] = useState([]);
  const [aogIds, setAogIds] = useState(() => new Set());
  const [planeId, setPlaneId] = useState("");
  const [issue, setIssue] = useState(() => {
    const saved = localStorage.getItem("oto-last-issue");
    return saved && ISSUE_LABELS.has(saved) ? saved : FIRST_ISSUE;
  });
  const [description, setDescription] = useState(() => sessionStorage.getItem("oto-disp-draft") || "");
  const [hint, setHint] = useState("");
  const [live, setLive] = useState("Загрузка бортов, инженеров и техники…");
  const [result, setResult] = useState({ type: "info", text: "Выберите борт из списка и тип неисправности." });
  const [inbox, setInbox] = useState(freshInbox);
  const [activeMcc, setActiveMcc] = useState(null);
  const [tileInfo, setTileInfo] = useState("Кликни по карте, чтобы выбрать клетку.");
  const [pending, setPending] = useState(false);
  const [alerts, setAlerts] = useState([]);
  const [alertsNote, setAlertsNote] = useState("");
  const [journalNote, setJournalNote] = useState("");
  const [journalPending, setJournalPending] = useState(false);
  const [reassignId, setReassignId] = useState("");
  const [reassignEngineer, setReassignEngineer] = useState("");
  const [reassignReason, setReassignReason] = useState("");
  const [reassignNote, setReassignNote] = useState("");
  const [reassignPending, setReassignPending] = useState(false);
  const [queueNote, setQueueNote] = useState("");
  const [queuePending, setQueuePending] = useState("");
  const [defectText, setDefectText] = useState("");
  const [defectAta, setDefectAta] = useState("");
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
  const [transportId, setTransportId] = useState("");
  const [transportX, setTransportX] = useState("");
  const [transportY, setTransportY] = useState("");
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
  const [trackNote, setTrackNote] = useState("");
  const [trackPending, setTrackPending] = useState(false);
  const [movements, setMovements] = useState([]);
  const [layersOn, setLayersOn] = useState(layersRef.current);
  const [planeFilter, setPlaneFilter] = useState("");
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
  const lastLiveAt = useRef(Date.now());

  formRef.current = { issue, description, planeId, activeMcc };
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

  useEffect(() => {
    if (!allowed) return undefined;
    const timer = window.setInterval(() => {
      hydrateProfile().catch(() => {});
    }, 60000);
    return () => clearInterval(timer);
  }, [allowed]);

  useEffect(() => {
    const timer = window.setInterval(() => setClock(new Date().toLocaleTimeString("ru-RU")), 1000);
    return () => clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!allowed) return undefined;
    const timer = window.setInterval(() => {
      setStaleLive(Date.now() - lastLiveAt.current > 20000);
    }, 4000);
    return () => clearInterval(timer);
  }, [allowed]);

  useEffect(() => {
    const q = planeFilter.trim().toLowerCase();
    if (!q || !aircraft.length) return;
    const list = aircraft.filter((plane) => {
      const hay = `${plane.id} ${plane.point?.[0]} ${plane.point?.[1]}`.toLowerCase();
      return hay.includes(q);
    });
    if (list.length && !list.some((plane) => plane.id === planeId)) {
      setPlaneId(list[0].id);
    }
  }, [planeFilter, aircraft, planeId]);

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
      if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
        event.preventDefault();
        actions.current.assign?.();
      }
      if (event.key === "Escape") {
        setActiveMcc(null);
        setPlaneFilter("");
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
        setAlerts(list);
        if (list.length > alertCountRef.current) softBeep(880, 180);
        alertCountRef.current = list.length;
        setMovements(moves);
        setFleet(transport);
        setAogList(aog);
        if (maps) {
          setMapCatalog(Array.isArray(maps.maps) ? maps.maps : []);
          setActiveMapId((prev) => prev || maps.active_id || "");
        }
        if (!transportId && transport[0]?.id) setTransportId(transport[0].id);
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
    const bag = { map: null, frame: 0, timer: 0, cleanupResize: null };
    let fleet = [];
    let aircraftNow = [];
    let agents = [];
    let busyIds = new Set();
    let showRoute = layersRef.current.route;

    (async () => {
      const tmj = await loadApronMap();
      if (cancelled || !mapNode.current) return;
      const grid = buildTileGrid(tmj);
      const tilesets = await loadTilesetImages();
      if (cancelled || !mapNode.current) return;

      const bounds = [[0, 0], [tmj.height * tmj.tileheight, tmj.width * tmj.tilewidth]];
      const map = L.map(mapNode.current, {
        crs: L.CRS.Simple,
        minZoom: -2,
        maxZoom: 6,
        zoomSnap: 0.25,
        attributionControl: false,
      });
      bag.map = map;
      L.imageOverlay(renderCanvas(tmj, grid, tilesets).toDataURL("image/png"), bounds).addTo(map);
      map.fitBounds(bounds);

      const standsLayer = L.layerGroup().addTo(map);
      const staffLayer = L.layerGroup().addTo(map);
      const transportLayer = L.layerGroup().addTo(map);
      const routeExtrasLayer = L.layerGroup().addTo(map);
      const layerOf = { staff: staffLayer, transport: transportLayer, stands: standsLayer, route: routeExtrasLayer };
      for (const [name, layer] of Object.entries(layerOf)) {
        if (!layersRef.current[name]) map.removeLayer(layer);
      }

      const onResize = () => map.invalidateSize();
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

      function startWalk(id, name, gridRoute) {
        const key = String(id);
        stopWalk(key);
        const centers = gridRoute.map(([x, y]) => [x + 0.5, y + 0.5]);
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

      function tick(now) {
        for (const walk of walks.values()) {
          if (walk.done || !walk.line) continue;
          const traveled = ((now - walk.startedAt) / 1000) * WALK_MPS;
          const sample = pointAt(walk.centers, traveled);
          if (!sample) continue;
          walk.marker.setLatLng(centerToLatLng(tmj, sample.done ? walk.centers.at(-1) : sample.point));
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

      function renderTransportMarkers() {
        transportLayer.clearLayers();
        for (const vehicle of fleet) {
          L.marker(cellToLatLng(tmj, vehicle.point[0], vehicle.point[1]), {
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
        }
      }

      async function refresh() {
        const started = performance.now();
        const staff = await loadStaff();
        if (cancelled) return;
        agents = staff.plotted;
        aircraftNow = await loadAircraft();
        const aog = await loadAogIds();
        fleet = await loadTransport();
        busyIds = await loadBusyEngineerIds();
        if (cancelled) return;
        for (const agent of agents) agent.busy = busyIds.has(String(agent.id));
        setAircraft(aircraftNow);
        setAogIds(aog);
        setPlaneId((prev) => (aircraftNow.some((plane) => plane.id === prev) ? prev : aircraftNow[0]?.id || ""));
        renderAircraftMarkers(aircraftNow, aog);
        renderStaffMarkers();
        renderTransportMarkers();
        const gap = staff.missing.length ? `, без координат: ${staff.missing.length}` : "";
        lastLiveAt.current = Date.now();
        setStaleLive(false);
        setPollMs(Math.round(performance.now() - started));
        const free = agents.filter((agent) => !agent.busy).length;
        setCrewNote(`Свободно ${free} / ${agents.length}`);
        setLive(`Бортов: ${aircraftNow.length} · инженеров на карте: ${agents.length}${gap} · техники: ${fleet.length} · AOG: ${aog.size}`);
      }

      map.on("click", (event) => {
        const cell = clickToCell(tmj, event.latlng.lat, event.latlng.lng);
        if (!cell) {
          setTileInfo("Клик вне карты.");
          return;
        }
        const cells = grid[cell.row][cell.col];
        const hintText = roadHint(cells);
        if (!cells.length) {
          setTileInfo(`Point [${cell.col}, ${cell.row}]${hintText}`);
          return;
        }
        const lines = cells.map((item) => {
          const flip = item.flippedH || item.flippedV || item.flippedD
            ? ` (flip h:${item.flippedH} v:${item.flippedV} d:${item.flippedD})`
            : "";
          return `${item.layerName}: gid=${item.gid}${flip}`;
        });
        setTileInfo(`Point [${cell.col}, ${cell.row}]${hintText}\n${lines.join("\n")}`);
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
        async assign() {
          const form = formRef.current;
          const plane = aircraftNow.find((item) => item.id === form.planeId);
          if (!plane) {
            setResult({ type: "info", text: "Нет воздушного судна. Борта приходят из /api/aircraft, фронт их не создаёт." });
            return;
          }
          const issueValue = form.issue;
          const text = form.description.trim() || issueLabel(issueValue);
          const busyBefore = new Set(busyIds);
          setPending(true);
          setResult({ type: "info", text: "Поиск свободного инженера и спецтранспорта..." });
          try {
            // Контракт assign: classifier_text + note + plane_uuid; issue и request_id опциональны.
            const mccItem = listMcc().find((item) => item.id === form.activeMcc);
            const response = await apiRequest("/api/assign", {
              method: "POST",
              body: {
                classifier_text: text,
                note: text,
                plane_uuid: plane.id,
                issue: issueValue,
                request_id: mccItem?.serverId || null,
              },
            });
            const gridRoute = Array.isArray(response.route) ? response.route.map(([x, y]) => [x, y]) : [];
            const assignedName = await engineerName(response.engineer_uuid);
            routeExtrasLayer.clearLayers();
            startWalk(response.engineer_uuid, assignedName, gridRoute);
            const routeLine = walks.get(String(response.engineer_uuid))?.line || null;
            const pickup = findTransportOnRoute(gridRoute, fleet);
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
            // Таблица только из ответа; пустой/отсутствующий candidates не заполняем.
            const chosenId = String(response.engineer_uuid ?? "");
            const rawCandidates = Array.isArray(response.candidates) ? response.candidates : [];
            const candidates = await Promise.all(rawCandidates.map(async (row) => ({
              engineerId: String(row.engineer_id ?? ""),
              name: await engineerName(row.engineer_id),
              eligible: Boolean(row.eligible),
              reason: row.reason == null ? "" : String(row.reason),
              etaSeconds: row.eta_seconds,
              usesVehicle: Boolean(row.uses_vehicle),
            })));
            candidates.sort((a, b) => {
              if (a.engineerId === chosenId) return -1;
              if (b.engineerId === chosenId) return 1;
              if (a.eligible !== b.eligible) return Number(b.eligible) - Number(a.eligible);
              return Number(a.etaSeconds ?? 1e9) - Number(b.etaSeconds ?? 1e9);
            });
            // Очередь ЦПУ помечаем assigned только здесь, после успешного assign — не при выборе строки.
            if (form.activeMcc && chosenId) {
              markMccAssigned(form.activeMcc);
              setActiveMcc(null);
              setInbox(freshInbox());
            }
            setResult({
              type: "ok",
              name: assignedName,
              plane: plane.id.slice(0, 8),
              issue: issueLabel(issueValue),
              onSite: gridRoute.length < 2,
              cells: Math.max(0, gridRoute.length - 1),
              minutes: Math.max(0, Math.round(Number(response.eta_seconds ?? response.time ?? 0) / 60)),
              pickup: pickup ? pickup.label : "",
              needed: pickup ? "" : (ISSUE_REQUIRED_VEHICLE[issueValue] ? vehicleLabel(ISSUE_REQUIRED_VEHICLE[issueValue]) : ""),
              skipped,
              sla: Boolean(response.sla_exceeded ?? response.time_limit_exceeded),
              chosenId,
              candidates,
              candidateMs: response.candidate_ms,
            });
            softBeep(response.sla_exceeded ? 420 : 880, 160);
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
                issue: issueLabel(issueValue),
                minutes: Math.max(0, Math.round(Number(response.eta_seconds ?? response.time ?? 0) / 60)),
                sla: Boolean(response.sla_exceeded ?? response.time_limit_exceeded),
              };
              setAssignHistory((prev) => {
                const next = [entry, ...prev].slice(0, 8);
                sessionStorage.setItem("oto-assign-history", JSON.stringify(next));
                return next;
              });
            }
            if (routeLine) map.fitBounds(routeLine.getBounds(), { padding: [80, 80], maxZoom: 3 });
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

  function chooseMcc(item) {
    const known = aircraft.some((plane) => plane.id === item.planeUuid);
    skipAuto.current = item.issue !== issue;
    lastAuto.current = item.description;
    setActiveMcc(item.id);
    if (known) setPlaneId(item.planeUuid);
    setIssue(item.issue);
    setDescription(item.description);
    sessionStorage.setItem("oto-disp-draft", item.description || "");
    if (item.serverId) setReassignId(String(item.serverId));
    setResult({
      type: "info",
      text: known
        ? `Замечание ЦПУ подставлено${item.urgency === "urgent" ? " · срочно" : ""}. Назначьте инженера.`
        : "Этого борта нет в /api/aircraft. Замечание не назначено, назначить его нельзя.",
    });
    if (known) {
      window.queueMicrotask?.(() => {
        const btn = document.getElementById("assign-button");
        btn?.scrollIntoView({ block: "nearest", behavior: "smooth" });
        btn?.focus();
      });
    }
  }

  function toggleLayer(name, on) {
    setLayersOn((current) => ({ ...current, [name]: on }));
    actions.current.setLayer?.(name, on);
  }

  function logout() {
    clearSession();
    navigate("/");
  }

  function takeAlert(item) {
    setReassignId(String(item.request_id || ""));
    if (item.alternative_engineer_id) setReassignEngineer(String(item.alternative_engineer_id));
  }

  async function downloadJournal() {
    setJournalPending(true);
    setJournalNote("");
    try {
      await downloadJournalCsv();
      softBeep(520, 120);
      setJournalNote("Файл journal.csv сохранён.");
    } catch (error) {
      setJournalNote(assignErrorMessage(error));
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
    const confirmed = window.confirm("Переназначить может только старший смены. Продолжить?");
    if (!confirmed) return;
    setReassignPending(true);
    setReassignNote("");
    try {
      const data = await reassignRequest(id, engineerId, reason);
      setReassignNote(`Переназначено: ${data?.id || id}${data?.status ? ` · ${data.status}` : ""}`);
    } catch (error) {
      setReassignNote(assignErrorMessage(error));
    } finally {
      setReassignPending(false);
    }
  }

  async function enqueueMcc(item, event) {
    event.stopPropagation();
    if (!item?.serverId) {
      setQueueNote("Нет serverId — сначала запись через ЦПУ на сервер.");
      return;
    }
    setQueuePending(item.id);
    setQueueNote("");
    try {
      const data = await queueRequest(item.serverId);
      markMccQueued(item.id);
      setInbox(freshInbox());
      setQueueNote(`В очереди: ${data?.id || item.serverId}${data?.status ? ` · ${data.status}` : ""}`);
    } catch (error) {
      setQueueNote(assignErrorMessage(error));
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
      setDefectText("");
      setDefectAta("");
      setDefectNote("Дефект добавлен в классификатор.");
    } catch (error) {
      setDefectNote(assignErrorMessage(error));
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
      setClassifyHint(`${name}, уверенность ${conf}. Подсказка, не назначение.`);
    } catch (error) {
      setClassifyHint(assignErrorMessage(error));
    } finally {
      setClassifyPending(false);
    }
  }

  async function onTrainClassifier() {
    setTrainPending(true);
    setTrainNote("");
    try {
      const data = await trainClassifier();
      const acc = data?.accuracy == null ? "—" : `${Math.round(Number(data.accuracy) * 100)}%`;
      setTrainNote(`Обучено: ${data?.documents ?? "?"} док. · accuracy ${acc}`);
    } catch (error) {
      setTrainNote(assignErrorMessage(error));
    } finally {
      setTrainPending(false);
    }
  }

  async function onDepartPlane() {
    if (!planeId) {
      setDepartNote("Выберите борт.");
      return;
    }
    setDepartPending(true);
    setDepartNote("");
    try {
      await departAircraft(planeId);
      setDepartNote(`Вылет зафиксирован: ${planeId.slice(0, 8)}`);
      const moves = await loadMovements(12);
      setMovements(moves);
    } catch (error) {
      setDepartNote(assignErrorMessage(error));
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
    setMapPending(true);
    setMapNote("");
    try {
      const data = await setActiveMap(activeMapId);
      setMapNote(`Активна: ${data?.name || activeMapId} · ${data?.width || "?"}×${data?.height || "?"}`);
      const maps = await listMaps();
      setMapCatalog(Array.isArray(maps.maps) ? maps.maps : []);
      setActiveMapId(maps.active_id || activeMapId);
    } catch (error) {
      setMapNote(assignErrorMessage(error));
    } finally {
      setMapPending(false);
    }
  }

  async function onSendTrack(event) {
    event.preventDefault();
    const lat = Number(trackLat);
    const lon = Number(trackLon);
    if (!trackObj.trim() || !Number.isFinite(lat) || !Number.isFinite(lon)) {
      setTrackNote("Нужны obj, lat, lon.");
      return;
    }
    setTrackPending(true);
    setTrackNote("");
    try {
      const ack = await postTrackPoint({ obj: trackObj.trim(), lat, lon });
      localStorage.setItem("oto-track-obj", trackObj.trim());
      localStorage.setItem("oto-track-lat", String(lat));
      localStorage.setItem("oto-track-lon", String(lon));
      setTrackNote(`track: accepted=${ack?.accepted} applied=${ack?.applied} obj=${ack?.obj || trackObj}`);
    } catch (error) {
      setTrackNote(assignErrorMessage(error));
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
      setTransportNote(`Техника ${vehicle.label}: ${x}, ${y}`);
      const next = await loadTransport();
      setFleet(next);
    } catch (error) {
      setTransportNote(assignErrorMessage(error));
    } finally {
      setTransportPending(false);
    }
  }

  async function onPreviewRoute() {
    const plane = aircraft.find((item) => item.id === planeId);
    if (!plane?.point) {
      setRouteNote("Выберите борт.");
      return;
    }
    setRoutePending(true);
    setRouteNote("");
    try {
      const [staff, busy] = await Promise.all([loadStaff(), loadBusyEngineerIds()]);
      const free = (staff.plotted || []).find((agent) => !busy.has(String(agent.id)) && Array.isArray(agent.point));
      if (!free?.point) {
        setRouteNote("Нет свободного инженера с координатами для пробы маршрута.");
        return;
      }
      const data = await getRoute(free.point, plane.point);
      const cells = Number(data?.distance);
      setRouteNote(`Проба: ${free.name || "инженер"} → борт · ${Number.isFinite(cells) ? cells : "?"} клеток`);
    } catch (error) {
      setRouteNote(assignErrorMessage(error));
    } finally {
      setRoutePending(false);
    }
  }

  async function onArrivePlane() {
    const plane = aircraft.find((item) => item.id === planeId);
    if (!plane?.point) {
      setArriveNote("Выберите борт со стоянкой.");
      return;
    }
    setArrivePending(true);
    setArriveNote("");
    try {
      const data = await arriveAircraft(plane.point, plane.id);
      setArriveNote(`Прибытие: ${(data?.aircraft_id || plane.id).slice(0, 8)}`);
      const moves = await loadMovements(12);
      setMovements(moves);
    } catch (error) {
      setArriveNote(assignErrorMessage(error));
    } finally {
      setArrivePending(false);
    }
  }

  return (
    <div className="dispatcher-page">
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
          <span>{session.name}{session.email ? ` · ${session.email}` : ""}</span>
          <span className={staleLive ? "live-label stale" : "live-label"}>
            {crewNote ? `${crewNote} · ` : ""}
            {pollMs != null ? `${pollMs} мс · ` : ""}
            {staleLive ? `STALE · ${clock}` : clock}
          </span>
        </div>
        <StatusChain current={dispatcherStatusStep(result, inbox, activeMcc)} />
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
            <button type="submit" className="shift-button" disabled={mapPending || !mapCatalog.length}>
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
                onClick={() => setPlaneId(row.id)}
              >
                <strong>{row.id.slice(0, 8)}</strong>
                <span>{row.point ? `${row.point[0]}, ${row.point[1]}` : "без точки"}</span>
              </button>
            )) : <p className="mcc-empty">Нет AOG.</p>}
          </div>
        </details>
        <details className="request-panel" open={assignHistory.length > 0}>
          <summary>Недавние назначения · {assignHistory.length}</summary>
          <div className="mcc-inbox">
            {assignHistory.length ? assignHistory.map((row, index) => (
              <div className="mcc-item" key={`${row.at}-${index}`}>
                <strong>{row.name} · {row.plane}</strong>
                <span>{row.issue} · ETA {row.minutes} мин{row.sla ? " · SLA" : ""} · {relativeRu(row.at)}</span>
              </div>
            )) : <p className="mcc-empty">Пока пусто в этой вкладке.</p>}
          </div>
        </details>
        <details className="request-panel" open={alerts.length > 0}>
          <summary>Риск норматива · {alerts.length}</summary>
          <div className="mcc-inbox">
            {alerts.length ? alerts.map((item, index) => {
              const minutes = marginMinutes(item.alternative_margin_seconds);
              const alt = item.alternative_engineer_id
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
          <button type="button" className="shift-button" disabled={journalPending} onClick={downloadJournal}>Скачать журнал</button>
          {journalNote ? <p className="fault-hint">{journalNote}</p> : null}
        </details>
        <details className="request-panel" open>
          <summary>Заявки ЦПУ · {inbox.length}</summary>
          <div className="mcc-inbox">
            {inbox.length ? inbox.map((item) => (
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
                  <span>Борт {String(item.planeUuid).slice(0, 8)} · {item.status || "не назначена"} · {relativeRu(item.createdAt)}</span>
                  <span>{item.description}</span>
                </button>
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
            )) : <p className="mcc-empty">Нет новых замечаний от ЦПУ.</p>}
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
                onClick={() => {
                  if (row.aircraft_id) setPlaneId(String(row.aircraft_id));
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
              <input type="text" value={defectText} autoComplete="off" onChange={(event) => setDefectText(event.target.value)} />
            </label>
            <button
              type="button"
              className="shift-button"
              disabled={!description.trim()}
              onClick={() => setDefectText(description.trim())}
            >
              Взять из описания заявки
            </button>
            <label>Глава ATA
              <input type="text" value={defectAta} autoComplete="off" onChange={(event) => setDefectAta(event.target.value)} />
            </label>
            <button type="submit" className="shift-button" disabled={defectPending}>Добавить дефект</button>
          </form>
          {defectNote ? <p className="fault-hint">{defectNote}</p> : null}
          <button type="button" className="shift-button" disabled={trainPending} onClick={onTrainClassifier}>
            {trainPending ? "Обучение…" : "Переобучить классификатор"}
          </button>
          {trainNote ? <p className="fault-hint">{trainNote}</p> : null}
        </details>
        <section className="request-panel">
          <div className="section-heading"><h2>Новая заявка</h2><span className="live-label">LIVE</span></div>
          <label>Фильтр бортов
            <input
              type="search"
              value={planeFilter}
              placeholder="uuid или координата"
              autoComplete="off"
              onChange={(event) => setPlaneFilter(event.target.value)}
            />
          </label>
          <label>Воздушное судно
            <select value={planeId} disabled={!aircraft.length} onChange={(event) => setPlaneId(event.target.value)}>
              {(() => {
                const q = planeFilter.trim().toLowerCase();
                const filtered = q
                  ? aircraft.filter((plane) => {
                    const hay = `${plane.id} ${plane.point?.[0]} ${plane.point?.[1]}`.toLowerCase();
                    return hay.includes(q);
                  })
                  : aircraft;
                const list = filtered.slice().sort((a, b) => Number(aogIds.has(b.id)) - Number(aogIds.has(a.id)));
                if (!list.length) return <option value="">Нет совпадений</option>;
                return list.map((plane) => (
                  <option key={plane.id} value={plane.id}>
                    {plane.id.slice(0, 8)} · {plane.point[0]}, {plane.point[1]}{aogIds.has(plane.id) ? " · AOG" : ""}
                  </option>
                ));
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
            }}
          >
            Копировать UUID борта
          </button>
          {planeId && aogIds.has(planeId) ? <p className="fault-hint stale-live">Выбранный борт в AOG.</p> : null}
          {!aircraft.length ? <p className="fault-hint stale-live">Нет бортов в Redis. Список приходит из /api/aircraft.</p> : null}
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
          <button type="button" className="shift-button" disabled={classifyPending || pending} onClick={onClassifyDescription}>
            {classifyPending ? "Классификация…" : "Подсказать тип инженера"}
          </button>
          {classifyHint ? <p className="fault-hint">{classifyHint}</p> : null}
          <button type="button" className="shift-button" disabled={arrivePending || !planeId} onClick={onArrivePlane}>
            {arrivePending ? "Фиксация…" : "Зафиксировать прибытие борта"}
          </button>
          {arriveNote ? <p className="fault-hint">{arriveNote}</p> : null}
          <button type="button" className="shift-button" disabled={departPending || !planeId} onClick={onDepartPlane}>
            {departPending ? "Фиксация…" : "Зафиксировать вылет борта"}
          </button>
          {departNote ? <p className="fault-hint">{departNote}</p> : null}
          <button type="button" className="shift-button" disabled={routePending || !planeId} onClick={onPreviewRoute}>
            {routePending ? "Считаем…" : "Проба маршрута getRoute"}
          </button>
          {routeNote ? <p className="fault-hint">{routeNote}</p> : null}
          <button
            id="assign-button"
            type="button"
            disabled={pending}
            aria-busy={pending}
            onClick={() => {
              if (pending) return;
              actions.current.assign?.();
            }}
          >
            {pending ? "Назначение…" : "Назначить инженера · Ctrl+Enter"}
          </button>
          <details className="shortcut-help">
            <summary>Горячие клавиши</summary>
            <ul>
              <li><kbd>Ctrl</kbd>+<kbd>Enter</kbd> — назначить инженера</li>
              <li><kbd>Esc</kbd> — снять выбор ЦПУ и фильтр бортов</li>
            </ul>
          </details>
          {staleLive ? <p className="fault-hint stale-live">Карта давно не обновлялась — проверьте сеть или API.</p> : null}
          <div className={result.type === "ok" ? "assignment-result success" : "assignment-result"}>
            {result.type === "ok" ? (
              <>
                <strong>{result.name}</strong><br />
                Борт {result.plane} · {result.issue}<br />
                {result.onSite ? "Инженер уже у воздушного судна, маршрут пустой." : <>Маршрут: <strong>{result.cells} клеток</strong><br />В пути: <strong>{result.minutes} мин</strong></>}
                {result.pickup ? <><br />Маршрут заезжает за спецтранспорт: <strong>{result.pickup}</strong>.</> : null}
                {result.needed ? <><br />Для неисправности нужен: <strong>{result.needed}</strong>.</> : null}
                {result.skipped ? <><br /><em>Занятые инженеры пропущены ({result.skipped}).</em></> : null}
                {result.sla ? <><br /><em>Время в пути больше 15 минут.</em></> : null}
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
                      result.sla ? "SLA превышен" : null,
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
                      const header = "engineer_id;name;eligible;reason;eta_min;vehicle;chosen";
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
                              title="Клик — подставить в переназначение"
                              onClick={() => {
                                setReassignEngineer(row.engineerId);
                                setLive(`В переназначение: ${row.name}`);
                              }}
                              style={{ cursor: "pointer" }}
                            >
                              <td>
                                <strong>{row.name}</strong>
                                <span className="candidate-id">{row.engineerId.slice(0, 8)}</span>
                                {chosen ? <span className="candidate-badge">выбран</span> : null}
                              </td>
                              <td>{row.eligible ? "да" : "нет"}</td>
                              <td>{chosen ? "—" : (row.reason || "—")}</td>
                              <td>{etaMin}</td>
                              <td>{row.usesVehicle ? "да" : "нет"}</td>
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
          <summary>Имитатор track</summary>
          <form onSubmit={onSendTrack}>
            <label>obj<input type="text" value={trackObj} autoComplete="off" onChange={(event) => setTrackObj(event.target.value)} /></label>
            <label>lat<input type="text" value={trackLat} autoComplete="off" onChange={(event) => setTrackLat(event.target.value)} /></label>
            <label>lon<input type="text" value={trackLon} autoComplete="off" onChange={(event) => setTrackLon(event.target.value)} /></label>
            <button type="submit" className="shift-button" disabled={trackPending}>POST /api/v1/track</button>
          </form>
          {trackNote ? <p className="fault-hint">{trackNote}</p> : null}
        </details>
        <details className="request-panel">
          <summary>Спецтранспорт</summary>
          <form onSubmit={onMoveTransport}>
            <label>Техника
              <select value={transportId} onChange={(event) => setTransportId(event.target.value)}>
                {fleet.length ? fleet.map((item) => (
                  <option key={item.id} value={item.id}>{item.label} · {item.id.slice(0, 8)}</option>
                )) : <option value="">Нет техники</option>}
              </select>
            </label>
            <label>X<input type="text" value={transportX} autoComplete="off" onChange={(event) => setTransportX(event.target.value)} /></label>
            <label>Y<input type="text" value={transportY} autoComplete="off" onChange={(event) => setTransportY(event.target.value)} /></label>
            <button type="submit" className="shift-button" disabled={transportPending || !fleet.length}>Сдвинуть технику</button>
          </form>
          {transportNote ? <p className="fault-hint">{transportNote}</p> : null}
        </details>
        <details className="request-panel">
          <summary>Переназначение</summary>
          <form onSubmit={submitReassign}>
            <label>Заявка
              <input type="text" value={reassignId} autoComplete="off" onChange={(event) => setReassignId(event.target.value)} />
            </label>
            <label>Инженер
              <input type="text" value={reassignEngineer} autoComplete="off" onChange={(event) => setReassignEngineer(event.target.value)} />
            </label>
            <label>Причина
              <input type="text" value={reassignReason} autoComplete="off" onChange={(event) => setReassignReason(event.target.value)} />
            </label>
            <button type="submit" className="shift-button" disabled={reassignPending}>Переназначить</button>
          </form>
          {reassignNote ? <p className="fault-hint">{reassignNote}</p> : null}
        </details>
        <details className="request-panel">
          <summary>Отображение</summary>
          <div>
            <label className="layer-row"><input type="checkbox" checked={layersOn.staff} onChange={(event) => toggleLayer("staff", event.target.checked)} /> Сотрудники</label>
            <label className="layer-row"><input type="checkbox" checked={layersOn.transport} onChange={(event) => toggleLayer("transport", event.target.checked)} /> Спецтранспорт</label>
            <label className="layer-row"><input type="checkbox" checked={layersOn.stands} onChange={(event) => toggleLayer("stands", event.target.checked)} /> Воздушные суда</label>
            <label className="layer-row"><input type="checkbox" checked={layersOn.route} onChange={(event) => toggleLayer("route", event.target.checked)} /> Маршрут</label>
          </div>
        </details>
        <details className="request-panel">
          <summary>Координаты</summary>
          <div id="tile-info">{tileInfo}</div>
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
