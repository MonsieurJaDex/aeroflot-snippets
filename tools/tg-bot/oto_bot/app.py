# Long polling: риск уходит в чат диспетчера сразу, как его отдал /api/alerts.

import json
import os
import time
import urllib.error
import urllib.request
import uuid

from oto_bot.format_alert import alert_line, has_solution
from oto_bot.oto_api import ApiError, OtoClient
from oto_bot.store import Store

REASON = "риск норматива: не успевает, передача из Telegram"


def load_env(path):
    if not os.path.exists(path):
        return
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            key, value = line.split("=", 1)
            os.environ.setdefault(key.strip(), value.strip().strip('"').strip("'"))


class Bot:
    def __init__(self):
        self.token = os.environ["BOT_TOKEN"].strip()
        self.api = OtoClient(
            os.environ.get("API_BASE", "http://127.0.0.1:3001"),
            os.environ["DISPATCHER_EMAIL"].strip(),
            os.environ["DISPATCHER_PASSWORD"],
        )
        data_dir = os.environ.get("DATA_DIR", "data")
        self.store = Store(os.path.join(data_dir, "state.json"))
        pinned = os.environ.get("TELEGRAM_CHAT_ID", "").strip()
        if pinned:
            self.store.allow(pinned)
        self.poll_seconds = max(5, int(os.environ.get("POLL_SECONDS", "10")))
        self._names = {}
        self._next_alerts = 0

    def run(self):
        me = self._tg("getMe")
        print("бот @" + me.get("username", "?") + " слушает риски", flush=True)
        self.api.login()
        print("диспетчер вошёл, роль " + self.api.role, flush=True)
        while True:
            self._drain_updates()
            now = time.monotonic()
            if now >= self._next_alerts:
                self._next_alerts = now + self.poll_seconds
                self._push_alerts()

    def _drain_updates(self):
        updates = self._tg(
            "getUpdates",
            offset=self.store.offset,
            timeout=self.poll_seconds,
            allowed_updates=["message", "callback_query"],
        )
        for update in updates:
            self.store.offset = int(update["update_id"]) + 1
            self.store.save()
            try:
                self._on_update(update)
            except Exception as error:
                print("update: " + str(error), flush=True)

    def _on_update(self, update):
        if "callback_query" in update:
            self._on_callback(update["callback_query"])
            return
        message = update.get("message") or {}
        chat = message.get("chat") or {}
        chat_id = chat.get("id")
        text = (message.get("text") or "").strip()
        if chat_id is None or not text.startswith("/"):
            return
        command = text.split()[0].split("@")[0].lower()
        if command == "/start":
            self._bind(chat_id)
        elif command == "/stop":
            self.store.drop(chat_id)
            self._send(chat_id, "Этот чат больше не получает риски.")
        elif command == "/risks":
            self._send_current(chat_id)
        elif command == "/chatid":
            self._send(chat_id, "chat id: " + str(chat_id))

    def _bind(self, chat_id):
        pinned = os.environ.get("TELEGRAM_CHAT_ID", "").strip()
        if pinned and str(chat_id) != pinned:
            self._send(chat_id, "Этот бот привязан к другому чату начальника смены.")
            return
        self.store.allow(chat_id)
        self._send(
            chat_id,
            "Чат начальника смены подключён. "
            "Предупреждение придёт заранее: кто не успевает и кто ещё успеет. "
            "Кнопка передаёт заявку этому инженеру.",
        )

    def _send_current(self, chat_id):
        if not self._allowed(chat_id):
            self._send(chat_id, "Сначала /start в чате диспетчера.")
            return
        rows = self.api.alerts()
        if not rows:
            self._send(chat_id, "Сейчас рисков норматива нет.")
            return
        for item in rows:
            self._deliver(chat_id, item)

    def _push_alerts(self):
        if not self.store.chats:
            return
        try:
            rows = self.api.alerts()
        except ApiError as error:
            print("alerts: " + str(error), flush=True)
            return
        for item in rows:
            for chat_id in list(self.store.chats):
                key = _sent_key(item) + ":" + str(chat_id)
                if self.store.already_sent(key):
                    continue
                if self._deliver(chat_id, item):
                    self.store.mark_sent(key)

    def _deliver(self, chat_id, item):
        name = self._name(item.get("alternative_engineer_id"))
        line = alert_line(item, name)
        request_id = str(item.get("request_id") or "")
        text = "<b>Риск норматива</b>\n" + _esc(line) + "\nЗаявка <code>" + _esc(request_id) + "</code>"
        markup = None
        if has_solution(item):
            token = uuid.uuid4().hex[:12]
            self.store.put_action(
                token,
                {
                    "request_id": request_id,
                    "engineer_id": item.get("alternative_engineer_id"),
                    "line": line,
                },
            )
            markup = {
                "inline_keyboard": [[
                    {"text": "Передать этому инженеру", "callback_data": "go:" + token}
                ]]
            }
        try:
            self._send(chat_id, text, markup)
            return True
        except Exception as error:
            print("send: " + str(error), flush=True)
            return False

    def _on_callback(self, query):
        query_id = query.get("id")
        data = query.get("data") or ""
        message = query.get("message") or {}
        chat = message.get("chat") or {}
        chat_id = chat.get("id")
        if chat_id is None or not self._allowed(chat_id):
            self._answer(query_id, "Чат не подключён")
            return
        if not data.startswith("go:"):
            self._answer(query_id, "Неизвестная кнопка")
            return
        action = self.store.pop_action(data[3:])
        if not action:
            self._answer(query_id, "Кнопка уже использована")
            return
        try:
            self.api.reassign(action["request_id"], action["engineer_id"], REASON)
        except ApiError as error:
            self._answer(query_id, "Не передалось")
            self._send(chat_id, "Передача не прошла: " + _esc(str(error)))
            return
        self._answer(query_id, "Передано")
        self._send(
            chat_id,
            "Заявка <code>" + _esc(action["request_id"]) + "</code> передана. " + _esc(action["line"]),
        )

    def _name(self, engineer_id):
        if not engineer_id:
            return None
        if engineer_id not in self._names:
            self._names[engineer_id] = self.api.engineer_name(engineer_id)
        return self._names[engineer_id]

    def _allowed(self, chat_id):
        pinned = os.environ.get("TELEGRAM_CHAT_ID", "").strip()
        if pinned:
            return str(chat_id) == pinned
        return self.store.known(chat_id)

    def _send(self, chat_id, text, markup=None):
        payload = {
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "HTML",
            "disable_web_page_preview": True,
        }
        if markup:
            payload["reply_markup"] = markup
        return self._tg("sendMessage", **payload)

    def _answer(self, query_id, text):
        try:
            self._tg("answerCallbackQuery", callback_query_id=query_id, text=text)
        except Exception as error:
            print("callback: " + str(error), flush=True)

    def _tg(self, method, **payload):
        url = "https://api.telegram.org/bot" + self.token + "/" + method
        data = json.dumps(payload).encode("utf-8")
        request = urllib.request.Request(
            url,
            data=data,
            headers={"Content-Type": "application/json"},
        )
        timeout = 20 + int(payload.get("timeout") or 0)
        try:
            with urllib.request.urlopen(request, timeout=timeout) as response:
                body = json.loads(response.read().decode("utf-8"))
        except urllib.error.HTTPError as error:
            raw = error.read().decode("utf-8", errors="replace")
            raise RuntimeError(method + " " + str(error.code) + " " + raw[:200]) from None
        except urllib.error.URLError as error:
            raise RuntimeError(method + " " + str(error.reason)) from None
        if not body.get("ok"):
            raise RuntimeError(method + " " + json.dumps(body, ensure_ascii=False)[:200])
        return body.get("result")


def _sent_key(item):
    return str(item.get("request_id") or "") + ":" + str(item.get("alternative_engineer_id") or "-")


def _esc(value):
    return (
        str(value)
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
    )


def main():
    here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    load_env(os.path.join(here, ".env"))
    missing = [key for key in ("BOT_TOKEN", "DISPATCHER_EMAIL", "DISPATCHER_PASSWORD") if not os.environ.get(key)]
    if missing:
        raise SystemExit("заполни .env: " + ", ".join(missing))
    while True:
        try:
            Bot().run()
        except KeyboardInterrupt:
            return
        except Exception as error:
            print("перезапуск: " + str(error), flush=True)
            time.sleep(5)
