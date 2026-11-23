// Диспетчерская карта: тайлы перрона, маркеры инженеров/техники, назначение через /api/assign.
// ИИ: черновик отрисовки тайлов/маркеров и демо-фильтров; координаты Point и /api/assign — сверил с бэком.
const CONFIG = {
  // Визуальная карта перрона (ассеты лежат в frontend/)
  mapCandidates: ["./real_map.tmj", "../et.tmj", "./et.tmj", "/et.tmj"],
  tilesets: [
    {
      firstgid: 973,
      path: "assets/tileset_custom.png",
      columns: 5,
      tileWidth: 16,
      tileHeight: 16,
      tilecount: 10,
    },
    {
      firstgid: 999,
      path: "assets/plane.png",
      columns: 3,
      tileWidth: 16,
      tileHeight: 16,
      tilecount: 9,
    },
    // В исходном .tmx второй tileset ссылается на тайд.tsx с тем же листом
    {
      firstgid: 1008,
      path: "assets/tileset_custom.png",
      columns: 5,
      tileWidth: 16,
      tileHeight: 16,
      tilecount: 10,
    },
  ],
};

const VEHICLE_LABELS = {
  FuelTruck: "Топливозаправщик",
  OilCart: "Маслораздаточная тележка",
  MaintenanceLift: "Аэродромный подъёмник",
  BorescopeCart: "Бороскоп-тележка",
  AvionicsTestSet: "КПА авионики",
  Aircraft: "Воздушное судно",
  fuel_truck: "Топливозаправщик",
  oil_cart: "Маслораздаточная тележка",
  maintenance_lift: "Аэродромный подъёмник",
  borescope_cart: "Бороскоп-тележка",
  avionics_test_set: "КПА авионики",
};

const ISSUE_REQUIRED_VEHICLE = {
  fuel_leak_from_drain_cap: "FuelTruck",
  oil_stain_near_gearbox: "OilCart",
  fairing_chip_or_scratch: "MaintenanceLift",
  paint_peeling_at_rivets: "MaintenanceLift",
  missing_pitot_cover: "MaintenanceLift",
  indication_fault: "AvionicsTestSet",
  loose_connector: "AvionicsTestSet",
  thrust_or_parameter_drop: "BorescopeCart",
  excessive_vibration: "BorescopeCart",
  metal_debris_in_oil_filter: "BorescopeCart",
  radar_failure_or_false_reading: "AvionicsTestSet",
  comms_loss_or_distortion: "AvionicsTestSet",
  ins_gyro_drift: "AvionicsTestSet",
};

const BACKEND_ROAD_IDS = new Set([0, 29]);
const FLIP_H = 0x80000000;
const FLIP_V = 0x40000000;
const FLIP_D = 0x20000000;

const FALLBACK_TILE_COLORS = {
  973: "#9aa3a8",
  974: "#1f4fd6",
  975: "#5eb8ff",
  976: "#4a5560",
  977: "#1f4fd6",
  978: "#3d4650",
  979: "#f4f6f8",
  980: "#4a5560",
  981: "#4a5560",
  982: "#f4f6f8",
};

function decodeGid(raw) {
  // Флаги flip из Tiled (без ИИ не выдумывать битовые маски — из спецификации Tiled).
  let gid = raw;
  let flippedH = false;
  let flippedV = false;
  let flippedD = false;

  if (gid >= FLIP_H) {
    flippedH = true;
    gid -= FLIP_H;
  }
  if (gid >= FLIP_V) {
    flippedV = true;
    gid -= FLIP_V;
  }
  if (gid >= FLIP_D) {
    flippedD = true;
    gid -= FLIP_D;
  }
  return { gid, flippedH, flippedV, flippedD };
}

