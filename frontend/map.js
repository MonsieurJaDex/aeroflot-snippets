const CONFIG = {
  mapCandidates: ["../et.tmj", "./et.tmj", "/et.tmj"],
  apiCandidates: ["http://127.0.0.1:3001/"],
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

const AGENTS = [
  { id: "ENG-014", name: "Алексей Смирнов", skill: "airframe", skillName: "Планер и двигатель", status: "free", row: 25, col: 18 },
  { id: "ENG-022", name: "Мария Волкова", skill: "avionics", skillName: "Авионика", status: "free", row: 37, col: 43 },
  { id: "ENG-031", name: "Илья Ким", skill: "airframe", skillName: "Планер и двигатель", status: "busy", row: 31, col: 29 },
  { id: "ENG-044", name: "Ольга Белова", skill: "avionics", skillName: "Авионика", status: "free", row: 48, col: 48 },
];

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
  for (const path of CONFIG.apiCandidates) {
    try {
      const res = await fetch(path);
      if (!res.ok) continue;
      const payload = await res.json();
      const matrix = Array.isArray(payload) ? payload : payload.matrix;
      if (!Array.isArray(matrix) || !Array.isArray(matrix[0])) continue;
      return mapFromMatrix(matrix);
    } catch (e) {
    }
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
    "Не удалось найти et.tmj. Запусти статический сервер из корня репозитория " +
      "(например: python -m http.server) и открой frontend/index.html через него."
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
  const tmj = await loadMap();
  const grid = buildTileGrid(tmj);

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

  const layerNames = tmj.layers
    .filter((l) => l.type === "tilelayer")
    .map((l) => l.name);
  let overlay = null;
  function redraw() {
    const canvas = renderCanvas(tmj, grid);
    const dataUrl = canvas.toDataURL("image/png");
    if (overlay) map.removeLayer(overlay);
    overlay = L.imageOverlay(dataUrl, bounds).addTo(map);
  }

  redraw();
  map.fitBounds(bounds);

  const layersList = document.getElementById("layers-list");
  layersList.innerHTML = "";
  for (const name of layerNames) {
    const row = document.createElement("label");
    row.className = "layer-row";
    row.innerHTML = `<input type="checkbox" checked /> <span>${name}</span>`;
    row.querySelector("input").addEventListener("change", redraw);
    layersList.appendChild(row);
  }

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

  const operationsLayer = L.layerGroup().addTo(map);
  for (const stand of STANDS) {
    L.marker(standPoint(stand), {
      icon: L.divIcon({ className: "route-marker", html: "", iconSize: [12, 12], iconAnchor: [6, 6] }),
    }).bindTooltip(stand.id, { permanent: true, direction: "top", className: "map-label", offset: [0, -5] }).addTo(operationsLayer);
  }
  for (const agent of AGENTS) {
    L.marker(agentPoint(agent), {
      icon: L.divIcon({ className: `agent-marker agent-${agent.status}`, html: "", iconSize: [28, 28], iconAnchor: [14, 14] }),
    }).bindTooltip(`${agent.name} · ${agent.status === "free" ? "свободен" : "занят"}`, { direction: "top" }).addTo(operationsLayer);
  }

  let routeLayer = null;
  const result = document.getElementById("assignment-result");
  document.getElementById("assign-button").addEventListener("click", () => {
    const stand = STANDS.find((item) => item.id === standSelect.value);
    const skill = document.getElementById("fault-select").value;
    const candidates = AGENTS.filter((agent) => agent.skill === skill && agent.status === "free");
    const ranked = candidates.map((agent) => ({
      agent,
      distance: Math.abs(agent.row - stand.row) + Math.abs(agent.col - stand.col),
    })).sort((a, b) => a.distance - b.distance);

    if (!ranked.length) {
      result.className = "assignment-result";
      result.innerHTML = "Нет свободного инженера нужной квалификации.";
      if (routeLayer) map.removeLayer(routeLayer);
      return;
    }

    const winner = ranked[0];
    const eta = Math.max(1, Math.ceil(winner.distance / 4));
    const route = [agentPoint(winner.agent), standPoint(stand)];
    if (routeLayer) map.removeLayer(routeLayer);
    routeLayer = L.polyline(route, { color: "#e30613", weight: 5, opacity: 0.9, dashArray: "10 8" }).addTo(map);
    result.className = "assignment-result success";
    result.innerHTML = `<strong>${winner.agent.name}</strong><br>${winner.agent.skillName}<br>ETA: <strong>${eta} мин</strong> · лимит 15 мин<br>Маршрут построен`;
    map.fitBounds(routeLayer.getBounds(), { padding: [80, 80], maxZoom: 3 });
  });

  const legend = document.getElementById("legend");
  legend.innerHTML = layerNames
    .map((name) => {
      const color = LAYER_COLORS[name] || DEFAULT_COLOR;
      return `<div class="legend-row"><span class="swatch" style="background:${color}"></span>${name}</div>`;
    })
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
  document.getElementById("layers-list").textContent = "—";
});
