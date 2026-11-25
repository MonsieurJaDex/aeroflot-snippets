// Прогоны одного id склеиваются в один path, карта кэшируется.

const SVG_NS = "http://www.w3.org/2000/svg";

const CELL_CLASS = {
  1: "apron-runway",
  2: "apron-taxi",
  3: "apron-svc-5",
  4: "apron-svc-8",
  5: "apron-svc-10",
  6: "apron-svc-20",
  7: "apron-walk",
  8: "apron-hangar",
  9: "apron-parking",
  10: "apron-shoulder",
  11: "apron-terminal",
  12: "apron-warehouse",
};

const DRAW_ORDER = [10, 3, 4, 5, 6, 2, 7, 1, 9, 12, 8, 11];
const SHADOW_IDS = new Set([8, 9, 11, 12]);
const TRACK_IDS = new Set([1, 2, 8, 9, 10, 11, 12]);

// Заливки в самом SVG: Leaflet берёт файл как картинку.
const APRON_STYLE = `
.apron-svg{font-family:Roboto,sans-serif}
.apron-grass{fill:#d5e2d4}
.apron-runway{fill:#0c2748;stroke:#0c2748;stroke-width:5;stroke-linejoin:round}
.apron-shoulder{fill:#1a3f68}
.apron-taxi{fill:#4f6e90;stroke:#4f6e90;stroke-width:3.5;stroke-linejoin:round}
.apron-walk{fill:#3d7ec4}
.apron-svc-5{fill:#f4f8fc}
.apron-svc-8{fill:#d4e4f4}
.apron-svc-10{fill:#8eb4d8}
.apron-svc-20{fill:#2f6eae}
.apron-terminal{fill:#00529b}
.apron-hangar{fill:#143864}
.apron-warehouse{fill:#5d7fa0}
.apron-parking{fill:#ffffff}
.apron-other{fill:#7f96ad}
.apron-shadow{fill:#0a1628;opacity:.28}
.apron-taxi-line{fill:none;stroke:#ffffff}
.apron-edge{fill:none;stroke:#f4f7fb}
.apron-threshold{fill:#f4f7fb}
.apron-centerline{fill:none;stroke:#f4f7fb}
.apron-roof{fill:#e8eef3}
.apron-ribbon{fill:#00366d}
.apron-ribbon-2{fill:#5aa6d6}
.apron-window{fill:#ffffff}
.apron-roof-metal{fill:#d5dee8}
.apron-rib{fill:none;stroke:#1a3f68}
.apron-door-frame{fill:#0e243c}
.apron-door{fill:#071422}
.apron-roof-wood{fill:#4a6d90}
.apron-stall{fill:none;stroke:#00529b}
.apron-place{fill:#f8fafc;stroke:#0a2744;paint-order:stroke}
.apron-zone{fill:none;stroke:#c5161c}
.apron-zone-label{fill:#9b1218;stroke:#ffffff;paint-order:stroke}
`;

let cachedSvg = { key: "", markup: "" };
let labelFrame = null;

function el(name, attrs, parent) {
  const node = document.createElementNS(SVG_NS, name);
  if (attrs) {
    for (const [key, value] of Object.entries(attrs)) {
      if (value != null && value !== "") node.setAttribute(key, String(value));
    }
  }
  parent?.appendChild(node);
  return node;
}

function text(parent, attrs, value) {
  const node = el("text", attrs, parent);
  node.textContent = value;
  return node;
}

function tileLayerData(tmj) {
  const layer = (tmj?.layers || []).find((item) => item.type === "tilelayer" && Array.isArray(item.data));
  return layer?.data || null;
}

function hashGrid(data) {
  let hash = 2166136261;
  for (let i = 0; i < data.length; i += 1) hash = Math.imul(hash ^ data[i], 16777619);
  return hash >>> 0;
}

