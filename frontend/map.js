const CONFIG = {
  mapCandidates: ["../et.tmj", "./et.tmj", "/et.tmj"],
  tilesetImage: {
    enabled: false,
    path: "assets/tilemap_packed.png",
    columns: 24,
    tileWidth: 16,
    tileHeight: 16,
    margin: 0,
    spacing: 0,
    firstgid: 1,
  },
};

const LAYER_COLORS = {
  "основа": "#dfe7e2",
  "дорога": "#aeb8c2",
  "здание": "#cbd3da",
  "самолеты": "#d8e9f8",
  "машины": "#f1dfb4",
  "сотрудники": "#d8f0e7",
};
const DEFAULT_COLOR = "#d5dce1";

const STANDS = [
  { id: "A-01", row: 10, col: 10 },
  { id: "A-02", row: 16, col: 10 },
  { id: "B-01", row: 10, col: 51 },
  { id: "B-02", row: 17, col: 51 },
  { id: "C-01", row: 44, col: 32 },
  { id: "Техцентр", row: 51, col: 11 },
];

// Инженеры на смене показаны декоративно: бэкенд пока не отдаёт их живые позиции по HTTP.
const AGENTS = [
  { id: "ENG-014", name: "Смена А", row: 25, col: 18 },
  { id: "ENG-022", name: "Смена Б", row: 37, col: 43 },
  { id: "ENG-031", name: "Смена В", row: 31, col: 29 },
  { id: "ENG-044", name: "Смена Г", row: 48, col: 48 },
];

function randomizeAgentPositions(width, height) {
  const occupied = new Set();
  const standCells = new Set(STANDS.map(({ row, col }) => `${row}:${col}`));

  for (const agent of AGENTS) {
    let row;
    let col;
    do {
      row = Math.floor(Math.random() * height);
      col = Math.floor(Math.random() * width);
    } while (occupied.has(`${row}:${col}`) || standCells.has(`${row}:${col}`));

    agent.row = row;
    agent.col = col;
    occupied.add(`${row}:${col}`);
  }
}

const BACKEND_ROAD_IDS = new Set([0, 29]);
const FLIP_H = 0x80000000;
const FLIP_V = 0x40000000;
const FLIP_D = 0x20000000;

