// п.8: пересчёт маршрута после geo не чаще minGapMs.

export function shouldRefreshRoute(lastAt, now, minGapMs = 15000) {
  const t1 = Number(now);
  const gap = Number(minGapMs);
  if (!Number.isFinite(t1) || !Number.isFinite(gap) || gap < 0) return false;
  const t0 = Number(lastAt);
  if (!Number.isFinite(t0)) return true;
  return t1 - t0 >= gap;
}

export function remainingRoute(route, index) {
  if (!Array.isArray(route)) return [];
  const i = Number(index);
  if (!Number.isInteger(i) || i < 0 || i > route.length) return [];
  return route.slice(i);
}
