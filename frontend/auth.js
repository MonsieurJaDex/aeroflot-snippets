// Общий модуль авторизации и работы с реальным API бэкенда (Axum + JWT).
const AeroAuth = (() => {
  const SESSION_KEY = "oto-session";
  const API_BASE_KEY = "oto-api-base";
  const DEFAULT_API_BASE = "http://127.0.0.1:3001";

  // Справочники совпадают с backend enums (AircraftIssue / EngineerType).
  const AIRCRAFT_ISSUES = [
    ["fuel_leak_from_drain_cap", "Подтекание топлива из дренажных колпачков"],
    ["oil_stain_near_gearbox", "Масляные пятна в районе редуктора"],
    ["hydraulic_leak_on_strut", "Следы гидравлики на штоках амортизаторов шасси"],
    ["fairing_chip_or_scratch", "Сколы и царапины на обтекателях/антеннах/фонарях"],
    ["paint_peeling_at_rivets", "Отслоение краски в зонах клёпки"],
    ["missing_pitot_cover", "Отсутствие заглушек на приёмниках давления"],
    ["uneven_tread_wear", "Неравномерный износ протектора"],
    ["tire_cut_to_cord", "Порезы до корда"],
    ["low_tire_pressure", "Низкое давление в шинах"],
    ["indication_fault", "Сбои индикации (лампа, предохранитель)"],
    ["loose_connector", "Ослабленный разъём"],
    ["seatbelt_adjustment", "Регулировка привязных ремней"],
    ["burned_out_signal_lamp", "Перегоревшая светосигнальная лампа"],
    ["thrust_or_parameter_drop", "Падение тяги/оборотов/температуры газов"],
    ["excessive_vibration", "Повышенная вибрация (дисбаланс)"],
    ["metal_debris_in_oil_filter", "Стружка в маслофильтре"],
    ["radar_failure_or_false_reading", "Отказ/ложные показания РЛС"],
    ["comms_loss_or_distortion", "Потеря связи / искажение сигнала"],
    ["ins_gyro_drift", "Уход гироплатформы ИНС"],
    ["other", "Другое"],
  ];

  const ENGINEER_TYPES = [
    ["integrity_inspector", "Инспектор целостности и герметичности"],
    ["crew_remarks_handler", "Обработка замечаний экипажа"],
    ["fueling_crew", "Заправка (топливо/масло/кислород)"],
    ["engine_technician", "Техник по двигателю"],
    ["avionics_engineer", "Инженер по авионике"],
    ["aviation_technician", "Авиатехник (диагностика/ремонт)"],
  ];

  const ENGINEER_TYPE_LABELS = new Map(ENGINEER_TYPES);

  function getApiBase() {
    return localStorage.getItem(API_BASE_KEY) || DEFAULT_API_BASE;
  }

  function setApiBase(value) {
    localStorage.setItem(API_BASE_KEY, value.replace(/\/$/, ""));
  }

  function getSession() {
    return JSON.parse(sessionStorage.getItem(SESSION_KEY) || "null");
  }

  function saveSession(session) {
    sessionStorage.setItem(SESSION_KEY, JSON.stringify(session));
  }

  function clearSession() {
    sessionStorage.removeItem(SESSION_KEY);
  }

  function decodeJwtPayload(token) {
    const part = token.split(".")[1];
    const base64 = part.replace(/-/g, "+").replace(/_/g, "/");
    return JSON.parse(decodeURIComponent(escape(atob(base64))));
  }

  async function tryRefresh() {
    const session = getSession();
    if (!session || !session.refreshToken) return false;
    try {
      const res = await fetch(`${getApiBase()}/api/auth/update_access`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ refresh_token: session.refreshToken }),
      });
      if (!res.ok) return false;
      const data = await res.json();
      session.accessToken = data.access_token;
      saveSession(session);
      return true;
    } catch (e) {
      return false;
    }
  }

  async function apiRequest(path, { method = "GET", body, auth = true, retry = true } = {}) {
    // Единая точка всех запросов: Bearer JWT + авто-refresh при 401.
    const headers = { "Content-Type": "application/json" };
    if (auth) {
      const session = getSession();
      if (!session) throw new Error("Нет активной сессии, войдите заново.");
      headers.Authorization = `Bearer ${session.accessToken}`;
    }

    const res = await fetch(`${getApiBase()}${path}`, {
      method,
      headers,
      body: body ? JSON.stringify(body) : undefined,
    });

    if (res.status === 401 && auth && retry) {
      const refreshed = await tryRefresh();
      if (refreshed) return apiRequest(path, { method, body, auth, retry: false });
      clearSession();
      window.location.href = "index.html";
      throw new Error("Сессия истекла, требуется повторный вход.");
    }

    if (!res.ok) {
      const text = await res.text().catch(() => "");
      throw new Error(text || `Ошибка запроса: ${res.status}`);
    }

    const contentType = res.headers.get("content-type") || "";
    return contentType.includes("application/json") ? res.json() : res.text();
  }

  async function login(email, password) {
    const data = await apiRequest("/api/auth/login", {
      method: "POST",
      auth: false,
      body: { email, password },
    });
    const payload = decodeJwtPayload(data.access_token);
    const session = {
      name: data.name,
      role: data.user_role,
      userId: payload.sub,
      accessToken: data.access_token,
      refreshToken: data.refresh_token,
    };
    saveSession(session);
    return session;
  }

  async function register({ email, name, password, userRole, engineerType }) {
    const data = await apiRequest("/api/auth/register", {
      method: "POST",
      auth: false,
      body: {
        email,
        name,
        password,
        user_role: userRole,
        engineer_type: engineerType || null,
      },
    });
    const payload = decodeJwtPayload(data.access_token);
    const session = {
      name: data.name,
      role: data.user_role,
      userId: payload.sub,
      engineerType: engineerType || null,
      accessToken: data.access_token,
      refreshToken: data.refresh_token,
    };
    saveSession(session);

    if (userRole === "Engineer") {
      await assignRandomStartingPosition(payload.sub);
    }

    return session;
  }

  // Backend не задаёт позицию инженеру при регистрации, поэтому фиксируем
  // стартовую точку через simulate endpoint — иначе assign не видит инженера в Redis.
  async function assignRandomStartingPosition(engineerId) {
    const x = 8 + Math.floor(Math.random() * 48);
    const y = 8 + Math.floor(Math.random() * 48);
    try {
      await apiRequest("/api/simulate/update_engineer_position", {
        method: "POST",
        auth: false,
        body: { id: engineerId, new_point: [x, y] },
      });
    } catch (error) {
      console.warn("[auth] не удалось задать стартовую позицию инженера", error);
    }
  }

  function logout() {
    clearSession();
    window.location.href = "index.html";
  }

  function requireRole(role) {
    const session = getSession();
    if (!session || session.role !== role) {
      window.location.href = "index.html";
      return null;
    }
    return session;
  }

  function issueLabel(value) {
    const found = AIRCRAFT_ISSUES.find(([v]) => v === value);
    return found ? found[1] : value;
  }

  function engineerTypeForIssue(value) {
    const groups = {
      fuel_leak_from_drain_cap: "integrity_inspector",
      oil_stain_near_gearbox: "integrity_inspector",
      hydraulic_leak_on_strut: "integrity_inspector",
      fairing_chip_or_scratch: "aviation_technician",
      paint_peeling_at_rivets: "aviation_technician",
      missing_pitot_cover: "fueling_crew",
      uneven_tread_wear: "fueling_crew",
      tire_cut_to_cord: "fueling_crew",
      low_tire_pressure: "fueling_crew",
      indication_fault: "crew_remarks_handler",
      loose_connector: "crew_remarks_handler",
      seatbelt_adjustment: "crew_remarks_handler",
      burned_out_signal_lamp: "crew_remarks_handler",
      thrust_or_parameter_drop: "engine_technician",
      excessive_vibration: "engine_technician",
      metal_debris_in_oil_filter: "engine_technician",
      radar_failure_or_false_reading: "avionics_engineer",
      comms_loss_or_distortion: "avionics_engineer",
      ins_gyro_drift: "avionics_engineer",
      other: "not_categorized",
    };
    const type = groups[value] || "not_categorized";
    return ENGINEER_TYPE_LABELS.get(type) || type;
  }

  return {
    AIRCRAFT_ISSUES,
    ENGINEER_TYPES,
    getApiBase,
    setApiBase,
    getSession,
    saveSession,
    clearSession,
    apiRequest,
    login,
    register,
    logout,
    requireRole,
    issueLabel,
    engineerTypeForIssue,
  };
})();