function issueGroups() {
  return [
    ["Герметичность систем", ["fuel_leak_from_drain_cap", "oil_stain_near_gearbox", "hydraulic_leak_on_strut"]],
    ["Внешние повреждения", ["fairing_chip_or_scratch", "paint_peeling_at_rivets"]],
    ["Шасси и пневматика", ["missing_pitot_cover", "uneven_tread_wear", "tire_cut_to_cord", "low_tire_pressure"]],
    ["Замечания экипажа", ["indication_fault", "loose_connector", "seatbelt_adjustment", "burned_out_signal_lamp"]],
    ["Двигатель", ["thrust_or_parameter_drop", "excessive_vibration", "metal_debris_in_oil_filter"]],
    ["Авионика", ["radar_failure_or_false_reading", "comms_loss_or_distortion", "ins_gyro_drift"]],
    ["Прочее", ["other"]],
  ];
}

const nameCache = new Map();

async function engineerName(id) {
  const key = String(id);
  if (nameCache.has(key)) return nameCache.get(key);
  try {
    const data = await AeroAuth.apiRequest(`/api/auth/getEngineerName/${key}`, { auth: false });
    const name = data && data.name ? data.name : `Инженер ${key.slice(0, 8)}`;
    nameCache.set(key, name);
    return name;
  } catch (error) {
    const fallback = `Инженер ${key.slice(0, 8)}`;
    nameCache.set(key, fallback);
    return fallback;
  }
}

function asPoint(point) {
  if (!Array.isArray(point) || point.length < 2) return null;
  return [Number(point[0]), Number(point[1])];
}

async function loadStaff() {
  let engineers = [];
  try {
    engineers = await AeroAuth.apiRequest("/api/geo/getEngineersPositions");
  } catch (error) {
    console.warn("[map] не удалось загрузить список сотрудников", error);
    return { plotted: [], missing: [] };
  }
  if (!Array.isArray(engineers)) return { plotted: [], missing: [] };

  const plotted = [];
  const missing = [];
  await Promise.all(
    engineers.map(async (entry) => {
      const id = String(entry.id);
      const name = await engineerName(id);
      const point = asPoint(entry.position);
      if (!point) {
        missing.push({ id, name });
        return;
      }
      plotted.push({ id, name, point, busy: false });
    })
  );
  plotted.sort((a, b) => a.name.localeCompare(b.name, "ru"));
  return { plotted, missing };
}

async function loadAircraft() {
  try {
    const data = await AeroAuth.apiRequest("/api/aircraft");
    const list = data && Array.isArray(data.aircraft) ? data.aircraft : [];
    return list
      .map((item) => ({ id: String(item.id), point: asPoint(item.point) }))
      .filter((item) => item.point);
  } catch (error) {
    console.warn("[map] не удалось загрузить воздушные суда", error);
    return [];
  }
}

async function loadAogIds() {
  try {
    const data = await AeroAuth.apiRequest("/api/tasks/aog");
    const ids = data && Array.isArray(data.plane_uuids) ? data.plane_uuids : [];
    return new Set(ids.map(String));
  } catch (error) {
    console.warn("[map] не удалось загрузить AOG", error);
    return new Set();
  }
}

async function loadBusyEngineerIds() {
  try {
    const data = await AeroAuth.apiRequest("/api/geo/activeEngineers");
    const ids = data && data.active_engineers ? data.active_engineers : [];
    return new Set(ids.map(String));
  } catch (error) {
    console.warn("[map] не удалось загрузить занятых инженеров", error);
    return new Set();
  }
}

function vehicleLabel(type) {
  return VEHICLE_LABELS[type] || String(type);
}

