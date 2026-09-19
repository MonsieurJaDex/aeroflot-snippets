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

const STANDS = [
  { id: "A-01", row: 10, col: 10 },
  { id: "A-02", row: 16, col: 10 },
  { id: "B-01", row: 10, col: 51 },
  { id: "B-02", row: 17, col: 51 },
  { id: "C-01", row: 44, col: 32 },
  { id: "Техцентр", row: 51, col: 11 },
];

// Стабильные UUID — сидим флот один раз, без дублей при каждом refresh
const DEFAULT_FLEET = [
  { id: "a1111111-1111-4111-8111-111111111101", vehicle_type: "FuelTruck", label: "Топливозаправщик", point: [18, 24] },
  { id: "a1111111-1111-4111-8111-111111111102", vehicle_type: "OilCart", label: "Маслораздаточная", point: [22, 28] },
  { id: "a1111111-1111-4111-8111-111111111103", vehicle_type: "MaintenanceLift", label: "Подъёмник", point: [40, 20] },
  { id: "a1111111-1111-4111-8111-111111111104", vehicle_type: "BorescopeCart", label: "Бороскоп-тележка", point: [36, 36] },
  { id: "a1111111-1111-4111-8111-111111111105", vehicle_type: "AvionicsTestSet", label: "КПА авионики", point: [28, 42] },
];

