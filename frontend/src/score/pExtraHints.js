// доп. баллы: текст подсказки и наличие speechSynthesis.

export function hintLine(text) {
  return String(text ?? "").trim();
}

export function canSpeak() {
  return typeof window !== "undefined" && Boolean(window.speechSynthesis);
}