async function loadTransport() {
  try {
    const list = await AeroAuth.apiRequest("/api/geo/getTransportPositions");
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

function findTransportOnRoute(route, fleet) {
  if (!route || route.length < 3 || !fleet.length) return null;
  const middle = route.slice(1, -1);
  for (const vehicle of fleet) {
    const hit = middle.find(([x, y]) => x === vehicle.point[0] && y === vehicle.point[1]);
    if (hit) return vehicle;
  }
  return null;
}

async function loadMap() {
  for (const path of CONFIG.mapCandidates) {
    try {
      const res = await fetch(path);
      if (!res.ok) continue;
      const json = await res.json();
      console.info(`[map] загружено из ${path}`);
      return json;
    } catch (e) {
      // пробуем следующий кандидат
    }
  }

  try {
    const matrix = await AeroAuth.apiRequest("/api/map", { method: "GET" });
    if (Array.isArray(matrix) && Array.isArray(matrix[0])) {
      console.info("[map] fallback: загружено из /api/map");
      return mapFromMatrix(matrix);
    }
  } catch (e) {
    console.warn("[map] /api/map недоступен", e);
  }

  throw new Error(
    "Не удалось получить карту. Проверь frontend/real_map.tmj и что backend запущен."
  );
}

function mapFromMatrix(matrix) {
  const height = matrix.length;
  const width = matrix[0].length;
  return {
    width,
    height,
    tilewidth: 16,
    tileheight: 16,
    layers: [
      {
        name: "дорога",
        type: "tilelayer",
        width,
        height,
        data: matrix.flat(),
      },
    ],
  };
}

function buildTileGrid(tmj) {
  const { width, height } = tmj;
  const grid = Array.from({ length: height }, () =>
    Array.from({ length: width }, () => [])
  );

  for (const layer of tmj.layers) {
    if (layer.type !== "tilelayer") continue;
    for (let row = 0; row < layer.height; row++) {
      for (let col = 0; col < layer.width; col++) {
        const raw = layer.data[row * layer.width + col];
        if (!raw) continue;
        const decoded = decodeGid(raw);
        grid[row][col].push({ layerName: layer.name, ...decoded });
      }
    }
  }
  return grid;
}

function loadImage(path) {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error(`Не удалось загрузить ${path}`));
    image.src = path;
  });
}

async function loadTilesetImages() {
  const loaded = [];
  for (const tileset of CONFIG.tilesets) {
    try {
      const image = await loadImage(tileset.path);
      loaded.push({ ...tileset, image });
    } catch (error) {
      console.warn("[map] tileset недоступен", tileset.path, error);
    }
  }
  return loaded;
}

function findTileset(tilesets, gid) {
  let matched = null;
  for (const tileset of tilesets) {
    if (gid >= tileset.firstgid) matched = tileset;
  }
  return matched;
}

function drawTile(ctx, tilesets, cell, dx, dy, tileW, tileH) {
  const tileset = findTileset(tilesets, cell.gid);
  if (tileset && tileset.image) {
    const localId = cell.gid - tileset.firstgid;
    if (localId >= 0 && localId < tileset.tilecount) {
      const sx = (localId % tileset.columns) * tileset.tileWidth;
      const sy = Math.floor(localId / tileset.columns) * tileset.tileHeight;
      ctx.save();
      ctx.translate(dx + tileW / 2, dy + tileH / 2);
      ctx.scale(cell.flippedH ? -1 : 1, cell.flippedV ? -1 : 1);
      if (cell.flippedD) ctx.rotate(Math.PI / 2);
      ctx.drawImage(
        tileset.image,
        sx,
        sy,
        tileset.tileWidth,
        tileset.tileHeight,
        -tileW / 2,
        -tileH / 2,
        tileW,
        tileH
      );
      ctx.restore();
      return;
    }
  }

  const color =
    FALLBACK_TILE_COLORS[cell.gid] ||
    FALLBACK_TILE_COLORS[((cell.gid - 1008 + 973) % 10) + 973] ||
    "#5a6570";
  ctx.fillStyle = color;
  ctx.fillRect(dx, dy, tileW, tileH);
}

function renderCanvas(tmj, grid, tilesets) {
  // ИИ: сборка canvas-фона из тайлов для Leaflet imageOverlay.
  const tileW = tmj.tilewidth;
  const tileH = tmj.tileheight;
  const canvas = document.createElement("canvas");
  canvas.width = tmj.width * tileW;
  canvas.height = tmj.height * tileH;
  const ctx = canvas.getContext("2d");

  ctx.fillStyle = "#d7e3ee";
  ctx.fillRect(0, 0, canvas.width, canvas.height);

  for (let row = 0; row < tmj.height; row++) {
    for (let col = 0; col < tmj.width; col++) {
      const cells = grid[row][col];
      if (cells.length === 0) continue;
      for (const cell of cells) {
        drawTile(ctx, tilesets, cell, col * tileW, row * tileH, tileW, tileH);
      }
    }
  }

  return canvas;
}

