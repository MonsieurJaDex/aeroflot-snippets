export const STATUS_STEPS = ["замечание", "не назначен", "назначен", "принял", "в работе", "выполнено"];

// Цепочка п.15: is_accepted; done/completed — выполнено.
export function statusFromTask(task) {
  if (!task) return "замечание";
  const st = String(task.status || "").toLowerCase();
  if (st === "done" || st === "completed") return "выполнено";
  if (task.is_accepted) return "в работе";
  return "назначен";
}
