// 5 км/ч и клетка 30 м — норматив финала.
export const CELL_METERS = 30;
export const WALK_MPS = 5 / 3.6;

export function segmentMeters(a, b) {
  return Math.hypot(b[0] - a[0], b[1] - a[1]) * CELL_METERS;
}

export function totalMeters(centers) {
  let meters = 0;
  for (let i = 1; i < centers.length; i += 1) meters += segmentMeters(centers[i - 1], centers[i]);
  return meters;
}

export function pointAt(centers, meters) {
  if (!centers.length) return null;
  if (centers.length === 1 || meters <= 0) {
    return {
      point: centers[0].slice(),
      nextIndex: centers.length > 1 ? 1 : 0,
      done: centers.length < 2,
    };
  }

  let left = meters;
  for (let i = 1; i < centers.length; i += 1) {
    const len = segmentMeters(centers[i - 1], centers[i]);
    if (len <= 0) continue;
    if (left <= len) {
      const t = left / len;
      const last = i === centers.length - 1;
      return {
        point: [
          centers[i - 1][0] + (centers[i][0] - centers[i - 1][0]) * t,
          centers[i - 1][1] + (centers[i][1] - centers[i - 1][1]) * t,
        ],
        nextIndex: i,
        done: last && t >= 1,
      };
    }
    left -= len;
  }

  return {
    point: centers[centers.length - 1].slice(),
    nextIndex: centers.length,
    done: true,
  };
}

function heading(a, b) {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  if (Math.abs(dx) >= Math.abs(dy)) return dx > 0 ? "восток" : dx < 0 ? "запад" : null;
  return dy > 0 ? "юг" : dy < 0 ? "север" : null;
}

/** Текстовая подсказка поворота по клеткам маршрута (опц. баллы ТЗ). */
export function turnHint(centers, nextIndex) {
  if (!Array.isArray(centers) || centers.length < 2) return "Маршрут построен";
  const i = Math.max(1, Math.min(Number(nextIndex) || 1, centers.length - 1));
  if (i >= centers.length - 1) return "Прямо до зоны обслуживания ВС";
  const prev = heading(centers[i - 1], centers[i]);
  const next = heading(centers[i], centers[i + 1]);
  if (!prev || !next || prev === next) {
    return next ? `Держитесь курса: ${next}` : "Прямо по маршруту";
  }
  const order = ["север", "восток", "юг", "запад"];
  const a = order.indexOf(prev);
  const b = order.indexOf(next);
  const diff = (b - a + 4) % 4;
  if (diff === 1) return `Впереди поворот направо → ${next}`;
  if (diff === 3) return `Впереди поворот налево → ${next}`;
  return `Разворот, затем ${next}`;
}
