// п.19: переназначение AOG и подпись ожидания запчасти
export function reassignAllowed(role, status, reason) {
  return (
    role === "Dispatcher" &&
    (status === "in_progress" || status === "accepted") &&
    Boolean(String(reason || "").trim())
  );
}

export function waitingPartLabel() {
  return "ожидание запчасти";
}
