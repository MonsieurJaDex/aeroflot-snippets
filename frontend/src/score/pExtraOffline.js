// доп. офлайн: устаревшая позиция и очередь без сети.

export function staleLabel(lastMs, nowMs) {
  const last = Number(lastMs);
  if (lastMs == null || !Number.isFinite(last)) return "нет точки";
  const now = Number(nowMs);
  const age = Number.isFinite(now) ? Math.max(0, Math.floor((now - last) / 1000)) : 0;
  return `последняя известная + ${age} с`;
}

export function shouldQueue(online) {
  return online === false;
}
