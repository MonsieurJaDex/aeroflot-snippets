// п.11: свежесть фикса и зона стоянки.
export function fieldFixLabel(ageSec) {
  if (ageSec == null || ageSec === "") return "нет фикса";
  const age = Number(ageSec);
  if (!Number.isFinite(age) || age < 0) return "нет фикса";
  if (age < 15) return "свежий";
  return `устарел · ${Math.round(age)} с`;
}

function cell(pair) {
  if (!Array.isArray(pair) || pair.length < 2) return null;
  const x = pair[0];
  const y = pair[1];
  if (!Number.isInteger(x) || !Number.isInteger(y)) return null;
  return [x, y];
}

export function inStandZone(point, stand, radiusCells = 1) {
  const a = cell(point);
  const b = cell(stand);
  const r = Number(radiusCells);
  if (!a || !b || !Number.isFinite(r) || r < 0) return false;
  return Math.max(Math.abs(a[0] - b[0]), Math.abs(a[1] - b[1])) <= r;
}
