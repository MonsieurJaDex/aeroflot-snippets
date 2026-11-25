// Полигон it2026. Картинка берётся из документа v2, не из TMJ.
const KEY = "oto-map-id";

export const MAP_FILES = {
  it2026_aero: { url: "/it2026_aero.json", name: "Полигон it2026", width: 412, height: 372 },
};

export function activeMapId() {
  return "it2026_aero";
}

export function mapUrl() {
  return MAP_FILES[activeMapId()].url;
}

export function activateMap(id) {
  if (!MAP_FILES[id]) return null;
  localStorage.setItem(KEY, id);
  return { id, ...MAP_FILES[id] };
}
