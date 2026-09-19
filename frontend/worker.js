const STANDS = [
  { id: "A-01", row: 10, col: 10 },
  { id: "A-02", row: 16, col: 10 },
  { id: "B-01", row: 10, col: 51 },
  { id: "B-02", row: 17, col: 51 },
  { id: "C-01", row: 44, col: 32 },
  { id: "Техцентр", row: 51, col: 11 },
];

const taskContent = document.getElementById("task-content");
const taskState = document.getElementById("task-state");
const acceptButton = document.getElementById("accept-task");
const notice = document.getElementById("worker-notice");
const toast = document.getElementById("worker-toast");
const mapDistance = document.getElementById("map-distance");
const workerMap = L.map("worker-map", { crs: L.CRS.Simple, zoomControl: false, attributionControl: false, minZoom: -1, maxZoom: 3 });
const mapBounds = [[0, 0], [64 * 16, 64 * 16]];
let workerRouteLayer = null;
let workerMarkers = null;
let workerMapOverlay = null;
let previousTaskId = null;
let previousTaskAccepted = null;
let toastTimer = null;

L.control.zoom({ position: "bottomright" }).addTo(workerMap);
workerMap.fitBounds(mapBounds);

async function loadWorkerMapBackground() {
  try {
    const res = await fetch("./real_map.tmj");
    if (!res.ok) return;
    const image = new Image();
    image.src = "assets/tileset_custom.png";
    await new Promise((resolve, reject) => {
      image.onload = resolve;
      image.onerror = reject;
    });

    const tmj = await res.json();
    const tileW = tmj.tilewidth || 16;
    const tileH = tmj.tileheight || 16;
    const canvas = document.createElement("canvas");
    canvas.width = tmj.width * tileW;
    canvas.height = tmj.height * tileH;
    const ctx = canvas.getContext("2d");
    ctx.fillStyle = "#1b252d";
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    const layer = tmj.layers.find((item) => item.type === "tilelayer");
    if (!layer) return;

    const firstGids = [1008, 973];
    for (let i = 0; i < layer.data.length; i++) {
      const raw = layer.data[i];
      if (!raw) continue;
      let gid = raw & 0x1fffffff;
      let tilesetFirst = 973;
      for (const candidate of firstGids) {
        if (gid >= candidate) {
          tilesetFirst = candidate;
          break;
        }
      }
      const localId = gid - tilesetFirst;
      if (localId < 0 || localId >= 10) continue;
      const col = i % layer.width;
      const row = Math.floor(i / layer.width);
      const sx = (localId % 5) * 16;
      const sy = Math.floor(localId / 5) * 16;
      ctx.drawImage(image, sx, sy, 16, 16, col * tileW, row * tileH, tileW, tileH);
    }

    if (workerMapOverlay) workerMap.removeLayer(workerMapOverlay);
    workerMapOverlay = L.imageOverlay(canvas.toDataURL("image/png"), [
      [0, 0],
      [canvas.height, canvas.width],
    ]).addTo(workerMap);
  } catch (error) {
    console.warn("[worker] фон карты не загружен", error);
  }
}

loadWorkerMapBackground();

const session = AeroAuth.requireRole("Engineer");

function updateProfile(task) {
  const initials = session.name
    .split(" ")
    .map((part) => part[0])
    .join("")
    .slice(0, 2)
    .toUpperCase();

  const skillEntry = AeroAuth.ENGINEER_TYPES.find(([value]) => value === session.engineerType);

  document.getElementById("account-avatar").textContent = initials || "??";
  document.getElementById("account-name").textContent = session.name;
  document.getElementById("account-role").textContent = skillEntry ? skillEntry[1] : "Инженер ОТО";
  document.getElementById("worker-skill").textContent = skillEntry ? skillEntry[1] : "Специализация не указана";
  document.getElementById("worker-status").textContent = task ? "Занят" : "Свободен";
  document.getElementById("worker-status").className = `worker-status ${task ? "busy" : "free"}`;
}

function standLabel(planePoint) {
  const stand = STANDS.find((item) => item.col === planePoint[0] && item.row === planePoint[1]);
  return stand ? stand.id : `${planePoint[0]}, ${planePoint[1]}`;
}