function decodeGid(raw) {
  let gid = raw;
  let flippedH = false,
    flippedV = false,
    flippedD = false;

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

function shade(hexColor, gid) {
  const n = ((gid * 2654435761) >>> 0) % 40; // 0..39
  const amt = n - 20; // -20..19
  const c = hexColor.replace("#", "");
  const r = clamp(parseInt(c.substr(0, 2), 16) + amt);
  const g = clamp(parseInt(c.substr(2, 2), 16) + amt);
  const b = clamp(parseInt(c.substr(4, 2), 16) + amt);
  return `rgb(${r},${g},${b})`;
}
function clamp(v) {
  return Math.max(0, Math.min(255, v));
}

async function loadMap() {
  try {
    const matrix = await AeroAuth.apiRequest("/api/map", { method: "GET" });
    if (Array.isArray(matrix) && Array.isArray(matrix[0])) {
      console.info("[map] загружено из /api/map");
      return mapFromMatrix(matrix);
    }
  } catch (e) {
    console.warn("[map] /api/map недоступен, пробую локальный et.tmj", e);
  }

  for (const path of CONFIG.mapCandidates) {
    try {
      const res = await fetch(path);
      if (!res.ok) continue;
      const json = await res.json();
      console.info(`[map] загружено из ${path}`);
      return json;
    } catch (e) {
    }
  }
  throw new Error(
    "Не удалось получить карту ни от бэкенда, ни из et.tmj. Проверь, что backend запущен и адрес API указан верно."
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

function renderCanvas(tmj, grid) {
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
      if (cells.some((cell) => cell.layerName === "дорога")) {
        ctx.fillStyle = "#42515d";
        ctx.fillRect(col * tileW, row * tileH, tileW, tileH);
      }
      if (cells.some((cell) => cell.layerName === "здание")) {
        ctx.fillStyle = "#28343d";
        ctx.fillRect(col * tileW + 1, row * tileH + 1, tileW - 2, tileH - 2);
      }
    }
  }

  ctx.strokeStyle = "rgba(195, 215, 225, 0.08)";
  ctx.lineWidth = 1;
  for (let col = 0; col <= tmj.width; col++) ctx.moveTo(col * tileW, 0), ctx.lineTo(col * tileW, canvas.height);
  for (let row = 0; row <= tmj.height; row++) ctx.moveTo(0, row * tileH), ctx.lineTo(canvas.width, row * tileH);
  ctx.stroke();

  return canvas;
}

async function main() {
  const session = AeroAuth.requireRole("Dispatcher");
  if (!session) return;

  document.getElementById("dispatcher-name").textContent = session.name;
  document.getElementById("logout-button").addEventListener("click", () => AeroAuth.logout());

  const tmj = await loadMap();
  const grid = buildTileGrid(tmj);

  const tileW = tmj.tilewidth;
  const tileH = tmj.tileheight;
  const pxWidth = tmj.width * tileW;
  const pxHeight = tmj.height * tileH;
    randomizeAgentPositions(tmj.width, tmj.height);

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
    const canvas = renderCanvas(tmj, grid);
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
  const agentPoint = (agent) => [
    pxHeight - (agent.row + 0.5) * tileH,
    (agent.col + 0.5) * tileW,
  ];

  const standsLayer = L.layerGroup().addTo(map);
  const staffLayer = L.layerGroup().addTo(map);
  for (const stand of STANDS) {
    L.marker(standPoint(stand), {
      icon: L.divIcon({ className: "route-marker", html: "", iconSize: [12, 12], iconAnchor: [6, 6] }),
    }).bindTooltip(stand.id, { permanent: true, direction: "top", className: "map-label", offset: [0, -5] }).addTo(standsLayer);
  }
  function renderStaffMarkers() {
    staffLayer.clearLayers();
    for (const agent of AGENTS) {
      L.marker(agentPoint(agent), {
        icon: L.divIcon({ className: "agent-marker agent-free", html: "", iconSize: [28, 28], iconAnchor: [14, 14] }),
      }).bindTooltip(`${agent.name} · на смене`, { direction: "top" }).addTo(staffLayer);
    }
  }

  renderStaffMarkers();

  let routeLayer = null;
  const faultSelect = document.getElementById("fault-select");
  for (const [value, label] of AeroAuth.AIRCRAFT_ISSUES) faultSelect.add(new Option(label, value));
  const faultHint = document.getElementById("fault-hint");
  const faultDescription = document.getElementById("fault-description");
  function updateFaultHint() {
    const issue = faultSelect.value;
    faultHint.textContent = `Нужный специалист: ${AeroAuth.engineerTypeForIssue(issue)}.`;
    if (!faultDescription.value.trim()) {
      faultDescription.value = `Обнаружена неисправность: ${AeroAuth.issueLabel(issue).toLowerCase()}. Требуется осмотр и устранение.`;
    }
  }
  faultSelect.addEventListener("change", updateFaultHint);
  updateFaultHint();

  const gridControl = document.getElementById("toggle-grid");
  document.getElementById("toggle-staff").addEventListener("change", (event) => event.target.checked ? staffLayer.addTo(map) : map.removeLayer(staffLayer));
  document.getElementById("toggle-stands").addEventListener("change", (event) => event.target.checked ? standsLayer.addTo(map) : map.removeLayer(standsLayer));
  document.getElementById("toggle-route").addEventListener("change", (event) => {
    if (routeLayer && event.target.checked) routeLayer.addTo(map);
    if (routeLayer && !event.target.checked) map.removeLayer(routeLayer);
  });
  gridControl.addEventListener("change", () => {
    document.getElementById("map").classList.toggle("show-grid", gridControl.checked);
  });
  const result = document.getElementById("assignment-result");
  document.getElementById("assign-button").addEventListener("click", async () => {
    const stand = STANDS.find((item) => item.id === standSelect.value);
    const issue = faultSelect.value;
    const description = document.getElementById("fault-description").value.trim() || AeroAuth.issueLabel(issue);

    result.className = "assignment-result";
    result.innerHTML = "Поиск свободного инженера...";

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
      result.innerHTML = `Ошибка: ${err.message}`;
      if (routeLayer) map.removeLayer(routeLayer);
      return;
    }

    const gridRoute = response.route.map(([x, y]) => [x, y]);
    const distanceCells = gridRoute.length - 1;
    const etaMinutes = Math.max(1, Math.round(response.time / 60));
    const route = gridRoute.map(([col, row]) => [pxHeight - (row + 0.5) * tileH, (col + 0.5) * tileW]);
    if (routeLayer) map.removeLayer(routeLayer);
    routeLayer = L.polyline(route, { color: "#e30613", weight: 5, opacity: 0.9, dashArray: "10 8" }).addTo(map);
    if (!document.getElementById("toggle-route").checked) map.removeLayer(routeLayer);

    result.className = "assignment-result success";
    result.innerHTML = `<strong>Инженер ${response.engineer_uuid.slice(0, 8)}</strong><br>${AeroAuth.issueLabel(issue)}<br>Маршрут: <strong>${distanceCells} клеток</strong><br>ETA: <strong>${etaMinutes} мин</strong><br>Задача закроется автоматически после истечения срока.`;
    map.fitBounds(routeLayer.getBounds(), { padding: [80, 80], maxZoom: 3 });
  });

  const legend = document.getElementById("legend");
  legend.innerHTML = [
    ["#159b72", "инженер на смене"],
    ["#1684b8", "стоянка ВС"],
    ["#e30613", "маршрут"],
  ].map(([color, label]) => `<div class="legend-row"><span class="swatch" style="background:${color}"></span>${label}</div>`).join("");

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
