// Локальная очередь ЦПУ; serverId — id ответа POST /api/requests.
const KEY = "oto-mcc-queue";

export function listMcc() {
  try {
    const items = JSON.parse(localStorage.getItem(KEY) || "[]");
    return Array.isArray(items) ? items : [];
  } catch {
    return [];
  }
}

function save(items) {
  localStorage.setItem(KEY, JSON.stringify(items));
}

export function addMcc({ issue, planeUuid, description, serverId = null, serverStatus = null }) {
  const item = {
    id: crypto.randomUUID(),
    serverId,
    issue,
    planeUuid,
    description,
    createdAt: new Date().toISOString(),
    status: serverStatus || "new",
  };
  const items = listMcc();
  items.unshift(item);
  save(items);
  return item;
}

// Только после успешного POST /api/assign — не при выборе заявки в очереди.
export function markMccAssigned(id) {
  if (!id) return;
  save(listMcc().map((item) => (item.id === id ? { ...item, status: "assigned" } : item)));
}

export function markMccQueued(id) {
  if (!id) return;
  save(listMcc().map((item) => (item.id === id ? { ...item, status: "unassigned" } : item)));
}

export function isMccAssigned(item) {
  return item?.status === "assigned";
}

export const MCC_QUEUE_KEY = KEY;
