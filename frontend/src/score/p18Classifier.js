// П.18: payload и проверка текста/ATA для классификатора.
export function defectPayload(text, ataChapter) {
  return { text: String(text ?? "").trim(), ata_chapter: String(ataChapter ?? "").trim() };
}

export function defectError(text, ata) {
  if (!String(text ?? "").trim() || !String(ata ?? "").trim()) {
    return "Нужны текст дефекта и глава ATA.";
  }
  return "";
}
