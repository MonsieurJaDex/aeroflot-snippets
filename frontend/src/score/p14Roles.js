// п.14: Engineer/Dispatcher — допустимые действия
export function canDo(role, action) {
  if (role === "Engineer") {
    return (
      action === "accept" ||
      action === "reject" ||
      action === "mel" ||
      action === "aog" ||
      action === "tools" ||
      action === "waiting-part" ||
      action === "complete"
    );
  }
  if (role === "Dispatcher") {
    return action === "assign" || action === "reassign";
  }
  return false;
}
