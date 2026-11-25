import L from "leaflet";
import { apiRequest } from "./session.js";

const TILESETS = [
  { firstgid: 973, path: "/assets/tileset_custom.png", columns: 5, tileWidth: 16, tileHeight: 16, tilecount: 10 },
  { firstgid: 999, path: "/assets/plane.png", columns: 3, tileWidth: 16, tileHeight: 16, tilecount: 9 },
  { firstgid: 1008, path: "/assets/tileset_custom.png", columns: 5, tileWidth: 16, tileHeight: 16, tilecount: 10 },
];

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

const FLIP_H = 0x80000000;
const FLIP_V = 0x40000000;
const FLIP_D = 0x20000000;
// Запасные gid старой TMJ-карты.
const ROAD_GIDS = new Set([973, 974, 975, 0, 29]);
let activeRoadIds = ROAD_GIDS;
const CELL_PX = 4;

function paletteColor(entry) {
  if (!entry) return "#5a6570";
  const pass = entry.pass || [];
  if (!pass.length) {
    if (entry.type === "terminal") return "#00529b";
    if (entry.type === "hangar") return "#4a5568";
    if (entry.type === "warehouse") return "#8c4a32";
    if (entry.type === "parking") return "#8d6a3a";
    if (entry.type === "runway") return "#243044";
    return "#6b7280";
  }
  if (entry.type === "pedestrian") return "#2f6fed";
  if (entry.type === "runway") return "#1f2937";
  if (entry.type === "taxi") return "#64748b";
  if (entry.type === "service") {
    if (entry.spd_kmh >= 20) return "#f6e2a8";
    if (entry.spd_kmh >= 10) return "#e7c98a";
    if (entry.spd_kmh >= 8) return "#d7b56e";
    return "#c4a15a";
  }
  return "#94a3b8";
}

function isV2(data) {
  const matrix = data?.matrix || data?.map;
  return Boolean(data && Array.isArray(matrix) && Array.isArray(data.palette) && matrix.length > 64);
}

function normalizeV2(data) {
  const matrix = data.matrix || data.map;
  const height = matrix.length;
  const width = matrix[0].length;
  const paletteById = new Map(data.palette.map((entry) => [Number(entry.id), entry]));
  const walkIds = new Set((data.layers?.walk || []).map(Number));
  const roadIds = new Set((data.layers?.road || []).map(Number));
  activeRoadIds = roadIds;
  const out = {
    format: 2,
    width,
    height,
    tilewidth: CELL_PX,
    tileheight: CELL_PX,
    cell_size_m: Number(data.cell_size_m) || 5,
    origin: Number.isFinite(Number(data.origin?.lat)) && Number.isFinite(Number(data.origin?.lon))
      ? { lat: Number(data.origin.lat), lon: Number(data.origin.lon) }
      : null,
    paletteById,
    walkIds,
    roadIds,
    objects: Array.isArray(data.objects) ? data.objects : [],
    zones: Array.isArray(data.zones) ? data.zones : [],
    layers: [{ name: "полигон", type: "tilelayer", width, height, data: matrix.flat() }],
  };
  if (typeof data.id === "string") out.id = data.id;
  if (typeof data.name === "string") out.name = data.name;
  return out;
}

function rememberMapStamp(map) {
  try {
    const stamp = `${map.id || "map"}:${map.width}x${map.height}`;
    const prev = localStorage.getItem("oto-map-stamp");
    if (prev && prev !== stamp) {
      localStorage.removeItem("oto-demo-stands");
    } else {
      const raw = JSON.parse(localStorage.getItem("oto-demo-stands") || "[]");
      if (Array.isArray(raw)) {
        const kept = raw.filter((row) => {
          const x = Number(row?.point?.[0]);
          const y = Number(row?.point?.[1]);
          return x >= 0 && y >= 0 && x < map.width && y < map.height;
        });
        if (kept.length !== raw.length) localStorage.setItem("oto-demo-stands", JSON.stringify(kept));
      }
    }
    localStorage.setItem("oto-map-stamp", stamp);
  } catch (_) {}
}

