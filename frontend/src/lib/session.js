const SESSION_KEY = "oto-session";
const API_BASE_KEY = "oto-api-base";
const DEFAULT_API_BASE = "http://127.0.0.1:3001";

// Бэкенд режет запрос по TimeoutLayer на 10 секундах и отдаёт 408.
// Пользователю показываем человеческую фразу, а не сырой текст ответа или код статуса.
export const SERVER_TIMEOUT_SECONDS = 10;
export const TIMEOUT_MESSAGE = "Сервер не успел ответить за " + SERVER_TIMEOUT_SECONDS + " секунд — повторите.";

export function isTimeoutStatus(status) {
  return status === 408 || status === 504;
}

export function isAbortError(err) {
  return Boolean(err) && (err.name === "AbortError" || err.name === "TimeoutError" || err.code === 20);
}

export function getApiBase() {
  const host = window.location.hostname;
  const loopback = !host || host === "localhost" || host === "127.0.0.1" || host === "::1";
  // Телефон и туннель: /api этого же сайта. Сохранённый 127.0.0.1 там — сам телефон.
  if (!loopback) return window.location.origin;
  // https-страница не может звать http://127.0.0.1:3001: браузер режет mixed content, /api идёт в прокси этого же сайта.
  if (window.location.protocol === "https:") return window.location.origin;
  const saved = localStorage.getItem(API_BASE_KEY);
  if (saved) return saved;
  return DEFAULT_API_BASE;
}

export function setApiBase(value) {
  localStorage.setItem(API_BASE_KEY, String(value || "").replace(/\/$/, "") || DEFAULT_API_BASE);
}

export function getSession() {
  try {
    return JSON.parse(sessionStorage.getItem(SESSION_KEY) || "null");
  } catch {
    return null;
  }
}

export function saveSession(session) {
  sessionStorage.setItem(SESSION_KEY, JSON.stringify(session));
}

export function clearSession() {
  sessionStorage.removeItem(SESSION_KEY);
}

async function tryRefresh() {
  const session = getSession();
  if (!session?.refreshToken) return false;
  try {
    const res = await fetch(`${getApiBase()}/api/auth/update_access`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ refresh_token: session.refreshToken }),
    });
    if (!res.ok) return false;
    const data = await res.json();
    session.accessToken = data.access_token;
    if (data.refresh_token) session.refreshToken = data.refresh_token;
    saveSession(session);
    return true;
  } catch {
    return false;
  }
}

export async function apiRequest(path, { method = "GET", body, auth = true, retry = true } = {}) {
  const headers = { "Content-Type": "application/json" };
  if (auth) {
    const session = getSession();
    if (!session) throw new Error("Нет активной сессии, войдите заново.");
    headers.Authorization = `Bearer ${session.accessToken}`;
  }

  let res;
  try {
    res = await fetch(`${getApiBase()}${path}`, {
      method,
      headers,
      body: body ? JSON.stringify(body) : undefined,
    });
  } catch (fetchError) {
    // Клиентский abort (AbortController / AbortSignal.timeout) — тот же таймаут,
    // а не «нет сети»: показываем ту же фразу, что и для 408.
    if (isAbortError(fetchError)) {
      const error = new Error(TIMEOUT_MESSAGE);
      error.status = 408;
      error.timeout = true;
      throw error;
    }
    const here = typeof window !== "undefined" && !["localhost", "127.0.0.1", "::1"].includes(window.location.hostname);
    const error = new Error(navigator.onLine === false
      ? "Нет сети. Проверьте соединение."
      : here
        ? "Нет связи с сервером. Обновите страницу."
        : "Сервер не отвечает. Проверьте адрес API.");
    error.status = 0;
    throw error;
  }

  if (res.status === 401 && auth && retry) {
    const refreshed = await tryRefresh();
    if (refreshed) return apiRequest(path, { method, body, auth, retry: false });
    clearSession();
    window.location.assign("/");
    throw new Error("Сессия истекла, требуется повторный вход.");
  }

  if (!res.ok) {
    const text = await res.text().catch(() => "");
    if (isTimeoutStatus(res.status) || /timed?\s?out/i.test(text)) {
      const error = new Error(TIMEOUT_MESSAGE);
      error.status = res.status;
      error.timeout = true;
      error.raw = text;
      throw error;
    }
    let message = text || `Ошибка запроса: ${res.status}`;
    try {
      const body = JSON.parse(text);
      if (body?.message) message = String(body.message);
      else if (body?.error) message = String(body.error);
    } catch {
      // текст не JSON
    }
    const error = new Error(message);
    error.status = res.status;
    error.raw = text;
    throw error;
  }

  const contentType = res.headers.get("content-type") || "";
  const data = contentType.includes("application/json") ? await res.json() : await res.text();
  return data;
}

export async function login(email, password) {
  const data = await apiRequest("/api/auth/login", {
    method: "POST",
    auth: false,
    body: { email, password },
  });
  saveSession({
    name: data.name,
    role: data.user_role,
    userId: data.id || null,
    accessToken: data.access_token,
    refreshToken: data.refresh_token,
  });
  await hydrateProfile();
  return getSession();
}

export async function register({ email, name, password, userRole, engineerType }) {
  const data = await apiRequest("/api/auth/register", {
    method: "POST",
    auth: false,
    body: {
      email,
      name,
      password,
      user_role: userRole,
      ...(engineerType ? { engineer_type: engineerType } : {}),
    },
  });
  saveSession({
    name: data.name,
    role: data.user_role,
    userId: data.id || null,
    engineerType: engineerType || null,
    accessToken: data.access_token,
    refreshToken: data.refresh_token,
  });
  await hydrateProfile();
  return getSession();
}

export async function hydrateProfile() {
  const session = getSession();
  if (!session) return null;
  try {
    const me = await apiRequest("/api/auth/me");
    session.name = me.name;
    session.role = me.user_role;
    session.userId = me.id;
    session.engineerType = me.engineer_type || null;
    session.email = me.email;
    saveSession(session);
    return session;
  } catch (error) {
    // сессия локально есть — профиль подтянем на следующем опросе
    console.warn("[auth] hydrate", error?.message || error);
    return session;
  }
}
