// п.20: норматив 15 минут
export function slaExceeded(seconds) {
  return seconds > 900;
}

export function slaLine(seconds) {
  if (seconds == null) return "";
  const min = Math.round(Number(seconds) / 60);
  const line = `${min} мин`;
  if (slaExceeded(seconds)) return `${line} — больше 15 минут`;
  return line;
}
