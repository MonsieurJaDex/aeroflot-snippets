# Чаты диспетчера и уже отправленные риски. Файл переживает перезапуск.

import json
from pathlib import Path


class Store:
    def __init__(self, path):
        self.path = Path(path)
        self.chats = []
        self.sent = {}
        self.actions = {}
        self.offset = 0
        self._load()

    def _load(self):
        if not self.path.exists():
            return
        try:
            data = json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            return
        self.chats = [int(item) for item in data.get("chats") or []]
        self.sent = dict(data.get("sent") or {})
        self.actions = dict(data.get("actions") or {})
        self.offset = int(data.get("offset") or 0)

    def save(self):
        self.path.parent.mkdir(parents=True, exist_ok=True)
        payload = {
            "chats": self.chats,
            "sent": self.sent,
            "actions": self.actions,
            "offset": self.offset,
        }
        self.path.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")

    def allow(self, chat_id):
        chat_id = int(chat_id)
        if chat_id not in self.chats:
            self.chats.append(chat_id)
            self.save()

    def drop(self, chat_id):
        chat_id = int(chat_id)
        self.chats = [item for item in self.chats if item != chat_id]
        self.save()

    def known(self, chat_id):
        return int(chat_id) in self.chats

    def mark_sent(self, key):
        self.sent[key] = True
        if len(self.sent) > 400:
            for old in list(self.sent)[:-200]:
                self.sent.pop(old, None)
        self.save()

    def already_sent(self, key):
        return key in self.sent

    def put_action(self, token, action):
        self.actions[token] = action
        if len(self.actions) > 80:
            for old in list(self.actions)[:-40]:
                self.actions.pop(old, None)
        self.save()

    def pop_action(self, token):
        action = self.actions.pop(token, None)
        self.save()
        return action
