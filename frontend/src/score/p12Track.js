// тело POST /api/v1/track: точка или пачка {points}

export function trackBody(obj, lat, lon, t, acc, spd) {
  return { obj, lat, lon, t: t ?? new Date().toISOString(), acc, spd };
}

export function trackBatch(points) {
  return points.length > 1 ? { points } : points[0];
}
