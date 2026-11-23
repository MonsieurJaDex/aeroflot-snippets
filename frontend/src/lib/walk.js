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
