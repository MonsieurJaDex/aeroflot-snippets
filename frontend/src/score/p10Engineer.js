// П.10: действия инженера по текущей задаче.
export const ENGINEER_ACTIONS = [
  { name: "accept", label: "Принять" },
  { name: "reject", label: "Отклонить" },
  { name: "tools", label: "Инструмент" },
  { name: "waiting-part", label: "Ожидание запчасти" },
  { name: "mel", label: "MEL" },
  { name: "complete", label: "Завершить" },
  { name: "aog", label: "AOG" },
];

export function actionEnabled(name, task) {
  if (!task) return false;
  if (name === "accept") return task.is_accepted === false;
  return true;
}
