const AGENTS = [
  { id: "ENG-014", name: "Алексей Смирнов", skillName: "Планер и двигатель", status: "free" },
  { id: "ENG-022", name: "Мария Волкова", skillName: "Авионика", status: "free" },
  { id: "ENG-031", name: "Илья Ким", skillName: "Планер и двигатель", status: "busy" },
  { id: "ENG-044", name: "Ольга Белова", skillName: "Авионика", status: "free" },
];

const selectedWorker = document.getElementById("worker-select");
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

for (const agent of AGENTS) selectedWorker.add(new Option(`${agent.name} · ${agent.id}`, agent.id));

function selectedAgent() {
  return AGENTS.find((agent) => agent.id === selectedWorker.value);
}

function updateProfile() {
  const agent = selectedAgent();
  document.getElementById("worker-skill").textContent = agent.skillName;
  document.getElementById("worker-status").textContent = agent.status === "free" ? "Свободен" : "Занят";
  document.getElementById("worker-status").className = `worker-status ${agent.status}`;
}

function renderTask() {
  const task = JSON.parse(localStorage.getItem("oto-assignment") || "null");
  const agent = selectedAgent();
  if (!task || task.engineerId !== agent.id) {
    taskContent.textContent = "Новых заявок нет.";
    taskState.textContent = "ОЖИДАНИЕ";
    acceptButton.hidden = true;
    routePreview.hidden = true;
    notice.textContent = "Ожидание назначения от диспетчера.";
    renderRouteMap(null);
    return;
  }

  taskContent.innerHTML = `<strong>ВС на стоянке ${task.stand}</strong><br>${task.fault}<br>${routeInstruction(task.route)}<br>Маршрут: ${task.distanceCells} клеток`;
  taskState.textContent = task.accepted ? "В РАБОТЕ" : "НОВОЕ";
  acceptButton.hidden = task.accepted;
  routePreview.hidden = false;
  routePreview.textContent = task.route.map((point) => `[${point[0]}, ${point[1]}]`).join(" → ");
  notice.textContent = task.accepted ? "Задание принято. Следуйте к месту стоянки." : "Диспетчер назначил вас на заявку.";
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
    L.marker(points.at(-1), { icon: mapIcon("worker-map-stand") }).bindTooltip(`Стоянка ${task.stand}`, { permanent: true, direction: "top", className: "map-label" }),
  ]).addTo(workerMap);
  mapDistance.textContent = `${task.distanceCells} КЛЕТОК`;
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

selectedWorker.addEventListener("change", () => {
  updateProfile();
  renderTask();
});

acceptButton.addEventListener("click", () => {
  const task = JSON.parse(localStorage.getItem("oto-assignment") || "null");
  if (!task) return;
  task.accepted = true;
  localStorage.setItem("oto-assignment", JSON.stringify(task));
  renderTask();
});

updateProfile();
renderTask();