function runsOf(data, width, height, tileW, tileH) {
  const chunks = new Map();
  const cells = {
    1: [], 2: [], 8: [], 9: [], 10: [], 11: [], 12: [],
  };
  for (let y = 0; y < height; y += 1) {
    const row = y * width;
    let x = 0;
    while (x < width) {
      const id = data[row + x] || 0;
      const x0 = x;
      x += 1;
      while (x < width && (data[row + x] || 0) === id) x += 1;
      if (!id) continue;
      if (TRACK_IDS.has(id)) {
        const list = cells[id];
        for (let col = x0; col < x; col += 1) list.push([col, y]);
      }
      const px = x0 * tileW;
      const py = y * tileH;
      const pw = (x - x0) * tileW;
      let bucket = chunks.get(id);
      if (!bucket) {
        bucket = [];
        chunks.set(id, bucket);
      }
      bucket.push(`M${px} ${py}h${pw + 0.5}v${tileH + 0.5}h${-(pw + 0.5)}z`);
    }
  }
  const paths = new Map();
  for (const [id, bucket] of chunks) paths.set(id, bucket.join(""));
  return { paths, cells };
}

function axisOf(points) {
  if (!points || points.length < 2) return null;
  let sx = 0;
  let sy = 0;
  for (const [x, y] of points) {
    sx += x;
    sy += y;
  }
  const mx = sx / points.length;
  const my = sy / points.length;
  let xx = 0;
  let xy = 0;
  let yy = 0;
  for (const [x, y] of points) {
    const dx = x - mx;
    const dy = y - my;
    xx += dx * dx;
    xy += dx * dy;
    yy += dy * dy;
  }
  const ang = 0.5 * Math.atan2(2 * xy, xx - yy);
  let ux = Math.cos(ang);
  let uy = Math.sin(ang);
  let nx = -uy;
  let ny = ux;
  let along = 0;
  let across = 0;
  for (const [x, y] of points) {
    const dx = x - mx;
    const dy = y - my;
    const t = dx * ux + dy * uy;
    const s = dx * nx + dy * ny;
    along += t * t;
    across += s * s;
  }
  if (across > along) {
    const tx = ux;
    const ty = uy;
    ux = nx;
    uy = ny;
    nx = -uy;
    ny = ux;
    if (Math.abs(tx - nx) + Math.abs(ty - ny) < 1e-6) {
      nx = tx;
      ny = ty;
    }
  }
  let tmin = Infinity;
  let tmax = -Infinity;
  let pmin = Infinity;
  let pmax = -Infinity;
  for (const [x, y] of points) {
    const dx = x - mx;
    const dy = y - my;
    const t = dx * ux + dy * uy;
    const s = dx * nx + dy * ny;
    if (t < tmin) tmin = t;
    if (t > tmax) tmax = t;
    if (s < pmin) pmin = s;
    if (s > pmax) pmax = s;
  }
  let deg = Math.atan2(uy, ux) * 180 / Math.PI;
  if (deg > 90) deg -= 180;
  if (deg < -90) deg += 180;
  return { mx, my, ux, uy, nx, ny, tmin, tmax, pmin, pmax, deg };
}

function svgAlong(axis, t, s, tileW, tileH) {
  const x = axis.mx + axis.ux * t + axis.nx * s;
  const y = axis.my + axis.uy * t + axis.ny * s;
  return [(x + 0.5) * tileW, (y + 0.5) * tileH];
}

function profileOf(points, axis, bins) {
  const span = (axis.tmax - axis.tmin) || 1;
  const minS = Array(bins).fill(Infinity);
  const maxS = Array(bins).fill(-Infinity);
  const count = Array(bins).fill(0);
  for (const [x, y] of points) {
    const dx = x - axis.mx;
    const dy = y - axis.my;
    const t = dx * axis.ux + dy * axis.uy;
    const s = dx * axis.nx + dy * axis.ny;
    let bin = Math.floor(((t - axis.tmin) / span) * bins);
    if (bin < 0) bin = 0;
    if (bin >= bins) bin = bins - 1;
    count[bin] += 1;
    if (s < minS[bin]) minS[bin] = s;
    if (s > maxS[bin]) maxS[bin] = s;
  }
  return { minS, maxS, count, bins, span };
}

function edgePath(axis, profile, side, tileW, tileH) {
  let d = "";
  let drawing = false;
  for (let i = 0; i < profile.bins; i += 1) {
    if (!profile.count[i]) {
      drawing = false;
      continue;
    }
    const t = axis.tmin + ((i + 0.5) / profile.bins) * profile.span;
    const s = side === "min" ? profile.minS[i] + 1 : profile.maxS[i] - 1;
    const [x, y] = svgAlong(axis, t, s, tileW, tileH);
    d += `${drawing ? "L" : "M"}${x.toFixed(1)} ${y.toFixed(1)}`;
    drawing = true;
  }
  return d;
}