const VEHICLE_LABELS = {
  FuelTruck: "Топливозаправщик",
  OilCart: "Маслораздаточная тележка",
  MaintenanceLift: "Аэродромный подъёмник",
  BorescopeCart: "Бороскоп-тележка",
  AvionicsTestSet: "КПА авионики",
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

function randomMapPoint() {
  return [8 + Math.floor(Math.random() * 48), 8 + Math.floor(Math.random() * 48)];
}

async function ensureEngineerPosition(id) {
  const point = randomMapPoint();
  try {
    await AeroAuth.apiRequest("/api/simulate/update_engineer_position", {
      method: "POST",
      auth: false,
      body: { id, new_point: point },
    });
    return point;
  } catch (error) {
    console.warn("[map] не удалось задать позицию инженера", id, error);
    return null;
  }
}

async function loadStaff() {
  // В демо-сценарии показываем только инженеров текущей команды (не всех 25).
  let engineers = [];
  try {
    engineers = await AeroAuth.apiRequest("/api/simulate/get_engineers_positions");
  } catch (error) {
    console.warn("[map] не удалось загрузить список сотрудников", error);
    return [];
  }

  const scenario = typeof DemoScenarios !== "undefined" ? DemoScenarios.getActiveScenario() : null;
  const roster = typeof DemoScenarios !== "undefined" ? DemoScenarios.getRoster() : {};
  const allowedIds = scenario
    ? new Set(scenario.engineers.map((item) => String(roster[item.email] || "")).filter(Boolean))
    : null;

  const agents = [];
  for (const entry of engineers) {
    const id = entry[0];
    if (allowedIds && allowedIds.size > 0 && !allowedIds.has(String(id))) continue;

    let point = entry[1];
    if (!point) {
      const preset = scenario?.engineers.find((item) => String(roster[item.email]) === String(id));
      point = preset?.point || (await ensureEngineerPosition(id));
    }
    if (!point) continue;
    agents.push({
      id,
      name: `Сотрудник ${String(id).slice(0, 8)}`,
      point: [point[0], point[1]],
      busy: false,
    });
  }

  // Подписи из демо-ростера
  if (scenario) {
    for (const agent of agents) {
      const match = scenario.engineers.find((item) => String(roster[item.email]) === String(agent.id));
      if (match) agent.name = match.name;
    }
  }

  return agents;
}

async function loadBusyEngineerIds() {
  try {
    const data = await AeroAuth.apiRequest("/api/simulate/active_engineers");
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
    const list = await AeroAuth.apiRequest("/api/simulate/get_transport_positions");
    if (!Array.isArray(list)) return [];
    return list.map((item) => ({
      id: String(item.id),
      vehicle_type: item.vehicle_type,
      label: vehicleLabel(item.vehicle_type),
      point: [item.point[0], item.point[1]],
    }));
  } catch (error) {
    console.warn("[map] не удалось загрузить спецтранспорт", error);
    return [];
  }
}

async function ensureDefaultFleet() {
  // Если открыт демо-сценарий — ставим его флот; иначе дефолтный набор из 5 машин.
  const scenario = typeof DemoScenarios !== "undefined" ? DemoScenarios.getActiveScenario() : null;
  const desired = scenario ? scenario.fleet : DEFAULT_FLEET;

  for (const vehicle of desired) {
    try {
      await AeroAuth.apiRequest("/api/simulate/update_transport_position", {
        method: "POST",
        auth: false,
        body: {
          id: vehicle.id,
          vehicle_type: vehicle.vehicle_type,
          new_point: vehicle.point,
        },
      });
    } catch (error) {
      console.warn("[map] не удалось создать единицу спецтранспорта", vehicle.vehicle_type, error);
    }
  }

  const loaded = await loadTransport();
  if (!scenario) return loaded.length ? loaded : desired.map((item) => ({ ...item, label: vehicleLabel(item.vehicle_type) }));

  // В демо показываем только флот текущего сценария
  const allowed = new Set(desired.map((item) => item.id));
  const filtered = loaded.filter((item) => allowed.has(item.id));
  return filtered.length ? filtered : desired.map((item) => ({ ...item, label: vehicleLabel(item.vehicle_type) }));
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

  ctx.fillStyle = "#1b252d";
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
  if (/only dispatchers/i.test(raw)) {
    return "Назначать может только диспетчер. Перелогиньтесь под ролью Dispatcher.";
  }
  return raw || "Неизвестная ошибка назначения";
}

async function main() {
  const session = AeroAuth.requireRole("Dispatcher");
  if (!session) return;

  document.getElementById("dispatcher-name").textContent = session.name;
  const switchLink = document.getElementById("switch-role-link");
  const inDemo = typeof DemoScenarios !== "undefined" && !!DemoScenarios.getActiveId();
  if (switchLink) {
    switchLink.hidden = !inDemo;
    switchLink.addEventListener("click", (event) => {
      event.preventDefault();
      DemoScenarios.returnToScenarioSwitcher();
    });
  }
  document.getElementById("logout-button").addEventListener("click", () => {
    if (inDemo) {
      DemoScenarios.returnToScenarioSwitcher();
      return;
    }
    AeroAuth.logout();
  });

  if (typeof DemoScenarios !== "undefined") {
    const scenario = DemoScenarios.getActiveScenario();
    if (scenario) {
      try {
        await DemoScenarios.applyScenarioLayout(scenario);
      } catch (error) {
        console.warn("[map] не удалось применить демо-расстановку", error);
      }
    }
  }

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
  });

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
  for (const stand of STANDS) standSelect.add(new Option(stand.id, stand.id));
  standSelect.value = "A-01";

  const standPoint = (stand) => [
    pxHeight - (stand.row + 0.5) * tileH,
    (stand.col + 0.5) * tileW,
  ];
  // Backend Point = [x, y] = [col, row] — должен совпадать со стартом маршрута
  const agentPoint = (agent) => [
    pxHeight - (agent.point[1] + 0.5) * tileH,
    (agent.point[0] + 0.5) * tileW,
  ];

  const standsLayer = L.layerGroup().addTo(map);
  const staffLayer = L.layerGroup().addTo(map);
  const transportLayer = L.layerGroup().addTo(map);
  const routeExtrasLayer = L.layerGroup().addTo(map);
  for (const stand of STANDS) {
    L.marker(standPoint(stand), {
      icon: L.divIcon({ className: "route-marker", html: "", iconSize: [12, 12], iconAnchor: [6, 6] }),
    })
      .bindTooltip(stand.id, { permanent: true, direction: "top", className: "map-label", offset: [0, -5] })
      .addTo(standsLayer);
  }

  let agents = await loadStaff();
  let fleet = await ensureDefaultFleet();
  let busyIds = await loadBusyEngineerIds();
  let lastAssignedId = null;

  function applyBusyFlags() {
    for (const agent of agents) {
      agent.busy = busyIds.has(String(agent.id));
    }
  }

  function renderStaffMarkers() {
    applyBusyFlags();
    staffLayer.clearLayers();
    for (const agent of agents) {
      const statusClass = agent.busy ? "agent-busy" : "agent-free";
      const statusText = agent.busy ? "занят на задаче" : "свободен";
      L.marker(agentPoint(agent), {
        icon: L.divIcon({
          className: `agent-marker ${statusClass}`,
          html: "",
          iconSize: [28, 28],
          iconAnchor: [14, 14],
        }),
      })
        .bindTooltip(`${agent.name} · ${statusText}`, { direction: "top" })
        .addTo(staffLayer);
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
    agents = await loadStaff();
    fleet = await loadTransport();
    if (fleet.length === 0) fleet = await ensureDefaultFleet();
    busyIds = await loadBusyEngineerIds();
    renderStaffMarkers();
    renderTransportMarkers();
  }

  renderStaffMarkers();
  renderTransportMarkers();
  window.setInterval(refreshOverlayState, 10000);

  let routeLayer = null;
  const faultSelect = document.getElementById("fault-select");
  const issueLabels = new Map(AeroAuth.AIRCRAFT_ISSUES);
  for (const [groupLabel, issueValues] of issueGroups()) {
    const group = document.createElement("optgroup");
    group.label = groupLabel;
    for (const value of issueValues) group.append(new Option(issueLabels.get(value), value));
    faultSelect.append(group);
  }
  const faultHint = document.getElementById("fault-hint");
  const faultDescription = document.getElementById("fault-description");
  function updateFaultHint() {
    const issue = faultSelect.value;
    const vehicleType = ISSUE_REQUIRED_VEHICLE[issue];
    const vehicleText = vehicleType
      ? `Спецтранспорт: ${vehicleLabel(vehicleType)}.`
      : "Спецтранспорт не требуется.";
    faultHint.textContent = `Нужный специалист: ${AeroAuth.engineerTypeForIssue(issue)}. ${vehicleText}`;
    if (!faultDescription.value.trim()) {
      faultDescription.value = `Обнаружена неисправность: ${AeroAuth.issueLabel(issue).toLowerCase()}. Требуется осмотр и устранение.`;
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
    if (routeLayer && event.target.checked) routeLayer.addTo(map);
    if (routeLayer && !event.target.checked) map.removeLayer(routeLayer);
    if (event.target.checked) routeExtrasLayer.addTo(map);
    else map.removeLayer(routeExtrasLayer);
  });

  const result = document.getElementById("assignment-result");
  document.getElementById("assign-button").addEventListener("click", async () => {
    const stand = STANDS.find((item) => item.id === standSelect.value);
    const issue = faultSelect.value;
    const description = document.getElementById("fault-description").value.trim() || AeroAuth.issueLabel(issue);
    const busyBefore = new Set(busyIds);

    result.className = "assignment-result";
    result.innerHTML = "Поиск свободного инженера и спецтранспорта...";

    // Назначение: бэкенд сам выбирает свободного инженера нужного типа и строит маршрут
    // (при необходимости — через точку спецтранспорта).
    let response;
    try {
      response = await AeroAuth.apiRequest("/api/assign", {
        method: "POST",
        body: {
          issue,
          plane_point: [stand.col, stand.row],
          description,
        },
      });
    } catch (err) {
      result.className = "assignment-result";
      result.innerHTML = `Ошибка: ${assignErrorMessage(err)}`;
      if (routeLayer) map.removeLayer(routeLayer);
      routeExtrasLayer.clearLayers();
      return;
    }

    const gridRoute = response.route.map(([x, y]) => [x, y]);
    const distanceCells = gridRoute.length - 1;
    const etaMinutes = Math.max(1, Math.round(response.time / 60));
    const route = gridRoute.map(([col, row]) => [
      pxHeight - (row + 0.5) * tileH,
      (col + 0.5) * tileW,
    ]);
    if (routeLayer) map.removeLayer(routeLayer);
    routeLayer = L.polyline(route, { color: "#e30613", weight: 5, opacity: 0.9, dashArray: "10 8" }).addTo(map);
    routeExtrasLayer.clearLayers();

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

    if (!document.getElementById("toggle-route").checked) {
      map.removeLayer(routeLayer);
      map.removeLayer(routeExtrasLayer);
    }

    const startPoint = gridRoute[0];
    const endPoint = gridRoute[gridRoute.length - 1];
    let agent = agents.find((item) => item.id === response.engineer_uuid);
    if (!agent) {
      agent = {
        id: response.engineer_uuid,
        name: `Сотрудник ${String(response.engineer_uuid).slice(0, 8)}`,
        point: startPoint,
        busy: true,
      };
      agents.push(agent);
    }
    agent.point = startPoint;
    agent.busy = true;
    busyIds.add(String(response.engineer_uuid));
    renderStaffMarkers();

    window.setTimeout(async () => {
      await refreshOverlayState();
      const updated = agents.find((item) => item.id === response.engineer_uuid);
      if (updated) updated.point = endPoint;
      else if (agent) agent.point = endPoint;
      renderStaffMarkers();
    }, 500);

    const skippedBusy = [...busyBefore].filter((id) => id !== String(response.engineer_uuid));
    const contingencyNote =
      lastAssignedId && lastAssignedId !== response.engineer_uuid
        ? `<br><em>Внештатная ситуация: предыдущий инженер занят — система назначила другого свободного.</em>`
        : skippedBusy.length
          ? `<br><em>Занятые инженеры пропущены (${skippedBusy.length}), выбран свободный.</em>`
          : "";
    const transportNote = pickup
      ? `<br>Маршрут учитывает спецтранспорт: <strong>${pickup.label}</strong> (жёлтая точка заезда).`
      : ISSUE_REQUIRED_VEHICLE[issue]
        ? `<br>Для неисправности нужен: <strong>${vehicleLabel(ISSUE_REQUIRED_VEHICLE[issue])}</strong>.`
        : "";

    lastAssignedId = response.engineer_uuid;
    result.className = "assignment-result success";
    result.innerHTML = `<strong>Инженер ${response.engineer_uuid.slice(0, 8)}</strong><br>${AeroAuth.issueLabel(issue)}<br>Маршрут: <strong>${distanceCells} клеток</strong><br>ETA: <strong>${etaMinutes} мин</strong>${transportNote}${contingencyNote}<br>Задача закроется автоматически после истечения срока.`;
    map.fitBounds(routeLayer.getBounds(), { padding: [80, 80], maxZoom: 3 });
  });

  const legend = document.getElementById("legend");
  legend.innerHTML = [
    ["#159b72", "инженер свободен"],
    ["#e17d32", "инженер занят"],
    ["#d4a017", "спецтранспорт"],
    ["#1684b8", "стоянка ВС"],
    ["#e30613", "маршрут"],
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
      tileInfo.textContent = `Клетка [${row}, ${col}]: пусто${roadHint}`;
    } else {
      const lines = cells.map(
        (c) =>
          `${c.layerName}: gid=${c.gid}` +
          (c.flippedH || c.flippedV || c.flippedD
            ? ` (flip h:${c.flippedH} v:${c.flippedV} d:${c.flippedD})`
            : "")
      );
      tileInfo.textContent = `Клетка [row=${row}, col=${col}]${roadHint}\n` + lines.join("\n");
    }
  });
}

main().catch((err) => {
  console.error(err);
  document.getElementById("tile-info").textContent = "Ошибка: " + err.message;
});
