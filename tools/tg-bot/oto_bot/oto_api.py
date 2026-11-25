# Клиент живого API: логин диспетчера, алерты, имя, переназначение.

import json
import urllib.error
import urllib.request


class ApiError(Exception):
    def __init__(self, status, message):
        super().__init__(message)
        self.status = status


class OtoClient:
    def __init__(self, base, email, password):
        self.base = base.rstrip("/")
        self.email = email
        self.password = password
        self.access_token = None
        self.refresh_token = None
        self.role = None

    def login(self):
        data = self._request(
            "POST",
            "/api/auth/login",
            {"email": self.email, "password": self.password},
            auth=False,
        )
        self.access_token = data["access_token"]
        self.refresh_token = data.get("refresh_token")
        self.role = str(data.get("user_role") or "")
        if self.role.lower() != "dispatcher":
            raise ApiError(403, "нужна учётка диспетчера, сейчас роль " + self.role)

    def alerts(self):
        data = self._authed("GET", "/api/alerts")
        rows = data.get("alerts") if isinstance(data, dict) else None
        return rows if isinstance(rows, list) else []

    def engineer_name(self, engineer_id):
        if not engineer_id:
            return None
        try:
            data = self._request(
                "GET",
                f"/api/auth/getEngineerName/{engineer_id}",
                auth=False,
            )
        except ApiError:
            return None
        name = data.get("name") if isinstance(data, dict) else None
        return str(name).strip() if name else None

    def reassign(self, request_id, engineer_id, reason):
        return self._authed(
            "POST",
            f"/api/requests/{request_id}/reassign",
            {"engineer_id": engineer_id, "reason": reason},
        )

    def _authed(self, method, path, body=None):
        try:
            return self._request(method, path, body, auth=True)
        except ApiError as error:
            if error.status != 401:
                raise
            self._refresh_or_login()
            return self._request(method, path, body, auth=True)

    def _refresh_or_login(self):
        if self.refresh_token:
            try:
                data = self._request(
                    "POST",
                    "/api/auth/update_access",
                    {"refresh_token": self.refresh_token},
                    auth=False,
                )
                self.access_token = data["access_token"]
                self.refresh_token = data.get("refresh_token") or self.refresh_token
                return
            except ApiError:
                pass
        self.login()

    def _request(self, method, path, body=None, auth=True):
        url = self.base + path
        payload = None if body is None else json.dumps(body).encode("utf-8")
        headers = {"Accept": "application/json"}
        if payload is not None:
            headers["Content-Type"] = "application/json"
        if auth:
            if not self.access_token:
                raise ApiError(401, "нет токена")
            headers["Authorization"] = "Bearer " + self.access_token
        request = urllib.request.Request(url, data=payload, headers=headers, method=method)
        try:
            with urllib.request.urlopen(request, timeout=20) as response:
                raw = response.read().decode("utf-8")
                status = response.status
        except urllib.error.HTTPError as error:
            raw = error.read().decode("utf-8", errors="replace")
            raise ApiError(error.code, _message(raw, error.code)) from None
        except urllib.error.URLError as error:
            raise ApiError(0, "нет связи с API: " + str(error.reason)) from None
        if not raw:
            return {}
        try:
            return json.loads(raw)
        except json.JSONDecodeError:
            if status >= 400:
                raise ApiError(status, raw[:300])
            return {"raw": raw}


def _message(raw, status):
    try:
        body = json.loads(raw)
    except json.JSONDecodeError:
        return raw[:300] or f"ошибка {status}"
    if isinstance(body, dict):
        for key in ("message", "error", "detail"):
            if body.get(key):
                return str(body[key])
    return raw[:300] or f"ошибка {status}"