function barPath(axis, t0, t1, s0, s1, tileW, tileH) {
  const pts = [
    svgAlong(axis, t0, s0, tileW, tileH),
    svgAlong(axis, t1, s0, tileW, tileH),
    svgAlong(axis, t1, s1, tileW, tileH),
    svgAlong(axis, t0, s1, tileW, tileH),
  ];
  return `${pts.map((p, i) => `${i ? "L" : "M"}${p[0].toFixed(1)} ${p[1].toFixed(1)}`).join("")}z`;
}

function thresholdPath(axis, profile, bin, dir, tileW, tileH) {
  if (bin < 0 || bin >= profile.bins || profile.count[bin] < 2) return "";
  const s0 = profile.minS[bin];
  const s1 = profile.maxS[bin];
  const span = s1 - s0;
  if (span < 6) return "";
  const stripe = 2.1;
  const gap = 1.35;
  const count = Math.max(4, Math.min(8, Math.floor((span * 0.78) / (stripe + gap))));
  const total = count * stripe + (count - 1) * gap;
  let s = (s0 + s1) / 2 - total / 2;
  const t0 = axis.tmin + ((bin + 0.5) / profile.bins) * profile.span;
  const lo = Math.min(t0, t0 + dir * 2.6);
  const hi = Math.max(t0, t0 + dir * 2.6);
  let d = "";
  for (let i = 0; i < count; i += 1) {
    d += barPath(axis, lo, hi, s, s + stripe, tileW, tileH);
    s += stripe + gap;
  }
  return d;
}

function endBins(profile) {
  let first = -1;
  let last = -1;
  for (let i = 0; i < profile.bins; i += 1) {
    if (!profile.count[i]) continue;
    if (first < 0) first = i;
    last = i;
  }
  return [first, last];
}

function clip(defs, id, d) {
  const node = el("clipPath", { id, clipPathUnits: "userSpaceOnUse" }, defs);
  el("path", { d }, node);
  return `url(#${id})`;
}

function cellsInside(cells, obj) {
  const x1 = Number(obj.x) + Number(obj.w || 1);
  const y1 = Number(obj.y) + Number(obj.h || 1);
  return cells.filter(([x, y]) => x >= obj.x && y >= obj.y && x < x1 && y < y1);
}

function centroid(cells, fallback) {
  if (!cells.length) return fallback;
  let sx = 0;
  let sy = 0;
  for (const [x, y] of cells) {
    sx += x;
    sy += y;
  }
  return [sx / cells.length, sy / cells.length];
}

function boundsOf(cells) {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const [x, y] of cells) {
    if (x < minX) minX = x;
    if (y < minY) minY = y;
    if (x > maxX) maxX = x;
    if (y > maxY) maxY = y;
  }
  return { minX, minY, maxX, maxY };
}

function measureLabel(value, size) {
  return String(value || "").length * size * 0.58;
}

function fitFont(value, size, pxW) {
  const room = Math.max(12, pxW - 8);
  const width = measureLabel(value, size);
  if (!width || width <= room) return size;
  return Math.max(9, (size * room) / width);
}

function clampLabel(x, y, value, size, anchor, box) {
  if (!box) return [x, y, size];
  const font = fitFont(value, size, box.w);
  const width = measureLabel(value, font);
  const halfH = font * 0.65;
  const pad = 4;
  let left = x - width / 2;
  if (anchor === "start") left = x;
  else if (anchor === "end") left = x - width;
  const overflowR = left + width - (box.w - pad);
  if (overflowR > 0) {
    x -= overflowR;
    left -= overflowR;
  }
  if (left < pad) x += pad - left;
  if (y + halfH > box.h - pad) y = box.h - pad - halfH;
  if (y - halfH < pad) y = pad + halfH;
  return [x, y, font];
}

function placeLabel(parent, x, y, value, size, extra) {
  const anchor = extra?.["text-anchor"] || "middle";
  const [px, py, font] = clampLabel(x, y, value, size, anchor, labelFrame);
  let attrs = extra;
  if (attrs?.transform && /rotate\(/.test(attrs.transform)) {
    attrs = {
      ...attrs,
      transform: attrs.transform.replace(
        /rotate\(([^ )]+) [^)]+\)/,
        `rotate($1 ${px.toFixed(1)} ${py.toFixed(1)})`,
      ),
    };
  }
  text(parent, {
    class: "apron-place",
    "text-anchor": "middle",
    "dominant-baseline": "middle",
    "font-size": Number(font).toFixed(1),
    "stroke-width": Math.max(2.4, font * 0.18).toFixed(1),
    ...attrs,
    x: Number(px).toFixed(1),
    y: Number(py).toFixed(1),
  }, value);
}