function assignErrorMessage(err) {
  // Тексты для жюри: занятость / отсутствие специалиста / нет техники.
  // ИИ: формулировки UI; коды/фразы ошибок сверять с реальными ответами Axum.
  const raw = String(err.message || err || "");
  if (/no suitable engineer available/i.test(raw)) {
    return "На данную исправность отсутствует подходящий специалист";
  }
  if (/engineer was not found/i.test(raw)) {
    return "На данную исправность отсутствует подходящий специалист";
  }
  if (/no available transport/i.test(raw)) {
    return "Нет свободного спецтранспорта нужного типа для этой неисправности.";
  }
  if (/aircraft not found/i.test(raw)) {
    return "Самолёт не найден в Redis. Список бортов берётся из /api/aircraft.";
  }
  if (/not implemented|todo/i.test(raw) || /other/i.test(raw) && /500/.test(raw)) {
    return "Тип «Другое» сервер пока не назначает.";
  }
  if (/only dispatchers/i.test(raw)) {
    return "Назначать может только диспетчер. Перелогиньтесь под ролью Dispatcher.";
  }
  return raw || "Неизвестная ошибка назначения";
}

async function main() {
  const session = AeroAuth.requireRole("Dispatcher");
  if (!session) return;

  document.getElementById("dispatcher-name").textContent = session.name;
  document.getElementById("logout-button").addEventListener("click", () => AeroAuth.logout());

  const tmj = await loadMap();
  const grid = buildTileGrid(tmj);
  const tilesets = await loadTilesetImages();

  const tileW = tmj.tilewidth;
  const tileH = tmj.tileheight;
  const pxWidth = tmj.width * tileW;
  const pxHeight = tmj.height * tileH;

  const bounds = [
    [0, 0],
    [pxHeight, pxWidth],
  ];

  const map = L.map("map", {
    crs: L.CRS.Simple,
    minZoom: -2,
    maxZoom: 6,
    zoomSnap: 0.25,
    attributionControl: false,
  });
  window.addEventListener("resize", () => map.invalidateSize());

  let overlay = null;
  function redraw() {
    const canvas = renderCanvas(tmj, grid, tilesets);
    const dataUrl = canvas.toDataURL("image/png");
    if (overlay) map.removeLayer(overlay);
    overlay = L.imageOverlay(dataUrl, bounds).addTo(map);
  }

  redraw();
  map.fitBounds(bounds);

  const standSelect = document.getElementById("stand-select");

  // Backend Point = [x, y] = [col, row]
  const agentPoint = (agent) => [
    pxHeight - (agent.point[1] + 0.5) * tileH,
    (agent.point[0] + 0.5) * tileW,
  ];

  const standsLayer = L.layerGroup().addTo(map);
  const staffLayer = L.layerGroup().addTo(map);
  const transportLayer = L.layerGroup().addTo(map);
  const routeExtrasLayer = L.layerGroup().addTo(map);

  let agents = [];
  let missingEngineers = [];
  let aircraft = [];
  let aogIds = new Set();
  let fleet = [];
  let busyIds = new Set();

  function fillAircraftSelect() {
    const previous = standSelect.value;
    standSelect.replaceChildren();
    if (!aircraft.length) {
      standSelect.add(new Option("Нет бортов в Redis", ""));
      standSelect.disabled = true;
      return;
    }
    standSelect.disabled = false;
    for (const plane of aircraft) {
      const aog = aogIds.has(plane.id) ? " · AOG" : "";
      standSelect.add(new Option(`${plane.id.slice(0, 8)} · ${plane.point[0]}, ${plane.point[1]}${aog}`, plane.id));
    }
    if ([...standSelect.options].some((option) => option.value === previous)) standSelect.value = previous;
  }

  function renderAircraftMarkers() {
    standsLayer.clearLayers();
    for (const plane of aircraft) {
      const aog = aogIds.has(plane.id);
      L.marker(agentPoint(plane), {
        icon: L.divIcon({
          className: aog ? "aircraft-marker aog" : "aircraft-marker",
          html: "",
          iconSize: [16, 16],
          iconAnchor: [8, 8],
        }),
      })
        .bindTooltip(`${aog ? "AOG · " : ""}ВС ${plane.id.slice(0, 8)}`, {
          permanent: true,
          direction: "top",
          className: "map-label",
          offset: [0, -6],
        })
        .addTo(standsLayer);
    }
  }

  function applyBusyFlags() {
    for (const agent of agents) {
      agent.busy = busyIds.has(String(agent.id));
    }
  }

  const walks = new Map();

  function centerLatLng(center) {
    return [pxHeight - center[1] * tileH, center[0] * tileW];
  }

  function routeCenters(gridRoute) {
    return gridRoute.map(([x, y]) => [x + 0.5, y + 0.5]);
  }

  function staffMarker(latlng, busy, label) {
    return L.marker(latlng, {
      icon: L.divIcon({
        className: `agent-marker ${busy ? "agent-busy" : "agent-free"}`,
        html: "",
        iconSize: [28, 28],
        iconAnchor: [14, 14],
      }),
    })
      .bindTooltip(label, { direction: "top" })
      .addTo(staffLayer);
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
    const centers = routeCenters(gridRoute);
    if (centers.length < 2) return;
    const showRoute = document.getElementById("toggle-route").checked;
    const line = L.polyline(centers.map(centerLatLng), {
      color: "#c5161c",
      weight: 5,
      opacity: 0.9,
      dashArray: "10 8",
    });
    if (showRoute) line.addTo(map);
    const marker = staffMarker(centerLatLng(centers[0]), true, `${name} · идёт к ВС`);
    walks.set(key, { centers, startedAt: performance.now(), line, marker, name });
  }

  function tickWalks(now) {
    for (const walk of walks.values()) {
      if (walk.done) continue;
      const traveled = ((now - walk.startedAt) / 1000) * AeroWalk.WALK_MPS;
      const sample = AeroWalk.pointAt(walk.centers, traveled);
      if (!sample) continue;
      walk.marker.setLatLng(centerLatLng(sample.done ? walk.centers.at(-1) : sample.point));
      if (sample.done) {
        if (walk.line) map.removeLayer(walk.line);
        walk.line = null;
        walk.marker.setTooltipContent(`${walk.name} · на месте`);
        walk.done = true;
        continue;
      }
      const rest = walk.centers.slice(sample.nextIndex).map(centerLatLng);
      walk.line.setLatLngs([centerLatLng(sample.point), ...rest]);
    }
    requestAnimationFrame(tickWalks);
  }

  requestAnimationFrame(tickWalks);

  function renderStaffMarkers() {
    applyBusyFlags();
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
      staffMarker(agentPoint(agent), agent.busy, `${agent.name} · ${statusText}`);
    }
    for (const [id, walk] of walks) {
      if (!seen.has(id)) walk.marker.addTo(staffLayer);
    }
  }

  function renderTransportMarkers() {
    transportLayer.clearLayers();
    for (const vehicle of fleet) {
      L.marker(agentPoint(vehicle), {
        icon: L.divIcon({
          className: "transport-marker",
          html: "",
          iconSize: [18, 18],
          iconAnchor: [9, 9],
        }),
      })
        .bindTooltip(`${vehicle.label}`, {
          permanent: true,
          direction: "top",
          className: "map-label",
          offset: [0, -8],
        })
        .addTo(transportLayer);
    }
  }

  async function refreshOverlayState() {
    const staff = await loadStaff();
    agents = staff.plotted;
    missingEngineers = staff.missing;
    aircraft = await loadAircraft();
    aogIds = await loadAogIds();
    fleet = await loadTransport();
    busyIds = await loadBusyEngineerIds();
    fillAircraftSelect();
    renderAircraftMarkers();
    renderStaffMarkers();
    renderTransportMarkers();
    const live = document.getElementById("live-status");
    if (live) {
      const gap = missingEngineers.length ? `, без координат: ${missingEngineers.length}` : "";
      live.textContent = `Бортов: ${aircraft.length} · инженеров на карте: ${agents.length}${gap} · техники: ${fleet.length} · AOG: ${aogIds.size}`;
    }
  }

  await refreshOverlayState();
  window.setInterval(refreshOverlayState, 5000);

  const faultSelect = document.getElementById("fault-select");
  const issueLabels = new Map(AeroAuth.AIRCRAFT_ISSUES);
  for (const [groupLabel, issueValues] of issueGroups()) {
    const group = document.createElement("optgroup");
    group.label = groupLabel;
    for (const value of issueValues) {
      if (value === "other") continue;
      group.append(new Option(issueLabels.get(value), value));
    }
    if (!group.children.length) continue;
    faultSelect.append(group);
  }
  const faultHint = document.getElementById("fault-hint");
  const faultDescription = document.getElementById("fault-description");
  let lastAutoDescription = "";
  function updateFaultHint() {
    const issue = faultSelect.value;
    const vehicleType = ISSUE_REQUIRED_VEHICLE[issue];
    const vehicleText = vehicleType
      ? `Спецтранспорт: ${vehicleLabel(vehicleType)}.`
      : "Спецтранспорт не требуется.";
    faultHint.textContent = `Нужный специалист: ${AeroAuth.engineerTypeForIssue(issue)}. ${vehicleText}`;
    const nextAuto = `Обнаружена неисправность: ${AeroAuth.issueLabel(issue).toLowerCase()}. Требуется осмотр и устранение.`;
    if (!faultDescription.value.trim() || faultDescription.value === lastAutoDescription) {
      faultDescription.value = nextAuto;
      lastAutoDescription = nextAuto;
    }
  }
  faultSelect.addEventListener("change", updateFaultHint);
  updateFaultHint();

  document.getElementById("toggle-staff").addEventListener("change", (event) =>
    event.target.checked ? staffLayer.addTo(map) : map.removeLayer(staffLayer)
  );
  document.getElementById("toggle-transport").addEventListener("change", (event) =>
    event.target.checked ? transportLayer.addTo(map) : map.removeLayer(transportLayer)
  );
  document.getElementById("toggle-stands").addEventListener("change", (event) =>
    event.target.checked ? standsLayer.addTo(map) : map.removeLayer(standsLayer)
  );
  document.getElementById("toggle-route").addEventListener("change", (event) => {
    for (const walk of walks.values()) {
      if (!walk.line) continue;
      if (event.target.checked) walk.line.addTo(map);
      else map.removeLayer(walk.line);
    }
    if (event.target.checked) routeExtrasLayer.addTo(map);
    else map.removeLayer(routeExtrasLayer);
  });

  const result = document.getElementById("assignment-result");
  document.getElementById("assign-button").addEventListener("click", async () => {
    const plane = aircraft.find((item) => item.id === standSelect.value);
    if (!plane) {
      result.className = "assignment-result";
      result.textContent = "Нет воздушного судна. Борта приходят из /api/aircraft, фронт их не создаёт.";
      return;
    }
    const issue = faultSelect.value;
    const description = document.getElementById("fault-description").value.trim() || AeroAuth.issueLabel(issue);
    const busyBefore = new Set(busyIds);

    result.className = "assignment-result";
    result.innerHTML = "Поиск свободного инженера и спецтранспорта...";

    // Назначение идёт по uuid самолёта; координату бэкенд берёт из Redis.
    let response;
    try {
      response = await AeroAuth.apiRequest("/api/assign", {
        method: "POST",
        body: {
          issue,
          plane_uuid: plane.id,
          description,
        },
      });
    } catch (err) {
      result.className = "assignment-result";
      result.innerHTML = `Ошибка: ${assignErrorMessage(err)}`;
      return;
    }

    const gridRoute = Array.isArray(response.route) ? response.route.map(([x, y]) => [x, y]) : [];
    const distanceCells = Math.max(0, gridRoute.length - 1);
    const etaMinutes = Math.max(0, Math.round(Number(response.time || 0) / 60));
    const assignedName = await engineerName(response.engineer_uuid);
    routeExtrasLayer.clearLayers();
    startWalk(response.engineer_uuid, assignedName, gridRoute);
    const routeLine = walks.get(String(response.engineer_uuid))?.line || null;

    const pickup = findTransportOnRoute(gridRoute, fleet);
    if (pickup) {
      L.marker(agentPoint(pickup), {
        icon: L.divIcon({
          className: "transport-pickup-marker",
          html: "",
          iconSize: [16, 16],
          iconAnchor: [8, 8],
        }),
      })
        .bindTooltip(`Заезд: ${pickup.label}`, {
          permanent: true,
          direction: "top",
          className: "map-label",
          offset: [0, -8],
        })
        .addTo(routeExtrasLayer);
    }

    if (!document.getElementById("toggle-route").checked) map.removeLayer(routeExtrasLayer);

    await refreshOverlayState();

    const skippedBusy = [...busyBefore].filter((id) => id !== String(response.engineer_uuid));
    const onSite = gridRoute.length < 2;
    const sla = response.time_limit_exceeded ? "<br><em>Время в пути больше 15 минут.</em>" : "";
    const contingencyNote = skippedBusy.length
      ? `<br><em>Занятые инженеры пропущены (${skippedBusy.length}).</em>`
      : "";
    const transportNote = pickup
      ? `<br>Маршрут заезжает за спецтранспорт: <strong>${pickup.label}</strong>.`
      : ISSUE_REQUIRED_VEHICLE[issue]
        ? `<br>Для неисправности нужен: <strong>${vehicleLabel(ISSUE_REQUIRED_VEHICLE[issue])}</strong>.`
        : "";
    const routeNote = onSite
      ? "Инженер уже у воздушного судна, маршрут пустой."
      : `Маршрут: <strong>${distanceCells} клеток</strong><br>В пути: <strong>${etaMinutes} мин</strong>`;

    result.className = "assignment-result success";
    result.innerHTML = `<strong>${assignedName}</strong><br>Борт ${plane.id.slice(0, 8)} · ${AeroAuth.issueLabel(issue)}<br>${routeNote}${transportNote}${contingencyNote}${sla}`;
    if (routeLine) map.fitBounds(routeLine.getBounds(), { padding: [80, 80], maxZoom: 3 });
  });

  const legend = document.getElementById("legend");
  legend.innerHTML = [
    ["#0c7a56", "инженер свободен"],
    ["#f47920", "инженер занят"],
    ["#f47920", "спецтранспорт"],
    ["#00529b", "воздушное судно"],
    ["#c5161c", "AOG"],
    ["#c5161c", "маршрут"],
  ]
    .map(([color, label]) => `<div class="legend-row"><span class="swatch" style="background:${color}"></span>${label}</div>`)
    .join("");

  const tileInfo = document.getElementById("tile-info");
  map.on("click", (e) => {
    const { lat, lng } = e.latlng;
    const col = Math.floor(lng / tileW);
    const row = tmj.height - 1 - Math.floor(lat / tileH);

    if (row < 0 || row >= tmj.height || col < 0 || col >= tmj.width) {
      tileInfo.textContent = "Клик вне карты.";
      return;
    }

    const cells = grid[row][col];
    const roadHint = cells.some((c) => BACKEND_ROAD_IDS.has(c.gid))
      ? " (проходимо для BFS бэкенда)"
      : "";

    if (cells.length === 0) {
      tileInfo.textContent = `Point [${col}, ${row}]${roadHint}`;
    } else {
      const lines = cells.map(
        (c) =>
          `${c.layerName}: gid=${c.gid}` +
          (c.flippedH || c.flippedV || c.flippedD
            ? ` (flip h:${c.flippedH} v:${c.flippedV} d:${c.flippedD})`
            : "")
      );
      tileInfo.textContent = `Point [${col}, ${row}]${roadHint}\n` + lines.join("\n");
    }
  });
}

main().catch((err) => {
  console.error(err);
  document.getElementById("tile-info").textContent = "Ошибка: " + err.message;
});
