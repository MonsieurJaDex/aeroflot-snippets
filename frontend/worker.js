const taskContent = document.getElementById("task-content");
const taskState = document.getElementById("task-state");
const acceptButton = document.getElementById("accept-task");
const routePreview = document.getElementById("route-preview");
const notice = document.getElementById("worker-notice");
const mapDistance = document.getElementById("map-distance");
const workerMap = L.map("worker-map", { crs: L.CRS.Simple, zoomControl: false, attributionControl: false, minZoom: -1, maxZoom: 3 });
const mapBounds = [[0, 0], [64, 64]];
let workerRouteLayer = null;
let workerMarkers = null;

L.control.zoom({ position: "bottomright" }).addTo(workerMap);
workerMap.fitBounds(mapBounds);

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
  const stand = typeof STANDS === "undefined"
    ? null
    : STANDS.find((item) => item.col === planePoint[0] && item.row === planePoint[1]);
  return stand ? stand.id : `${planePoint[0]}, ${planePoint[1]}`;
}

function renderTask(task) {
  updateProfile(task);
  if (!task) {
    taskContent.textContent = "Новых заявок нет.";
    taskState.textContent = "ОЖИДАНИЕ";
    acceptButton.hidden = true;
    routePreview.hidden = true;
    notice.textContent = "Ожидание назначения от диспетчера.";
    renderRouteMap(null);
    return;
  }

  const issueLabel = AeroAuth.issueLabel(task.issue);
  const distanceCells = Math.max(0, task.route.length - 1);
  taskContent.innerHTML = `<strong>ВС: ${standLabel(task.plane_point)}</strong><br>${issueLabel}<br>${task.description}<br>${routeInstruction(task.route)}<br>Маршрут: ${distanceCells} клеток`;
  taskState.textContent = task.is_accepted ? "В РАБОТЕ" : "НОВОЕ";
  acceptButton.hidden = task.is_accepted;
  routePreview.hidden = false;
  routePreview.textContent = task.route.map((point) => `[${point[0]}, ${point[1]}]`).join(" → ");
  notice.textContent = task.is_accepted ? "Задание принято. Следуйте к месту стоянки." : "Диспетчер назначил вас на заявку.";
  renderRouteMap(task);
}

function renderRouteMap(task) {
  if (workerRouteLayer) workerMap.removeLayer(workerRouteLayer);
  if (workerMarkers) workerMap.removeLayer(workerMarkers);
  if (!task) {
    mapDistance.textContent = "НЕТ МАРШРУТА";
    workerMap.fitBounds(mapBounds);
    return;
  }

  const points = task.route.map(([x, y]) => [64 - y - 0.5, x + 0.5]);
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
    await AeroAuth.apiRequest("/api/tasks/current/accept", { method: "POST" });
    await loadCurrentTask();
  } catch (error) {
    notice.textContent = `Не удалось принять задание: ${error.message}`;
  } finally {
    acceptButton.disabled = false;
  }
});

document.getElementById("logout-button").addEventListener("click", () => AeroAuth.logout());

if (session) {
  loadCurrentTask();
  window.setInterval(loadCurrentTask, 10000);
}