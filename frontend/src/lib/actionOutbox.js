// Очередь действий инженера без сети; после online уходит сама.
const KEY = "oto-action-outbox";

function read() {
  try {
    const rows = JSON.parse(localStorage.getItem(KEY) || "[]");
    return Array.isArray(rows) ? rows : [];
  } catch {
    return [];
  }
}

function write(rows) {
  localStorage.setItem(KEY, JSON.stringify(rows.slice(-40)));
}

export function enqueueAction(action) {
  const path = String(action?.path || "");
  if (!path) return;
  const rows = read().filter((row) => row.path !== path);
  rows.push({
    path,
    method: action.method || "POST",
    body: action.body ?? null,
    at: Date.now(),
  });
  write(rows);
}

export function pendingActionCount() {
  return read().length;
}

export async function flushActions(send) {
  const rows = read();
  if (!rows.length) return 0;
  const left = [];
  let sent = 0;
  for (const row of rows) {
    try {
      await send(row);
      sent += 1;
    } catch (error) {
      const status = Number(error?.status) || 0;
      if (status >= 400 && status < 500 && status !== 408) continue;
      left.push(row);
    }
  }
  write(left);
  return sent;
}
