// Локальная очередь геоточек при обрыве связи; после online — досылка.
const KEY = "oto-geo-outbox";

function read() {
  try {
    const rows = JSON.parse(localStorage.getItem(KEY) || "[]");
    return Array.isArray(rows) ? rows : [];
  } catch {
    return [];
  }
}

function write(rows) {
  localStorage.setItem(KEY, JSON.stringify(rows.slice(-200)));
}

export function enqueueEngineerPoint(id, point, t = Math.floor(Date.now() / 1000)) {
  // В очереди достаточно последней точки на инженера — старые при восстановлении сети лишние.
  const rows = read().filter((row) => String(row.id) !== String(id));
  rows.push({ id, point, t });
  write(rows);
}

export function enqueueGpsFix(id, fix) {
  // Последний GPS-фикс на id; клеточные строки без kind не трогаем.
  const { lat, lon, t, acc, spd } = fix;
  const rows = read().filter((row) => row.kind !== "gps" || String(row.id) !== String(id));
  rows.push({ kind: "gps", id, lat, lon, t, acc, spd });
  write(rows);
}

export function pendingCount() {
  return read().length;
}

export async function flushEngineerOutbox(send) {
  const rows = read();
  if (!rows.length) return 0;
  const left = [];
  let sent = 0;
  for (const row of rows) {
    try {
      await send(row);
      sent += 1;
    } catch {
      left.push(row);
    }
  }
  write(left);
  return sent;
}
