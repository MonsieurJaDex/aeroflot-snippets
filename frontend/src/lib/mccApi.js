import { apiRequest } from "./session.js";

// Создание замечания и подсказка типа, без назначения.
export function createRemark(body) {
  return apiRequest("/api/requests", { method: "POST", body });
}

export function classifyRemark(text) {
  return apiRequest("/api/classify", { method: "POST", body: { text } });
}