function drawGrass(svg, pxW, pxH) {
  el("rect", { class: "apron-grass", x: 0, y: 0, width: pxW, height: pxH }, svg);
}

function drawRunway(svg, defs, paths, cells, objects, tileW, tileH) {
  const shoulder = cells[10];
  const pavement = cells[1];
  const axis = axisOf(shoulder.length > 8 ? shoulder : pavement);
  const group = el("g", { class: "apron-runway-marks" }, svg);
  if (axis && shoulder.length) {
    const pad = (paths.get(10) || "") + (paths.get(1) || "");
    const local = el("g", { "clip-path": clip(defs, "apron-clip-shoulder", pad) }, group);
    const profile = profileOf(shoulder, axis, 42);
    const left = edgePath(axis, profile, "min", tileW, tileH);
    const right = edgePath(axis, profile, "max", tileW, tileH);
    if (left) {
      el("path", {
        class: "apron-edge",
        d: left,
        "stroke-width": 3,
        "stroke-linecap": "round",
        "stroke-linejoin": "round",
      }, local);
    }
    if (right) {
      el("path", {
        class: "apron-edge",
        d: right,
        "stroke-width": 3,
        "stroke-linecap": "round",
        "stroke-linejoin": "round",
      }, local);
    }
    const [first, last] = endBins(profile);
    const bars = thresholdPath(axis, profile, Math.min(profile.bins - 1, first + 1), 1, tileW, tileH)
      + thresholdPath(axis, profile, Math.max(0, last - 1), -1, tileW, tileH);
    if (bars) el("path", { class: "apron-threshold", d: bars }, local);
  }
  const core = axisOf(pavement);
  if (core) {
    const a = svgAlong(core, core.tmin + 8, 0, tileW, tileH);
    const b = svgAlong(core, core.tmax - 8, 0, tileW, tileH);
    el("path", {
      class: "apron-centerline",
      d: `M${a[0].toFixed(1)} ${a[1].toFixed(1)}L${b[0].toFixed(1)} ${b[1].toFixed(1)}`,
      "stroke-width": 2.6,
      "stroke-linecap": "round",
      "stroke-dasharray": "12 11",
    }, group);
  }
  const named = (objects || []).find((obj) => obj.kind === "runway" && obj.name);
  if (!named) return;
  const host = axis || core;
  if (!host) {
    placeLabel(
      group,
      (Number(named.x) + Number(named.w || 1) / 2) * tileW,
      (Number(named.y) + Number(named.h || 1) / 2) * tileH,
      named.name,
      26,
    );
    return;
  }
  const [px, py] = svgAlong(host, (host.tmin + host.tmax) / 2, 0, tileW, tileH);
  placeLabel(group, px, py, named.name, 26, {
    "letter-spacing": 1.4,
    transform: `rotate(${host.deg.toFixed(1)} ${px.toFixed(1)} ${py.toFixed(1)})`,
  });
}

function hangarDoor(cells) {
  const box = boundsOf(cells);
  if (!Number.isFinite(box.minX)) return null;
  const left = new Map();
  for (const [x, y] of cells) {
    const prev = left.get(y);
    if (prev == null || x < prev) left.set(y, x);
  }
  const span = box.maxY - box.minY;
  const y0 = Math.round(box.minY + span * 0.2);
  const y1 = Math.round(box.minY + span * 0.78);
  const edges = [];
  for (let y = y0; y <= y1; y += 1) {
    if (left.has(y)) edges.push(left.get(y));
  }
  if (!edges.length) return null;
  edges.sort((a, b) => a - b);
  return {
    x: edges[Math.floor(edges.length * 0.15)],
    y: y0,
    w: 13,
    h: Math.max(8, y1 - y0),
  };
}

