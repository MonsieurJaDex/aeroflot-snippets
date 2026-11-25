// п.13: роль по шагу и открытость замечания.
export function chainRole(step) {
  if (step === "mcc") return "ЦПУ";
  if (step === "dispatcher") return "диспетчер";
  if (step === "engineer") return "инженер";
  return "";
}

export function remarkIsOpen(item) {
  return item?.status !== "assigned";
}
