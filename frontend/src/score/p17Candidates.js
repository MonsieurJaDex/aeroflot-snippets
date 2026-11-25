// П.17: выбранный id, eligible, меньший ETA; пустой ввод — [].

export function etaMinutes(seconds) {
  const value = Number(seconds);
  if (!Number.isFinite(value) || value < 0) return null;
  return Math.round(value / 60);
}

export function sortCandidates(rows, chosenId) {
  if (!Array.isArray(rows) || rows.length === 0) return [];
  const chosen = chosenId == null ? "" : String(chosenId);
  return [...rows].sort((a, b) => {
    if (a.engineerId === chosen) return -1;
    if (b.engineerId === chosen) return 1;
    if (a.eligible !== b.eligible) return Number(b.eligible) - Number(a.eligible);
    return Number(a.etaSeconds ?? 1e9) - Number(b.etaSeconds ?? 1e9);
  });
}
