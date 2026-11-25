# Текст п.21: заранее и с тем, кто ещё успевает.


def margin_minutes(seconds):
    if isinstance(seconds, bool) or not isinstance(seconds, (int, float)):
        return None
    if seconds <= 0:
        return None
    return max(1, int(round(seconds / 60)))


def engineer_label(name, engineer_id):
    raw = (name or "").strip()
    if raw:
        if raw.lower().startswith("инженер"):
            return raw
        return f"инженер {raw}"
    if engineer_id:
        return f"инженер {str(engineer_id)[:8]}"
    return None


def alert_line(item, name=None):
    """Фраза из критерия 21, если замена ещё укладывается в срок."""
    engineer_id = item.get("alternative_engineer_id")
    minutes = margin_minutes(item.get("alternative_margin_seconds"))
    label = engineer_label(name, engineer_id)
    if engineer_id and label and minutes is not None:
        return f"не успевает, ближайший свободный – {label}, успевает с запасом {minutes} мин"
    if engineer_id and label:
        return f"не успевает, ближайший свободный – {label}"
    return "не успевает, замены в пуле нет"


def has_solution(item):
    return bool(item.get("alternative_engineer_id")) and margin_minutes(
        item.get("alternative_margin_seconds")
    ) is not None
