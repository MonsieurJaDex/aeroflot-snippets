// Клетка после POST /api/v1/track: снап или булавка сервера, 404 — legacy.

function asCell(value) {
  if (!Array.isArray(value) || value.length < 2) return null;
  if (!Number.isFinite(value[0]) || !Number.isFinite(value[1])) return null;
  return value;
}

export function cellAfterAck({ applied, httpStatus, serverCell, localCell, previousCell }) {
  const local = asCell(localCell);
  const server = asCell(serverCell);
  const previous = asCell(previousCell);
  if (httpStatus === 404 && local) return { cell: local, writeLegacy: true };
  if (applied && server) return { cell: server, writeLegacy: false };
  if (applied) return { cell: local || previous || null, writeLegacy: false };
  return { cell: previous || null, writeLegacy: false };
}
