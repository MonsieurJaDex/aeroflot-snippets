// п.22: имя journal.csv со штампом и UTF-8 BOM для Excel.
export function journalFilename(date = new Date()) {
  const stamp = date.toISOString().slice(0, 19).replace(/:/g, "-");
  return `journal-${stamp}.csv`;
}

export function withBom(csv) {
  return `\uFEFF${csv}`;
}