function drawTerminal(svg, defs, paths, cells, obj, tileW, tileH) {
  const own = cellsInside(cells, obj);
  const d = paths.get(11);
  if (!d || !own.length) return;
  const box = boundsOf(own);
  const url = clip(defs, "apron-clip-terminal", d);
  const g = el("g", { "clip-path": url, class: "apron-terminal-art" }, svg);
  const roofH = Math.max(tileH * 5, (box.maxY - box.minY) * tileH * 0.16);
  el("rect", {
    class: "apron-roof",
    x: box.minX * tileW,
    y: box.minY * tileH,
    width: (box.maxX - box.minX + 1) * tileW,
    height: roofH,
  }, g);
  el("rect", {
    class: "apron-ribbon",
    x: box.minX * tileW,
    y: box.minY * tileH + roofH,
    width: (box.maxX - box.minX + 1) * tileW,
    height: tileH * 1.7,
  }, g);
  el("rect", {
    class: "apron-ribbon-2",
    x: box.minX * tileW,
    y: box.minY * tileH + roofH + tileH * 1.7,
    width: (box.maxX - box.minX + 1) * tileW,
    height: tileH * 1.5,
  }, g);
  let windows = "";
  const yWindows = (box.minY + (box.maxY - box.minY) * 0.42) * tileH;
  for (let col = box.minX + 2; col <= box.maxX - 3; col += 4) {
    windows += `M${col * tileW} ${yWindows}h${tileW * 2.2}v${tileH * 1.7}h${-tileW * 2.2}z`;
    windows += `M${col * tileW} ${yWindows + tileH * 3.1}h${tileW * 2.2}v${tileH * 1.7}h${-tileW * 2.2}z`;
  }
  if (windows) el("path", { class: "apron-window", d: windows }, g);
  const [cx, cy] = centroid(own, [box.minX, box.minY]);
  placeLabel(svg, (cx + 0.5) * tileW, (box.minY + (box.maxY - box.minY) * 0.72 + 0.5) * tileH, obj.name || "Терминал", 30);
}

function drawHangar(svg, defs, paths, cells, obj, tileW, tileH) {
  const own = cellsInside(cells, obj);
  const d = paths.get(8);
  if (!d || !own.length) return;
  const box = boundsOf(own);
  const g = el("g", { "clip-path": clip(defs, "apron-clip-hangar", d), class: "apron-hangar-art" }, svg);
  el("rect", {
    class: "apron-roof-metal",
    x: box.minX * tileW,
    y: box.minY * tileH,
    width: (box.maxX - box.minX + 1) * tileW,
    height: Math.max(tileH * 6, (box.maxY - box.minY) * tileH * 0.12),
  }, g);
  let ribs = "";
  for (let col = box.minX + 6; col < box.maxX; col += 8) {
    ribs += `M${(col + 0.5) * tileW} ${box.minY * tileH}V${(box.maxY + 1) * tileH}`;
  }
  if (ribs) {
    el("path", {
      class: "apron-rib",
      d: ribs,
      "stroke-width": 1.6,
      "stroke-linecap": "butt",
    }, g);
  }
  const door = hangarDoor(own);
  if (door) {
    const x = door.x * tileW;
    const y = door.y * tileH;
    const w = door.w * tileW;
    const h = door.h * tileH;
    el("rect", { class: "apron-door-frame", x, y, width: w, height: h }, g);
    el("rect", {
      class: "apron-door",
      x: x + tileW * 0.6,
      y: y + tileH * 0.45,
      width: w - tileW * 1.2,
      height: h - tileH * 0.45,
    }, g);
    el("rect", {
      class: "apron-door-frame",
      x: x + w / 2 - tileW * 0.35,
      y: y + tileH * 0.45,
      width: tileW * 0.7,
      height: h - tileH * 0.45,
    }, g);
  }
  const [cx, cy] = centroid(own, [Number(obj.x), Number(obj.y)]);
  placeLabel(svg, (cx + 6.5) * tileW, (cy + 0.5) * tileH, obj.name || "Ангар", 36);
}

