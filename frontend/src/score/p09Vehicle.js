import { ISSUE_REQUIRED_VEHICLE, vehicleLabel } from "../lib/catalog.js";

// п.9 спецтехника по коду неисправности

export function vehicleForIssue(issue) {
  return ISSUE_REQUIRED_VEHICLE[issue] ?? null;
}

export function vehicleLine(type) {
  if (!type) return "";
  const label = vehicleLabel(type);
  return label === String(type) ? "" : label;
}
