// п.21: заранее, с вариантом кто ещё успевает.
export function alertText(item) {
  const message = item?.message || "не успевает";
  const name = item?.engineer || item?.name;
  const margin = item?.marginMinutes;
  if (name && typeof margin === "number") {
    return `${message}, ближайший свободный – ${name}, успевает с запасом ${margin} мин`;
  }
  if (name) return `${message}, ближайший свободный – ${name}`;
  if (typeof margin === "number") return `${message}, запас ${margin} мин`;
  return String(message);
}

export function isEarly(item) {
  return typeof item?.marginMinutes === "number" && item.marginMinutes > 0;
}