function drawWarehouse(svg, defs, paths, cells, obj, tileW, tileH, index) {
  const own = cellsInside(cells, obj);
  const d = paths.get(12);
  if (!d) return;
  const g = el("g", { "clip-path": clip(defs, `apron-clip-warehouse-${index}`, d) }, svg);
  const x = Number(obj.x) * tileW;
  const y = Number(obj.y) * tileH;
  const w = Number(obj.w || 1) * tileW;
  const h = Number(obj.h || 1) * tileH;
  el("rect", { class: "apron-roof-wood", x, y, width: w, height: Math.max(tileH * 2.2, h * 0.28) }, g);
  el("rect", {
    class: "apron-door",
    x: x + w * 0.34,
    y: y + h * 0.58,
    width: w * 0.32,
    height: h * 0.32,
  }, g);
  const [cx, cy] = centroid(own, [Number(obj.x) + Number(obj.w || 1) / 2, Number(obj.y) + Number(obj.h || 1) / 2]);
  placeLabel(svg, (cx + 0.5) * tileW, (cy + 0.5) * tileH, obj.name || "Склад", 18);
}

function drawParking(svg, defs, paths, obj, tileW, tileH) {
  const d = paths.get(9);
  if (!d) return;
  const g = el("g", { "clip-path": clip(defs, "apron-clip-parking", d) }, svg);
  const x0 = Number(obj.x);
  const y0 = Number(obj.y);
  const w = Number(obj.w || 1);
  const h = Number(obj.h || 1);
  let stalls = "";
  for (let i = 1; i < w; i += 1) {
    const x = (x0 + i) * tileW;
    stalls += `M${x} ${y0 * tileH}V${(y0 + h) * tileH}`;
  }
  if (stalls) {
    el("path", {
      class: "apron-stall",
      d: stalls,
      "stroke-width": 1.5,
      "stroke-linecap": "butt",
    }, g);
  }
  placeLabel(
    svg,
    (x0 + 0.4) * tileW,
    (y0 + h / 2) * tileH,
    obj.name || "Стоянка",
    15,
    { "text-anchor": "start" },
  );
}

function zoneCaption(zone) {
  const label = String(zone.label || "").trim();
  if (label) return label.slice(0, 16);
  return String(zone.id || "").trim().slice(0, 16);
}

function drawZones(svg, zones, tileW, tileH, metres, objects) {
  const group = el("g", { class: "apron-zones" }, svg);
  const named = new Set((objects || []).map((obj) => String(obj.name || "").trim().toLowerCase()).filter(Boolean));
  const placed = [];
  for (const zone of zones || []) {
    const radiusM = Number(zone.radius_m);
    if (!Number.isFinite(radiusM) || radiusM <= 0) continue;
    const cx = (Number(zone.x) + 0.5) * tileW;
    const cy = (Number(zone.y) + 0.5) * tileH;
    const r = (radiusM / metres) * tileW;
    el("circle", {
      class: "apron-zone",
      cx: cx.toFixed(1),
      cy: cy.toFixed(1),
      r: r.toFixed(2),
      "stroke-width": 2.4,
      "stroke-dasharray": "6 4",
    }, group);
    const caption = zoneCaption(zone);
    if (!caption || named.has(caption.toLowerCase())) continue;
    let x = cx;
    let y = cy - r - 11;
    const limit = labelFrame?.h || y + 18;
    let crowded = false;
    for (let n = 0; n < 6; n += 1) {
      const hit = placed.some((item) => Math.abs(item.x - x) < 110 && Math.abs(item.y - y) < 20);
      if (!hit) break;
      if (n === 5 || y + 18 > limit - 8) {
        crowded = true;
        break;
      }
      y += 18;
    }
    if (crowded) continue;
    const fitted = clampLabel(x, y, caption, 16, "middle", labelFrame);
    x = fitted[0];
    y = fitted[1];
    placed.push({ x, y });
    text(group, {
      class: "apron-zone-label",
      x: x.toFixed(1),
      y: y.toFixed(1),
      "text-anchor": "middle",
      "dominant-baseline": "middle",
      "font-size": Number(fitted[2]).toFixed(1),
      "stroke-width": 3,
    }, caption);
  }
}

