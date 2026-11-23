const taskContent = document.getElementById("task-content");
const taskState = document.getElementById("task-state");
const completeButton = document.getElementById("complete-task");
const aogButton = document.getElementById("aog-task");
const notice = document.getElementById("worker-notice");
const toast = document.getElementById("worker-toast");
const mapDistance = document.getElementById("map-distance");
const selfPosition = document.getElementById("self-position");
const workerMap = L.map("worker-map", { crs: L.CRS.Simple, zoomControl: false, attributionControl: false, minZoom: -1, maxZoom: 3 });
window.addEventListener("resize", () => workerMap.invalidateSize());

let mapWidth = 64;
let mapHeight = 64;
let tileSize = 16;
let workerMapOverlay = null;
let selfMarker = null;
let selfPoint = null;
let walk = null;
let walkFrame = 0;
let previousTaskId = null;
let toastTimer = null;
let aogOn = false;
let currentTask = null;

L.control.zoom({ position: "bottomright" }).addTo(workerMap);

function mapBounds() {
  return [[0, 0], [mapHeight * tileSize, mapWidth * tileSize]];
}

workerMap.fitBounds(mapBounds());

async function loadWorkerMapBackground() {
  try {
    const res = await fetch("./real_map.tmj");
    if (!res.ok) return;

    const loadImg = (src) =>
      new Promise((resolve, reject) => {
        const image = new Image();
        image.onload = () => resolve(image);
        image.onerror = reject;
        image.src = src;
      });

    const [groundImage, planeImage, tmj] = await Promise.all([
      loadImg("assets/tileset_custom.png"),
      loadImg("assets/plane.png"),
      res.json(),
    ]);

    mapWidth = tmj.width;
    mapHeight = tmj.height;
    tileSize = tmj.tilewidth || 16;

    const canvas = document.createElement("canvas");
    canvas.width = tmj.width * tileSize;
    canvas.height = tmj.height * tileSize;
    const ctx = canvas.getContext("2d");
    ctx.fillStyle = "#d7e3ee";
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    const layer = tmj.layers.find((item) => item.type === "tilelayer");
    if (!layer) return;

    const sheets = [
      { firstgid: 1008, image: groundImage, columns: 5, tilecount: 10 },
      { firstgid: 999, image: planeImage, columns: 3, tilecount: 9 },
      { firstgid: 973, image: groundImage, columns: 5, tilecount: 10 },
    ];

    for (let i = 0; i < layer.data.length; i++) {
      const raw = layer.data[i];
      if (!raw) continue;
      const gid = raw & 0x1fffffff;
      const sheet = sheets.find((item) => gid >= item.firstgid);
      if (!sheet) continue;
      const localId = gid - sheet.firstgid;
      if (localId < 0 || localId >= sheet.tilecount) continue;
      const col = i % layer.width;
      const row = Math.floor(i / layer.width);
      const sx = (localId % sheet.columns) * 16;
      const sy = Math.floor(localId / sheet.columns) * 16;
      ctx.drawImage(sheet.image, sx, sy, 16, 16, col * tileSize, row * tileSize, tileSize, tileSize);
    }

    if (workerMapOverlay) workerMap.removeLayer(workerMapOverlay);
    workerMapOverlay = L.imageOverlay(canvas.toDataURL("image/png"), mapBounds()).addTo(workerMap);
    if (selfPoint) placeSelfMarker();
    focusWorkerMap();
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

function planeLabel(task) {
  const point = Array.isArray(task.plane_point) ? `${task.plane_point[0]}, ${task.plane_point[1]}` : "—";
  const id = task.plane_uuid ? task.plane_uuid.slice(0, 8) : "без uuid";
  return `${id} · ${point}`;
}

function renderTask(task) {
  if (task && task.id !== previousTaskId) {
    showToast("Новое назначение получено");
    aogOn = false;
  }
  if (!task && previousTaskId) showToast("Задача закрыта");
  previousTaskId = task ? task.id : null;
  currentTask = task;

  updateProfile(task);
  if (!task) {
    taskContent.textContent = "Новых заявок нет.";
    taskState.textContent = "ОЖИДАНИЕ";
    completeButton.hidden = true;
    aogButton.hidden = true;
    notice.textContent = "Ожидание назначения от диспетчера. Нажатие на карту отправляет вашу позицию.";
    renderRouteMap(null);
    return;
  }

  const issueLabel = AeroAuth.issueLabel(task.issue);
  const route = Array.isArray(task.route) ? task.route : [];
  const distanceCells = Math.max(0, route.length - 1);
  const routeText = route.length < 2 ? "Вы уже у воздушного судна." : `Маршрут: ${distanceCells} клеток`;
  taskContent.innerHTML = `<strong>ВС ${planeLabel(task)}</strong><br>${issueLabel}<br>${task.description || ""}<br>${routeText}`;
  taskState.textContent = aogOn ? "AOG" : "В РАБОТЕ";
  completeButton.hidden = false;
  aogButton.hidden = !task.plane_uuid;
  aogButton.textContent = aogOn ? "Снять AOG" : "Поставить AOG";
  notice.textContent = "Задача активна. Завершите её, когда работы закончены.";
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

function stopWalk() {
  if (walkFrame) cancelAnimationFrame(walkFrame);
  walkFrame = 0;
  if (!walk) return;
  if (walk.line) workerMap.removeLayer(walk.line);
  if (walk.marker) workerMap.removeLayer(walk.marker);
  if (walk.dest) workerMap.removeLayer(walk.dest);
  walk = null;
}

function centerLatLng(center) {
  return [mapHeight * tileSize - center[1] * tileSize, center[0] * tileSize];
}

function renderRouteMap(task) {
  const route = task && Array.isArray(task.route) ? task.route : [];
  if (!task || route.length < 2) {
    stopWalk();
    mapDistance.textContent = task ? "УЖЕ НА МЕСТЕ" : "НЕТ МАРШРУТА";
    focusWorkerMap();
    return;
  }
  if (walk && walk.taskId === task.id) return;

  stopWalk();
  const centers = route.map(([x, y]) => [x + 0.5, y + 0.5]);
  walk = {
    taskId: task.id,
    centers,
    startedAt: performance.now(),
    done: false,
    line: L.polyline(centers.map(centerLatLng), {
      color: "#c5161c",
      weight: 5,
      opacity: 0.95,
      lineJoin: "round",
    }).addTo(workerMap),
    marker: L.marker(centerLatLng(centers[0]), { icon: mapIcon("worker-map-engineer") })
      .bindTooltip("Вы", { permanent: true, direction: "top", className: "map-label" })
      .addTo(workerMap),
    dest: L.marker(centerLatLng(centers.at(-1)), { icon: mapIcon("worker-map-stand") })
      .bindTooltip(`ВС ${planeLabel(task)}`, { permanent: true, direction: "top", className: "map-label" })
      .addTo(workerMap),
  };
  mapDistance.textContent = `${Math.max(0, route.length - 1)} КЛЕТОК`;
  workerMap.fitBounds(walk.line.getBounds(), { padding: [36, 36], maxZoom: 2 });
  walkFrame = requestAnimationFrame(tickWalk);
}

function tickWalk(now) {
  walkFrame = 0;
  if (!walk || walk.done) return;
  const traveled = ((now - walk.startedAt) / 1000) * AeroWalk.WALK_MPS;
  const sample = AeroWalk.pointAt(walk.centers, traveled);
  if (!sample) return;
  const at = centerLatLng(sample.done ? walk.centers.at(-1) : sample.point);
  walk.marker.setLatLng(at);
  if (sample.done) {
    workerMap.removeLayer(walk.line);
    walk.line = null;
    walk.done = true;
    mapDistance.textContent = "НА МЕСТЕ";
    return;
  }
  const rest = walk.centers.slice(sample.nextIndex).map(centerLatLng);
  walk.line.setLatLngs([at, ...rest]);
  const leftMeters = Math.max(0, AeroWalk.totalMeters(walk.centers) - traveled);
  const cellsLeft = Math.ceil(leftMeters / AeroWalk.CELL_METERS - 1e-6);
  mapDistance.textContent = `${cellsLeft} КЛЕТОК`;
  walkFrame = requestAnimationFrame(tickWalk);
}

function mapIcon(className) {
  return L.divIcon({ className, html: "", iconSize: [18, 18], iconAnchor: [9, 9] });
}

function placeSelfMarker() {
  if (!selfPoint) return;
  if (selfMarker) workerMap.removeLayer(selfMarker);
  selfMarker = L.marker(centerLatLng([selfPoint[0] + 0.5, selfPoint[1] + 0.5]), {
    icon: mapIcon("worker-map-self"),
    zIndexOffset: 700,
  })
    .bindTooltip("Ваша точка", { permanent: true, direction: "right", className: "map-label" })
    .addTo(workerMap);
  selfPosition.textContent = `ВЫ: ${selfPoint[0]}, ${selfPoint[1]}`;
}

function showSelfPoint(point) {
  const next = [Number(point[0]), Number(point[1])];
  if (!Number.isFinite(next[0]) || !Number.isFinite(next[1])) return;
  const same = selfPoint && selfPoint[0] === next[0] && selfPoint[1] === next[1];
  selfPoint = next;
  if (same && selfMarker) return;
  placeSelfMarker();
  if (!walk) focusWorkerMap();
}

function focusWorkerMap() {
  const latlngs = [];
  if (walk && walk.line) latlngs.push(...walk.line.getLatLngs());
  if (selfMarker) latlngs.push(selfMarker.getLatLng());
  if (latlngs.length >= 2) {
    workerMap.fitBounds(L.latLngBounds(latlngs), { padding: [36, 36], maxZoom: 2 });
    return;
  }
  if (selfMarker) {
    workerMap.setView(selfMarker.getLatLng(), 1);
    return;
  }
  workerMap.fitBounds(mapBounds());
}

async function refreshSelfPosition() {
  if (!session || !session.userId) return;
  try {
    const engineers = await AeroAuth.apiRequest("/api/geo/getEngineersPositions");
    if (!Array.isArray(engineers)) return;
    const mine = engineers.find((entry) => String(entry.id) === String(session.userId));
    if (!mine || !Array.isArray(mine.position)) return;
    showSelfPoint(mine.position);
  } catch (error) {
    console.warn("[worker] своя позиция", error);
  }
}

async function loadCurrentTask() {
  try {
    const task = await AeroAuth.apiRequest("/api/tasks/current");
    renderTask(task && task.id ? task : null);
  } catch (error) {
    if (error.status === 404) {
      renderTask(null);
      return;
    }
    notice.textContent = `Не удалось получить назначение: ${error.message}`;
  }
}

completeButton.addEventListener("click", async () => {
  completeButton.disabled = true;
  try {
    await AeroAuth.apiRequest("/api/tasks/current/complete", { method: "POST" });
    showToast("Задача завершена");
    await loadCurrentTask();
  } catch (error) {
    notice.textContent = `Не удалось завершить задачу: ${error.message}`;
  } finally {
    completeButton.disabled = false;
  }
});

aogButton.addEventListener("click", async () => {
  aogButton.disabled = true;
  try {
    const data = await AeroAuth.apiRequest("/api/tasks/current/aog", {
      method: "POST",
      body: { toggle: !aogOn },
    });
    aogOn = Boolean(data && data.aog);
    if (currentTask) renderTask(currentTask);
    showToast(aogOn ? "Самолёт отмечен как AOG" : "AOG снят");
  } catch (error) {
    notice.textContent = `Не удалось обновить AOG: ${error.message}`;
  } finally {
    aogButton.disabled = false;
  }
});

workerMap.on("click", async (event) => {
  if (!session || !session.userId) return;
  const x = Math.floor(event.latlng.lng / tileSize);
  const y = mapHeight - 1 - Math.floor(event.latlng.lat / tileSize);
  if (x < 0 || y < 0 || x >= mapWidth || y >= mapHeight) return;
  try {
    await AeroAuth.apiRequest("/api/geo/updateEngineerPosition", {
      method: "POST",
      auth: false,
      body: {
        id: session.userId,
        batch: [[[x, y], Math.floor(Date.now() / 1000)]],
      },
    });
    showSelfPoint([x, y]);
    notice.textContent = `Позиция отправлена: ${x}, ${y}`;
  } catch (error) {
    notice.textContent = `Не удалось отправить позицию: ${error.message}`;
  }
});

document.getElementById("logout-button").addEventListener("click", () => AeroAuth.logout());

if (session) {
  AeroAuth.hydrateProfile()
    .catch((error) => console.warn("[worker] профиль", error))
    .finally(() => {
      const fresh = AeroAuth.getSession();
      if (fresh) {
        session.name = fresh.name;
        session.engineerType = fresh.engineerType;
        session.userId = fresh.userId;
      }
      updateProfile(null);
      loadCurrentTask();
      refreshSelfPosition();
      window.setInterval(() => {
        loadCurrentTask();
        refreshSelfPosition();
      }, 5000);
      window.requestAnimationFrame(() => workerMap.invalidateSize());
    });
}
