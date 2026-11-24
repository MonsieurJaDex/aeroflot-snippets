// 5 км/ч и клетка 30 м — норматив финала.
// Совпадает с map2.location.json cell_size_m.
export const CELL_METERS = 5;
export const WALK_MPS = 5 / 3.6;

function keyOf(x, y) {
  return `${x},${y}`;
}

function neighbors4(x, y) {
  return [[x + 1, y], [x - 1, y], [x, y + 1], [x, y - 1]];
}

function bfsRoad(start, end, walkMask) {
  const sx = Math.floor(start[0]);
  const sy = Math.floor(start[1]);
  const ex = Math.floor(end[0]);
  const ey = Math.floor(end[1]);
  if (sx === ex && sy === ey) return [[sx, sy]];
  const h = walkMask?.length || 0;
  const w = h ? walkMask[0].length : 0;
  const inBounds = (x, y) => y >= 0 && x >= 0 && y < h && x < w;
  const ok = (x, y) => inBounds(x, y) && (walkMask[y][x] || (x === sx && y === sy) || (x === ex && y === ey));
  if (!ok(sx, sy) && !inBounds(sx, sy)) return null;
  const queue = [[sx, sy]];
  const prev = new Map([[keyOf(sx, sy), null]]);
  for (let i = 0; i < queue.length; i += 1) {
    const [cx, cy] = queue[i];
    if (cx === ex && cy === ey) break;
    for (const [nx, ny] of neighbors4(cx, cy)) {
      const k = keyOf(nx, ny);
      if (prev.has(k) || !ok(nx, ny)) continue;
      prev.set(k, [cx, cy]);
      queue.push([nx, ny]);
    }
  }
  if (!prev.has(keyOf(ex, ey))) return null;
  const path = [[ex, ey]];
  let cur = keyOf(ex, ey);
  while (prev.get(cur)) {
    const p = prev.get(cur);
    path.push(p);
    cur = keyOf(p[0], p[1]);
  }
  path.reverse();
  return path;
}

/** Маршрут только по 4-связным дорогам — без диагоналей сквозь стены. */
export function expandRouteOnRoads(route, walkMask) {
  if (!Array.isArray(route) || route.length < 2) {
    return Array.isArray(route) ? route.map((p) => [Math.floor(p[0]), Math.floor(p[1])]) : [];
  }
  const out = [];
  const push = (pt) => {
    const next = [Math.floor(pt[0]), Math.floor(pt[1])];
    const last = out[out.length - 1];
    if (last && last[0] === next[0] && last[1] === next[1]) return;
    out.push(next);
  };
  push(route[0]);
  for (let i = 1; i < route.length; i += 1) {
    const a = out[out.length - 1];
    const b = [Math.floor(route[i][0]), Math.floor(route[i][1])];
    const manh = Math.abs(a[0] - b[0]) + Math.abs(a[1] - b[1]);
    if (manh <= 1) {
      push(b);
      continue;
    }
    const via = walkMask?.length ? bfsRoad(a, b, walkMask) : null;
    if (via && via.length) {
      for (const pt of via.slice(1)) push(pt);
      continue;
    }
    // Запас: ортогональный угол без BFS (всё ещё не диагональ).
    if (a[0] !== b[0]) push([b[0], a[1]]);
    push(b);
  }
  return out;
}

export function segmentMeters(a, b) {
  return Math.hypot(b[0] - a[0], b[1] - a[1]) * CELL_METERS;
}

export function totalMeters(centers) {
  let meters = 0;
  for (let i = 1; i < centers.length; i += 1) meters += segmentMeters(centers[i - 1], centers[i]);
  return meters;
}

export function pointAt(centers, meters) {
  if (!centers.length) return null;
  if (centers.length === 1 || meters <= 0) {
    return {
      point: centers[0].slice(),
      nextIndex: centers.length > 1 ? 1 : 0,
      done: centers.length < 2,
    };
  }

  let left = meters;
  for (let i = 1; i < centers.length; i += 1) {
    const len = segmentMeters(centers[i - 1], centers[i]);
    if (len <= 0) continue;
    if (left <= len) {
      const t = left / len;
      const last = i === centers.length - 1;
      return {
        point: [
          centers[i - 1][0] + (centers[i][0] - centers[i - 1][0]) * t,
          centers[i - 1][1] + (centers[i][1] - centers[i - 1][1]) * t,
        ],
        nextIndex: i,
        done: last && t >= 1,
      };
    }
    left -= len;
  }

  return {
    point: centers[centers.length - 1].slice(),
    nextIndex: centers.length,
    done: true,
  };
}

function heading(a, b) {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  if (Math.abs(dx) >= Math.abs(dy)) return dx > 0 ? "восток" : dx < 0 ? "запад" : null;
  return dy > 0 ? "юг" : dy < 0 ? "север" : null;
}

/** Текстовая подсказка поворота по клеткам маршрута (опц. баллы ТЗ). */
export function turnHint(centers, nextIndex) {
  if (!Array.isArray(centers) || centers.length < 2) return "Маршрут построен";
  const i = Math.max(1, Math.min(Number(nextIndex) || 1, centers.length - 1));
  if (i >= centers.length - 1) return "Прямо до зоны обслуживания ВС";
  const prev = heading(centers[i - 1], centers[i]);
  const next = heading(centers[i], centers[i + 1]);
  if (!prev || !next || prev === next) {
    return next ? `Держитесь курса: ${next}` : "Прямо по маршруту";
  }
  const order = ["север", "восток", "юг", "запад"];
  const a = order.indexOf(prev);
  const b = order.indexOf(next);
  const diff = (b - a + 4) % 4;
  if (diff === 1) return `Впереди поворот направо → ${next}`;
  if (diff === 3) return `Впереди поворот налево → ${next}`;
  return `Разворот, затем ${next}`;
}