function thinMask(mask, width, height) {
  const kill = [];
  let guard = 0;
  let changed = true;
  while (changed && guard < 24) {
    changed = false;
    guard += 1;
    for (let step = 0; step < 2; step += 1) {
      kill.length = 0;
      for (let y = 1; y < height - 1; y += 1) {
        const row = y * width;
        for (let x = 1; x < width - 1; x += 1) {
          const i = row + x;
          if (!mask[i]) continue;
          const p2 = mask[i - width];
          const p3 = mask[i - width + 1];
          const p4 = mask[i + 1];
          const p5 = mask[i + width + 1];
          const p6 = mask[i + width];
          const p7 = mask[i + width - 1];
          const p8 = mask[i - 1];
          const p9 = mask[i - width - 1];
          const neighbors = p2 + p3 + p4 + p5 + p6 + p7 + p8 + p9;
          if (neighbors < 2 || neighbors > 6) continue;
          const ring = [p2, p3, p4, p5, p6, p7, p8, p9, p2];
          let turns = 0;
          for (let k = 0; k < 8; k += 1) if (!ring[k] && ring[k + 1]) turns += 1;
          if (turns !== 1) continue;
          if (step === 0) {
            if (p2 && p4 && p6) continue;
            if (p4 && p6 && p8) continue;
          } else if ((p2 && p4 && p8) || (p2 && p6 && p8)) continue;
          kill.push(i);
        }
      }
      if (kill.length) changed = true;
      for (let n = 0; n < kill.length; n += 1) mask[kill[n]] = 0;
    }
  }
}

function rdp(points, epsilon) {
  if (points.length < 3) return points;
  const ax = points[0][0];
  const ay = points[0][1];
  const bx = points[points.length - 1][0];
  const by = points[points.length - 1][1];
  const dx = bx - ax;
  const dy = by - ay;
  const len = Math.hypot(dx, dy) || 1;
  let max = 0;
  let index = 0;
  for (let i = 1; i < points.length - 1; i += 1) {
    const dist = Math.abs((points[i][0] - ax) * dy - (points[i][1] - ay) * dx) / len;
    if (dist > max) {
      max = dist;
      index = i;
    }
  }
  if (max <= epsilon) return [points[0], points[points.length - 1]];
  const left = rdp(points.slice(0, index + 1), epsilon);
  const right = rdp(points.slice(index), epsilon);
  return left.slice(0, -1).concat(right);
}

function taxiCenterline(cells, tileW, tileH) {
  if (!cells.length) return "";
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const [x, y] of cells) {
    if (x < minX) minX = x;
    if (y < minY) minY = y;
    if (x > maxX) maxX = x;
    if (y > maxY) maxY = y;
  }
  const ox = minX - 1;
  const oy = minY - 1;
  const width = maxX - minX + 3;
  const height = maxY - minY + 3;
  const mask = new Uint8Array(width * height);
  for (const [x, y] of cells) mask[(y - oy) * width + (x - ox)] = 1;
  thinMask(mask, width, height);
  const seen = new Uint8Array(mask.length);
  const dirs = [[1, 0], [-1, 0], [0, 1], [0, -1], [1, 1], [1, -1], [-1, 1], [-1, -1]];
  const seeds = [];
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) if (mask[y * width + x]) seeds.push([x, y]);
  }
  const at = (x, y) => x >= 0 && y >= 0 && x < width && y < height && mask[y * width + x];
  const neigh = (x, y) => {
    const out = [];
    for (const [dx, dy] of dirs) if (at(x + dx, y + dy)) out.push([x + dx, y + dy]);
    return out;
  };
  const lines = [];
  const trace = (x, y) => {
    const line = [];
    const join = neigh(x, y).find(([nx, ny]) => seen[ny * width + nx]);
    if (join) line.push([join[0] + ox, join[1] + oy]);
    let cx = x;
    let cy = y;
    while (cx != null) {
      const index = cy * width + cx;
      if (seen[index]) break;
      seen[index] = 1;
      line.push([cx + ox, cy + oy]);
      const next = neigh(cx, cy).filter(([nx, ny]) => !seen[ny * width + nx]);
      if (!next.length) break;
      let pick = next[0];
      if (next.length > 1 && line.length >= 2) {
        const prev = line[line.length - 2];
        const pdx = cx - (prev[0] - ox);
        const pdy = cy - (prev[1] - oy);
        let best = -Infinity;
        for (const candidate of next) {
          const dot = (candidate[0] - cx) * pdx + (candidate[1] - cy) * pdy;
          if (dot > best) {
            best = dot;
            pick = candidate;
          }
        }
      }
      cx = pick[0];
      cy = pick[1];
    }
    return line;
  };
  for (const [x, y] of seeds) {
    if (seen[y * width + x] || neigh(x, y).length !== 1) continue;
    const line = trace(x, y);
    if (line.length > 3) lines.push(rdp(line, 1));
  }
  for (const [x, y] of seeds) {
    if (seen[y * width + x]) continue;
    const line = trace(x, y);
    if (line.length > 3) lines.push(rdp(line, 1));
  }
  let d = "";
  for (const line of lines) {
    for (let i = 0; i < line.length; i += 1) {
      const x = ((line[i][0] + 0.5) * tileW).toFixed(1);
      const y = ((line[i][1] + 0.5) * tileH).toFixed(1);
      d += `${i ? "L" : "M"}${x} ${y}`;
    }
  }
  return d;
}

