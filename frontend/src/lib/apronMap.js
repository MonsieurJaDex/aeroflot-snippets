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
const BACKEND_ROAD_IDS = new Set([0, 29]);

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

export async function loadApronMap() {
  try {
    const res = await fetch("/real_map.tmj");
    if (res.ok) return res.json();
  } catch {
    // ниже запасной вариант с /api/map
  }

  try {
    const matrix = await apiRequest("/api/map");
    if (Array.isArray(matrix) && Array.isArray(matrix[0])) return mapFromMatrix(matrix);
  } catch (error) {
    console.warn("[map] /api/map недоступен", error);
  }

  throw new Error("Не удалось получить карту. Проверь frontend/public/real_map.tmj и что backend запущен.");
}

export function buildTileGrid(tmj) {
  const grid = Array.from({ length: tmj.height }, () => Array.from({ length: tmj.width }, () => []));
  for (const layer of tmj.layers || []) {
    if (layer.type !== "tilelayer") continue;
    for (let row = 0; row < layer.height; row += 1) {
      for (let col = 0; col < layer.width; col += 1) {
        const raw = layer.data[row * layer.width + col];
        if (!raw) continue;
        grid[row][col].push({ layerName: layer.name, ...decodeGid(raw) });
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

function drawTile(ctx, tilesets, cell, dx, dy, tileW, tileH) {
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

  const color = FALLBACK_TILE_COLORS[cell.gid]
    || FALLBACK_TILE_COLORS[((cell.gid - 1008 + 973) % 10) + 973]
    || "#5a6570";
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
  ctx.fillStyle = "#d7e3ee";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  for (let row = 0; row < tmj.height; row += 1) {
    for (let col = 0; col < tmj.width; col += 1) {
      for (const cell of grid[row][col]) {
        drawTile(ctx, tilesets, cell, col * tileW, row * tileH, tileW, tileH);
      }
    }
  }
  return canvas;
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

export function roadHint(cells) {
  return cells.some((cell) => BACKEND_ROAD_IDS.has(cell.gid)) ? " (проходимо для BFS бэкенда)" : "";
}