function decodeGid(raw) {
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

function mapFromMatrix(matrix) {
  const height = matrix.length;
  const width = matrix[0].length;
  return {
    width,
    height,
    tilewidth: 16,
    tileheight: 16,
    layers: [{ name: "дорога", type: "tilelayer", width, height, data: matrix.flat() }],
  };
}

export function gpsToCell(tmj, lat, lon) {
  const origin = tmj?.origin;
  const cell = Number(tmj?.cell_size_m) || 5;
  if (!origin || !Number.isFinite(lat) || !Number.isFinite(lon)) return null;
  const cos = Math.max(0.2, Math.cos((origin.lat * Math.PI) / 180));
  const x = Math.floor(((lon - origin.lon) * 111320 * cos) / cell);
  const y = Math.floor(((origin.lat - lat) * 111320) / cell);
  if (x < 0 || y < 0 || x >= tmj.width || y >= tmj.height) return null;
  return [x, y];
}

export async function loadApronMap() {
  try {
    const data = await apiRequest("/api/map");
    if (data && typeof data === "object") {
      const matrix = data.matrix || data.map;
      if (Array.isArray(matrix) && Array.isArray(data.palette)) {
        const map = normalizeV2(data);
        rememberMapStamp(map);
        return map;
      }
      throw new Error("Сервер отдал карту без матрицы.");
    }
    throw new Error("Сервер отдал карту без матрицы.");
  } catch (error) {
    const status = error?.status;
    if (error?.message === "Сервер отдал карту без матрицы.") throw error;
    if (status !== undefined && status !== 0) throw error;
    console.warn("[map] /api/map недоступен", error?.message || error);
  }

  try {
    const res = await fetch("/it2026_aero.json");
    if (res.ok) {
      const data = await res.json();
      if (isV2(data)) {
        const map = normalizeV2(data);
        rememberMapStamp(map);
        return map;
      }
    }
  } catch (error) {
    console.warn("[map] файл полигона", error);
  }

  activeRoadIds = ROAD_GIDS;
  const res = await fetch("/real_map.tmj");
  if (res.ok) return res.json();
  throw new Error("Не удалось получить карту полигона.");
}

export function buildTileGrid(tmj) {
  const grid = Array.from({ length: tmj.height }, () => Array.from({ length: tmj.width }, () => []));
  for (const layer of tmj.layers || []) {
    if (layer.type !== "tilelayer") continue;
    for (let row = 0; row < layer.height; row += 1) {
      for (let col = 0; col < layer.width; col += 1) {
        const raw = layer.data[row * layer.width + col];
        if (!raw) continue;
        const cell = decodeGid(raw);
        const entry = tmj.paletteById?.get(cell.gid);
        const layerName = entry
          ? `${entry.type}${entry.spd_kmh ? ` ${entry.spd_kmh} км/ч` : ""}`
          : layer.name;
        grid[row][col].push({ layerName, ...cell });
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

export async function loadTilesetImages() {
  const loaded = [];
  for (const tileset of TILESETS) {
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

function drawTile(ctx, tilesets, cell, dx, dy, tileW, tileH, entry) {
  if (!entry) {
    const tileset = findTileset(tilesets, cell.gid);
    if (tileset?.image) {
      const localId = cell.gid - tileset.firstgid;
      if (localId >= 0 && localId < tileset.tilecount) {
        const sx = (localId % tileset.columns) * tileset.tileWidth;
        const sy = Math.floor(localId / tileset.columns) * tileset.tileHeight;
        ctx.save();
        ctx.translate(dx + tileW / 2, dy + tileH / 2);
        ctx.scale(cell.flippedH ? -1 : 1, cell.flippedV ? -1 : 1);
        if (cell.flippedD) ctx.rotate(Math.PI / 2);
        ctx.drawImage(tileset.image, sx, sy, tileset.tileWidth, tileset.tileHeight, -tileW / 2, -tileH / 2, tileW, tileH);
        ctx.restore();
        return;
      }
    }
  }

  const color = entry
    ? paletteColor(entry)
    : (FALLBACK_TILE_COLORS[cell.gid]
      || FALLBACK_TILE_COLORS[((cell.gid - 1008 + 973) % 10) + 973]
      || "#5a6570");
  ctx.fillStyle = color;
  ctx.fillRect(dx, dy, tileW, tileH);
}

export function renderCanvas(tmj, grid, tilesets) {
  const tileW = tmj.tilewidth;
  const tileH = tmj.tileheight;
  const canvas = document.createElement("canvas");
  canvas.width = tmj.width * tileW;
  canvas.height = tmj.height * tileH;
  const ctx = canvas.getContext("2d");
  ctx.fillStyle = "#c5d4b4";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  for (let row = 0; row < tmj.height; row += 1) {
    for (let col = 0; col < tmj.width; col += 1) {
      for (const cell of grid[row][col]) {
        drawTile(
          ctx,
          tilesets,
          cell,
          col * tileW,
          row * tileH,
          tileW,
          tileH,
          tmj.paletteById?.get(cell.gid),
        );
      }
    }
  }
  const metres = tmj.cell_size_m || 5;
  ctx.strokeStyle = "#c5161c";
  ctx.lineWidth = 2;
  for (const zone of tmj.zones || []) {
    const radius = (Number(zone.radius_m) / metres) * tileW;
    ctx.beginPath();
    ctx.arc((Number(zone.x) + 0.5) * tileW, (Number(zone.y) + 0.5) * tileH, radius, 0, Math.PI * 2);
    ctx.stroke();
  }
  return canvas;
}

const PICTURE_FILL = {
  runway: "#162033",
  terminal: "#00529b",
  hangar: "#143864",
  warehouse: "#5d7fa0",
  parking: "#ffffff",
};

export function apronPictureUrl(tmj) {
  const tile = tmj.tilewidth || 4;
  const w = Math.max(1, tmj.width || 1) * tile;
  const h = Math.max(1, tmj.height || 1) * tile;
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d");
  ctx.fillStyle = "#d5e2d4";
  ctx.fillRect(0, 0, w, h);
  for (const obj of tmj.objects || []) {
    const x = Number(obj.x) * tile;
    const y = Number(obj.y) * tile;
    const ow = Math.max(tile * 2, Number(obj.w || 1) * tile);
    const oh = Math.max(tile * 2, Number(obj.h || 1) * tile);
    ctx.fillStyle = PICTURE_FILL[obj.kind] || "#3e5168";
    ctx.fillRect(x, y, ow, oh);
  }
  ctx.fillStyle = "#c5161c";
  for (const zone of tmj.zones || []) {
    ctx.beginPath();
    ctx.arc((Number(zone.x) + 0.5) * tile, (Number(zone.y) + 0.5) * tile, tile * 2.4, 0, Math.PI * 2);
    ctx.fill();
  }
  return canvas.toDataURL("image/png");
}

export function cellToLatLng(tmj, col, row) {
  const tileW = tmj.tilewidth;
  const tileH = tmj.tileheight;
  const pxHeight = tmj.height * tileH;
  return [pxHeight - (row + 0.5) * tileH, (col + 0.5) * tileW];
}

export function centerToLatLng(tmj, center) {
  const tileW = tmj.tilewidth;
  const tileH = tmj.tileheight;
  const pxHeight = tmj.height * tileH;
  return [pxHeight - center[1] * tileH, center[0] * tileW];
}

export function clickToCell(tmj, lat, lng) {
  const col = Math.floor(lng / tmj.tilewidth);
  const row = tmj.height - 1 - Math.floor(lat / tmj.tileheight);
  if (row < 0 || row >= tmj.height || col < 0 || col >= tmj.width) return null;
  return { col, row };
}

export function routeSpansMap(tmj, bounds) {
  if (!tmj || !bounds) return false;
  const spanX = Math.abs(bounds.getEast() - bounds.getWest()) / tmj.tilewidth;
  const spanY = Math.abs(bounds.getNorth() - bounds.getSouth()) / tmj.tileheight;
  return spanX >= tmj.width * 0.2 || spanY >= tmj.height * 0.2;
}

export function prepareMapFrame(node, tmj) {
  if (!node || !tmj?.width || !tmj?.height) return;
  const mobile = window.matchMedia("(max-width: 860px)").matches;
  if (!mobile) {
    node.style.height = "";
    node.style.flex = "";
    node.style.maxHeight = "";
    return;
  }
  const height = Math.max(320, Math.round(window.innerHeight * 0.58));
  const next = `${height}px`;
  node.style.width = "100%";
  if (node.style.height !== next) node.style.height = next;
  node.style.flex = "0 0 auto";
  node.style.maxHeight = "62dvh";
}

export function apronFocusCell(tmj) {
  const runway = (tmj?.objects || []).find((obj) => obj.kind === "runway");
  if (runway) {
    return {
      col: Number(runway.x) + Number(runway.w || 1) / 2,
      row: Number(runway.y) + Number(runway.h || 1) / 2,
    };
  }
  return { col: (tmj?.width || 2) / 2, row: (tmj?.height || 2) / 2 };
}

export function applyPhoneApron(map, tmj) {
  if (!window.matchMedia("(max-width: 860px)").matches) return false;
  const tile = tmj.tilewidth || 4;
  const zoom = Math.log2(6 / tile);
  const focus = apronFocusCell(tmj);
  const z = Math.max(map.getMinZoom(), Math.min(map.getMaxZoom(), zoom));
  map._sizeChanged = true;
  if (map._loaded) map.invalidateSize(false);
  map.setView(cellToLatLng(tmj, focus.col, focus.row), z, { animate: false });
  return true;
}

export function roadHint(cells) {
  return cells.some((cell) => activeRoadIds.has(cell.gid)) ? " (дорога)" : "";
}

function placeTitle(name) {
  const text = String(name || "").replace(/^стоянка\s+/i, "").trim();
  if (!text) return "";
  return text.length > 16 ? `${text.slice(0, 14)}…` : text;
}

export function addPlaceLabels(map, tmj) {
  const layer = L.layerGroup().addTo(map);
  const seen = new Set();
  const places = [];
  for (const obj of tmj.objects || []) {
    const name = placeTitle(obj.name);
    if (!name) continue;
    seen.add(name.toLowerCase());
    places.push({
      name,
      col: Number(obj.x) + Number(obj.w || 1) / 2,
      row: Number(obj.y) + Number(obj.h || 1) / 2,
    });
  }
  for (const zone of tmj.zones || []) {
    const name = placeTitle(zone.label || zone.id);
    if (!name || seen.has(name.toLowerCase())) continue;
    places.push({
      name,
      col: Number(zone.x) + 0.5,
      row: Number(zone.y) + 0.5,
    });
  }
  for (const place of places) {
    L.marker(cellToLatLng(tmj, place.col, place.row), {
      interactive: false,
      keyboard: false,
      icon: L.divIcon({ className: "map-pin-hidden", html: "", iconSize: [1, 1] }),
    }).bindTooltip(place.name, {
      permanent: true,
      direction: "top",
      className: "map-label",
      offset: [0, -2],
    }).addTo(layer);
  }
  return layer;
}

/** Маска хода: палитра полигона или старые gid TMJ. */
export function buildWalkMask(tmj) {
  const height = tmj?.height || 0;
  const width = tmj?.width || 0;
  const mask = Array.from({ length: height }, () => Array(width).fill(false));
  const layer = (tmj?.layers || []).find((item) => item.type === "tilelayer" && Array.isArray(item.data));
  if (!layer) return mask;
  const walk = tmj.walkIds;
  for (let row = 0; row < height; row += 1) {
    for (let col = 0; col < width; col += 1) {
      const raw = layer.data[row * width + col] || 0;
      const { gid } = decodeGid(raw);
      mask[row][col] = walk ? walk.has(gid) : ROAD_GIDS.has(gid);
    }
  }
  return mask;
}

export function isWalkable(mask, x, y) {
  const col = Math.floor(Number(x));
  const row = Math.floor(Number(y));
  if (!mask?.length || row < 0 || col < 0 || row >= mask.length || col >= mask[0].length) return false;
  return Boolean(mask[row][col]);
}