function buildApronSvg(tmj, data) {
  const tileW = tmj.tilewidth || 4;
  const tileH = tmj.tileheight || tileW;
  const width = tmj.width;
  const height = tmj.height;
  const pxW = width * tileW;
  const pxH = height * tileH;
  labelFrame = { w: pxW, h: pxH };
  const { paths, cells } = runsOf(data, width, height, tileW, tileH);
  const svg = el("svg", {
    xmlns: SVG_NS,
    viewBox: `0 0 ${pxW} ${pxH}`,
    width: pxW,
    height: pxH,
    preserveAspectRatio: "xMidYMid meet",
    class: "apron-svg",
  });
  const style = el("style", { type: "text/css" }, svg);
  style.textContent = APRON_STYLE;
  const defs = el("defs", {}, svg);
  drawGrass(svg, pxW, pxH);
  const ground = el("g", { class: "apron-cells" }, svg);
  for (const id of DRAW_ORDER) {
    const d = paths.get(id);
    if (!d) continue;
    if (SHADOW_IDS.has(id)) el("path", { class: "apron-shadow", d, transform: "translate(3 4)" }, ground);
    el("path", { class: CELL_CLASS[id] || "apron-other", d }, ground);
  }
  let taxiD = "";
  try {
    taxiD = taxiCenterline(cells[2], tileW, tileH);
  } catch (error) {
    console.warn("[map] ось рулёжки", error);
  }
  if (taxiD && paths.get(2)) {
    el("path", {
      class: "apron-taxi-line",
      d: taxiD,
      "clip-path": clip(defs, "apron-clip-taxi", paths.get(2)),
      "stroke-width": 2.8,
      "stroke-linecap": "round",
      "stroke-linejoin": "round",
    }, svg);
  }
  drawRunway(svg, defs, paths, cells, tmj.objects, tileW, tileH);
  let warehouseIndex = 0;
  for (const obj of tmj.objects || []) {
    if (obj.kind === "terminal") drawTerminal(svg, defs, paths, cells[11], obj, tileW, tileH);
    else if (obj.kind === "hangar") drawHangar(svg, defs, paths, cells[8], obj, tileW, tileH);
    else if (obj.kind === "warehouse") {
      drawWarehouse(svg, defs, paths, cells[12], obj, tileW, tileH, warehouseIndex);
      warehouseIndex += 1;
    } else if (obj.kind === "parking") drawParking(svg, defs, paths, obj, tileW, tileH);
  }
  drawZones(svg, tmj.zones, tileW, tileH, tmj.cell_size_m || 5, tmj.objects);
  labelFrame = null;
  return svg;
}

function cloneSvg(markup) {
  const parsed = new DOMParser().parseFromString(markup, "image/svg+xml");
  const svg = document.importNode(parsed.documentElement, true);
  svg.setAttribute("class", "apron-svg");
  return svg;
}

export function renderApronSvg(tmj) {
  if (tmj?.format !== 2) return null;
  const data = tileLayerData(tmj);
  if (!data) return null;
  try {
    const key = `${tmj.width}x${tmj.height}:${hashGrid(data)}:${tmj.objects?.length || 0}:${tmj.zones?.length || 0}`;
    if (cachedSvg.key !== key) {
      const built = buildApronSvg(tmj, data);
      cachedSvg = { key, markup: new XMLSerializer().serializeToString(built) };
      return built;
    }
    return cloneSvg(cachedSvg.markup);
  } catch (error) {
    console.warn("[map] svg", error);
    return null;
  }
}

export function apronOverlayUrl(tmj) {
  const svg = renderApronSvg(tmj);
  if (!svg) return null;
  const xml = new XMLSerializer().serializeToString(svg);
  return URL.createObjectURL(new Blob([xml], { type: "image/svg+xml;charset=utf-8" }));
}