function renderTask(task) {
  const taskChanged = task && task.id !== previousTaskId;
  const taskClosed = !task && previousTaskId;
  const taskAccepted = task && task.is_accepted && previousTaskAccepted === false;
  if (taskChanged) showToast("Новое назначение получено");
  if (taskAccepted) showToast("Задание принято и переведено в работу");
  if (taskClosed) showToast("Задача закрыта диспетчером");
  previousTaskId = task ? task.id : null;
  previousTaskAccepted = task ? task.is_accepted : null;

  updateProfile(task);
  if (!task) {
    taskContent.textContent = "Новых заявок нет.";
    taskState.textContent = "ОЖИДАНИЕ";
    acceptButton.hidden = true;
    notice.textContent = "Ожидание назначения от диспетчера.";
    renderRouteMap(null);
    return;
  }

  const issueLabel = AeroAuth.issueLabel(task.issue);
  const distanceCells = Math.max(0, task.route.length - 1);
  taskContent.innerHTML = `<strong>ВС: ${standLabel(task.plane_point)}</strong><br>${issueLabel}<br>${task.description}<br>${routeInstruction(task.route)}<br>Маршрут: ${distanceCells} клеток`;
  taskState.textContent = task.is_accepted ? "В РАБОТЕ" : "НОВОЕ";
  acceptButton.hidden = task.is_accepted;
  notice.textContent = task.is_accepted ? "Задание принято. Следуйте к месту стоянки." : "Диспетчер назначил вас на заявку.";
  renderRouteMap(task);
}

function showToast(message) {
  toast.textContent = message;
  toast.hidden = false;
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => {
    toast.hidden = true;
  }, 4500);
}

function renderRouteMap(task) {
  if (workerRouteLayer) workerMap.removeLayer(workerRouteLayer);
  if (workerMarkers) workerMap.removeLayer(workerMarkers);
  if (!task) {
    mapDistance.textContent = "НЕТ МАРШРУТА";
    workerMap.fitBounds(mapBounds);
    return;
  }

  const tileSize = 16;
  const mapHeight = 64 * tileSize;
  const points = task.route.map(([x, y]) => [
    mapHeight - (y + 0.5) * tileSize,
    (x + 0.5) * tileSize,
  ]);
  workerRouteLayer = L.polyline(points, { color: "#e30613", weight: 5, opacity: 0.95, lineJoin: "round" }).addTo(workerMap);
  workerMarkers = L.layerGroup([
    L.marker(points[0], { icon: mapIcon("worker-map-engineer") }).bindTooltip("Вы", { permanent: true, direction: "top", className: "map-label" }),
    L.marker(points.at(-1), { icon: mapIcon("worker-map-stand") }).bindTooltip(`ВС: ${standLabel(task.plane_point)}`, { permanent: true, direction: "top", className: "map-label" }),
  ]).addTo(workerMap);
  mapDistance.textContent = `${Math.max(0, task.route.length - 1)} КЛЕТОК`;
  workerMap.fitBounds(workerRouteLayer.getBounds(), { padding: [36, 36], maxZoom: 2 });
}

function mapIcon(className) {
  return L.divIcon({ className, html: "", iconSize: [18, 18], iconAnchor: [9, 9] });
}

function routeInstruction(route) {
  const [startX, startY] = route[0];
  const [endX, endY] = route.at(-1);
  const vertical = endY - startY;
  const horizontal = endX - startX;
  const steps = [];
  if (vertical) steps.push(`${Math.abs(vertical)} клеток ${vertical < 0 ? "вверх" : "вниз"}`);
  if (horizontal) steps.push(`${Math.abs(horizontal)} клеток ${horizontal < 0 ? "влево" : "вправо"}`);
  return steps.length ? `Двигайтесь: ${steps.join(", затем ")}.` : "Вы уже на месте.";
}

async function loadCurrentTask() {
  try {
    const task = await AeroAuth.apiRequest("/api/tasks/current");
    renderTask(task);
  } catch (error) {
    notice.textContent = `Не удалось получить назначение: ${error.message}`;
  }
}

acceptButton.addEventListener("click", async () => {
  acceptButton.disabled = true;
  try {
    // Ручка используется: POST /api/tasks/current/accept
    await AeroAuth.apiRequest("/api/tasks/current/accept", { method: "POST" });
    await loadCurrentTask();
  } catch (error) {
    const raw = String(error.message || "");
    if (/no active task/i.test(raw)) {
      notice.textContent = "Активного задания нет — обновите список или дождитесь нового назначения.";
    } else {
      notice.textContent = `Не удалось принять задание: ${raw}`;
    }
    await loadCurrentTask();
  } finally {
    acceptButton.disabled = false;
  }
});

document.getElementById("logout-button").addEventListener("click", () => AeroAuth.logout());

if (session) {
  loadCurrentTask();
  window.setInterval(loadCurrentTask, 10000);
}