// Цепочка start → pickup → end, null-точки отбрасываются (п.6).

function asIntPair(point) {
  if (!Array.isArray(point) || point.length < 2) return null;
  const x = Math.round(Number(point[0]));
  const y = Math.round(Number(point[1]));
  if (!Number.isFinite(x) || !Number.isFinite(y)) return null;
  return [x, y];
}

export function viaStops(start, pickup, end) {
  return [start, pickup, end].map(asIntPair).filter(Boolean);
}

export function describeVia(stops) {
  const n = Math.max(0, (Array.isArray(stops) ? stops.length : 0) - 2);
  const mod10 = n % 10;
  const mod100 = n % 100;
  let word = "промежуточных точек";
  if (mod10 === 1 && mod100 !== 11) word = "промежуточная точка";
  else if (mod10 >= 2 && mod10 <= 4 && (mod100 < 12 || mod100 > 14)) word = "промежуточные точки";
  return `${n} ${word}`;
}
