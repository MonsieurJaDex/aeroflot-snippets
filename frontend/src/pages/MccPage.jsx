import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { AIRCRAFT_ISSUES, engineerTypeLabel, issueLabel } from "../lib/catalog.js";
import NetBanner from "../components/NetBanner.jsx";
import PasswordField from "../components/PasswordField.jsx";
import { copyText } from "../lib/copyText.js";
import { relativeRu } from "../lib/format.js";
import { classifyRemark, createRemark, trainClassifier } from "../lib/mccApi.js";
import { addMcc, clearAllMcc, clearAssignedMcc, countOpenMcc, isMccAssigned, listMcc, MCC_QUEUE_KEY } from "../lib/mccQueue.js";
import { clearSession, getSession, login, register } from "../lib/session.js";

const ISSUES = AIRCRAFT_ISSUES.filter(([value]) => value !== "other");

function freshList() {
  return listMcc().slice().sort((a, b) => {
    const urgent = Number(b.urgency === "urgent") - Number(a.urgency === "urgent");
    if (urgent) return urgent;
    return String(b.createdAt || "").localeCompare(String(a.createdAt || ""));
  });
}

function signedIn() {
  return Boolean(getSession()?.accessToken);
}

function readError(error) {
  const raw = String(error?.message || "");
  try {
    const body = JSON.parse(raw);
    if (body?.message) return String(body.message);
  } catch {
    // ответ не JSON
  }
  return raw || "Не удалось выполнить запрос.";
}

function confidenceLabel(value) {
  const n = Number(value);
  if (!Number.isFinite(n)) return "—";
  const pct = n <= 1 ? n * 100 : n;
  return `${Math.round(pct)}%`;
}

function urgencyCaption(value) {
  if (value === "urgent") return "срочная";
  if (value === "normal") return "обычная";
  return "не указана";
}

const EMAIL_RE = /^[^\s@]+@[a-z0-9](?:[a-z0-9-]*[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]*[a-z0-9])?)+$/i;
const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

function standPoint(planeUuid) {
  try {
    const rows = JSON.parse(localStorage.getItem("oto-demo-stands") || "[]");
    if (!Array.isArray(rows)) return null;
    const wanted = String(planeUuid || "");
    const ordered = [
      ...rows.filter((row) => wanted && String(row?.id) === wanted),
      ...rows.filter((row) => String(row?.id) === "demo-stand"),
      ...rows,
    ];
    for (const row of ordered) {
      if (!Array.isArray(row?.point) || row.point.length < 2) continue;
      const x = Number(row.point[0]);
      const y = Number(row.point[1]);
      if (!Number.isFinite(x) || !Number.isFinite(y)) continue;
      return [Math.trunc(x), Math.trunc(y)];
    }
  } catch {
    return null;
  }
  return null;
}

function remarkBody({ description, planeUuid, ata, note, urgency, point }) {
  const body = { text: description, source: "mcc" };
  if (planeUuid && UUID_RE.test(planeUuid)) body.plane_uuid = planeUuid;
  else if (Array.isArray(point) && point.length >= 2) body.point = [Number(point[0]), Number(point[1])];
  if (ata) body.ata_chapter = ata;
  if (note) body.note = note;
  if (urgency) body.urgency = urgency;
  return body;
}

export default function MccPage() {
  const navigate = useNavigate();
  const formRef = useRef(null);
  const [items, setItems] = useState(freshList);
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");
  const [hint, setHint] = useState("");
  const [pending, setPending] = useState(false);
  const [classifying, setClassifying] = useState(false);
  const [sessionName, setSessionName] = useState(() => getSession()?.name || "");
  const [authMode, setAuthMode] = useState("login");
  const [authName, setAuthName] = useState("");
  const [authError, setAuthError] = useState("");
  const [authPending, setAuthPending] = useState(false);
  const [trainPending, setTrainPending] = useState(false);
  const [authEmail, setAuthEmail] = useState("");
  const [authPassword, setAuthPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [savedPlane, setSavedPlane] = useState(() => localStorage.getItem("oto-last-plane") || "");
  const [draftText, setDraftText] = useState(() => sessionStorage.getItem("oto-mcc-draft") || "");
  const [nowTick, setNowTick] = useState(() => Date.now());

  useEffect(() => {
    const timer = window.setInterval(() => setNowTick(Date.now()), 60000);
    return () => clearInterval(timer);
  }, []);

  useEffect(() => {
    const open = countOpenMcc();
    document.title = open ? `ОТО · ЦПУ · очередь ${open}` : "ОТО · ЦПУ";
    return () => { document.title = "Аэрофлот ОТО"; };
  }, [items]);

  useEffect(() => {
    const onStorage = (event) => {
      if (event.key === MCC_QUEUE_KEY) setItems(freshList());
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);

  function readForm() {
    const form = new FormData(formRef.current);
    return {
      planeUuid: String(form.get("plane") || "").trim(),
      issue: String(form.get("issue") || ISSUES[0][0]),
      description: String(form.get("description") || "").trim(),
      ata: String(form.get("ata") || "").trim(),
      note: String(form.get("note") || "").trim(),
      urgency: String(form.get("urgency") || "").trim(),
    };
  }

  async function onSubmit(event) {
    event.preventDefault();
    const form = event.currentTarget;
    setError("");
    setStatus("");
    const draft = readForm();
    if (!draft.description) return;
    const planeIsUuid = UUID_RE.test(draft.planeUuid);
    const point = planeIsUuid ? null : standPoint(draft.planeUuid);
    if (!planeIsUuid && !point && (signedIn() || draft.planeUuid)) {
      setError(draft.planeUuid
        ? "UUID борта выглядит неверно, клетки стоянки нет. Сервер замечание не принял."
        : "Нужен UUID борта или клетка стоянки. Сервер замечание не принял.");
      return;
    }
    if (!signedIn()) {
      addMcc(draft);
      form.reset();
      if (draft.planeUuid) {
        localStorage.setItem("oto-last-plane", draft.planeUuid);
        setSavedPlane(draft.planeUuid);
        const planeInput = form.elements.namedItem("plane");
        if (planeInput) planeInput.value = draft.planeUuid;
      }
      sessionStorage.removeItem("oto-mcc-draft");
      setDraftText("");
      setHint("");
      setError("Нужен вход, чтобы записать замечание на сервер. Оно осталось в локальной очереди.");
      setItems(freshList());
      return;
    }
    setPending(true);
    try {
      const created = await createRemark(remarkBody({ ...draft, point }));
      const serverStatus = typeof created?.status === "string" && created.status ? created.status : "new";
      addMcc({ ...draft, serverId: created?.id || null, serverStatus });
      if (planeIsUuid) {
        localStorage.setItem("oto-last-plane", draft.planeUuid);
        setSavedPlane(draft.planeUuid);
      }
      form.reset();
      if (planeIsUuid) {
        const planeInput = form.elements.namedItem("plane");
        if (planeInput) planeInput.value = draft.planeUuid;
      }
      sessionStorage.removeItem("oto-mcc-draft");
      setDraftText("");
      setHint("");
      setStatus("Замечание записано на сервере. Инженера не назначали.");
      setItems(freshList());
    } catch (error) {
      if (Number(error?.status) !== 404) {
        setError(readError(error));
        return;
      }
      // Живой сервер без /api/requests: замечание остаётся в очереди этого браузера.
      addMcc(draft);
      if (draft.planeUuid) {
        localStorage.setItem("oto-last-plane", draft.planeUuid);
        setSavedPlane(draft.planeUuid);
      }
      form.reset();
      if (draft.planeUuid) {
        const planeInput = form.elements.namedItem("plane");
        if (planeInput) planeInput.value = draft.planeUuid;
      }
      sessionStorage.removeItem("oto-mcc-draft");
      setDraftText("");
      setHint("");
      setStatus("Замечание в очереди диспетчера в этом браузере.");
      setItems(freshList());
    } finally {
      setPending(false);
    }
  }

  function switchAuth(next) {
    setAuthMode(next);
    setAuthPassword("");
    setShowPassword(false);
    setAuthError("");
  }

  async function onAuth(event) {
    event.preventDefault();
    setAuthError("");
    if (!EMAIL_RE.test(authEmail.trim())) {
      setAuthError("Почта: имя@сервис.домен, например ivan@mail.ru");
      return;
    }
    setAuthPending(true);
    try {
      const session = await login(authEmail.trim(), authPassword);
      setSessionName(session?.name || "");
      setAuthPassword("");
      setShowPassword(false);
      setStatus(`Вход: ${session?.name || "ок"}. Можно писать замечания на сервер.`);
    } catch (err) {
      setAuthError(readError(err));
    } finally {
      setAuthPending(false);
    }
  }

  async function onRegister(event) {
    event.preventDefault();
    setAuthError("");
    const name = authName.trim();
    if (!name) {
      setAuthError("Укажите имя.");
      return;
    }
    if (!EMAIL_RE.test(authEmail.trim())) {
      setAuthError("Почта: имя@сервис.домен, например ivan@mail.ru");
      return;
    }
    setAuthPending(true);
    try {
      const session = await register({
        email: authEmail.trim(),
        name,
        password: authPassword,
        userRole: "Dispatcher",
      });
      const shown = session?.name || name;
      setSessionName(shown);
      setAuthPassword("");
      setShowPassword(false);
      setStatus(`Регистрация: ${shown}. Можно писать замечания на сервер.`);
    } catch (err) {
      setAuthError(readError(err));
    } finally {
      setAuthPending(false);
    }
  }

  function onLogout() {
    clearSession();
    setSessionName("");
    setAuthPassword("");
    setShowPassword(false);
    setAuthError("");
    setAuthMode("login");
    setStatus("Вышли. Серверные замечания снова недоступны без входа.");
  }

  function backToRoles() {
    localStorage.removeItem("oto-last-role");
    navigate("/", { state: { resetRoles: true } });
  }

  async function onTrain() {
    setError("");
    if (!signedIn()) {
      setError("Нужен вход, чтобы переобучить классификатор.");
      return;
    }
    if (!window.confirm("Переобучить классификатор на сервере?")) return;
    setTrainPending(true);
    try {
      const data = await trainClassifier();
      const acc = data?.accuracy == null ? "—" : `${Math.round(Number(data.accuracy) * 100)}%`;
      setStatus(`Классификатор обучен: ${data?.documents ?? "?"} док. · accuracy ${acc}`);
    } catch (err) {
      setError(readError(err));
    } finally {
      setTrainPending(false);
    }
  }

  async function onClassify() {
    setError("");
    const { description } = readForm();
    if (!description) {
      setError("Для подсказки нужно описание.");
      return;
    }
    if (!signedIn()) {
      setHint("");
      setError("Нужен вход, чтобы подсказать тип инженера.");
      return;
    }
    setClassifying(true);
    try {
      const data = await classifyRemark(description);
      const typeName = engineerTypeLabel(data.engineer_type) || data.engineer_type;
      setHint(`${typeName}, уверенность ${confidenceLabel(data.confidence)}. Подсказка, не назначение.`);
    } catch (err) {
      setHint("");
      setError(readError(err));
    } finally {
      setClassifying(false);
    }
  }

  return (
    <main id="app-main" className="mcc-page" tabIndex={-1}>
      <NetBanner />
      <div className="mcc-layout">
        <aside className="mcc-sidebar">
          <div className="brand-row">
            <div>
              <p className="eyebrow">АЭРОФЛОТ · ОТО</p>
              <h1>ЦПУ</h1>
            </div>
            <button type="button" className="role-link" onClick={backToRoles}>К ролям</button>
          </div>
          <p className="mcc-intro">Фиксация замечания без назначения инженера. Для записи на сервер нужен вход.</p>

          {!sessionName ? (
            <div className="auth-panel mcc-auth">
              <div className="auth-tabs" role="tablist" aria-label="Вход или регистрация">
                <button className={authMode === "login" ? "auth-tab active" : "auth-tab"} type="button" role="tab" aria-selected={authMode === "login"} onClick={() => switchAuth("login")}>Вход</button>
                <button className={authMode === "register" ? "auth-tab active" : "auth-tab"} type="button" role="tab" aria-selected={authMode === "register"} onClick={() => switchAuth("register")}>Регистрация</button>
              </div>
              {authMode === "login" ? (
                <form className="auth-form" autoComplete="off" onSubmit={onAuth}>
                  <label>Email
                    <input type="email" name="email" required autoComplete="username" value={authEmail} onChange={(event) => setAuthEmail(event.target.value)} />
                  </label>
                  <PasswordField
                    value={authPassword}
                    onChange={(event) => setAuthPassword(event.target.value)}
                    show={showPassword}
                    onToggle={() => setShowPassword((prev) => !prev)}
                  />
                  <button type="submit" disabled={authPending}>{authPending ? "Вход…" : "Войти"}</button>
                </form>
              ) : (
                <form className="auth-form" autoComplete="off" onSubmit={onRegister}>
                  <label>Имя
                    <input type="text" name="name" required autoComplete="name" value={authName} onChange={(event) => setAuthName(event.target.value)} />
                  </label>
                  <label>Email
                    <input type="email" name="email" required autoComplete="username" value={authEmail} onChange={(event) => setAuthEmail(event.target.value)} />
                  </label>
                  <PasswordField
                    autoComplete="new-password"
                    value={authPassword}
                    onChange={(event) => setAuthPassword(event.target.value)}
                    show={showPassword}
                    onToggle={() => setShowPassword((prev) => !prev)}
                  />
                  <button type="submit" disabled={authPending}>{authPending ? "Регистрация…" : "Зарегистрироваться"}</button>
                </form>
              )}
              {authError ? <div className="auth-error" role="alert">{authError}</div> : null}
            </div>
          ) : (
            <div className="auth-panel mcc-session-panel">
              <p className="section-label">Сессия</p>
              <div className="account-name">{sessionName}</div>
              <button type="button" className="role-link" onClick={onLogout}>Выйти</button>
            </div>
          )}
        </aside>

        <section className="mcc-main">
          <form className="auth-panel auth-form mcc-form" ref={formRef} onSubmit={onSubmit}>
            <p className="section-label">Новое замечание</p>
            <label>UUID воздушного судна
              <input name="plane" type="text" spellCheck={false} autoComplete="off" placeholder="необязательно, если борт ещё не известен" defaultValue={savedPlane} />
            </label>
            <button
              type="button"
              className="mcc-classify"
              onClick={async () => {
                try {
                  const text = (await navigator.clipboard.readText()).trim();
                  const match = text.match(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/i);
                  const plane = formRef.current?.elements?.namedItem("plane");
                  if (!match || !plane) {
                    setError("В буфере нет UUID.");
                    return;
                  }
                  plane.value = match[0];
                  localStorage.setItem("oto-last-plane", match[0]);
                  setSavedPlane(match[0]);
                  setStatus(`UUID из буфера: ${match[0].slice(0, 8)}…`);
                } catch {
                  setError("Нет доступа к буферу обмена.");
                }
              }}
            >
              Вставить UUID из буфера
            </button>
            <label>Тип неисправности
              <select
                name="issue"
                required
                defaultValue={localStorage.getItem("oto-mcc-issue") || ISSUES[0][0]}
                onChange={(event) => localStorage.setItem("oto-mcc-issue", event.target.value)}
              >
                {ISSUES.map(([value, label]) => <option key={value} value={value}>{label}</option>)}
              </select>
            </label>
            <label>Описание
              <textarea
                name="description"
                rows={3}
                required
                placeholder="Что сообщил экипаж или сотрудник"
                value={draftText}
                onChange={(event) => {
                  const next = event.target.value;
                  setDraftText(next);
                  sessionStorage.setItem("oto-mcc-draft", next);
                }}
              />
            </label>
            <label>Глава ATA<input name="ata" type="text" placeholder="необязательно, например 28" /></label>
            <label>Срочность
              <select
                name="urgency"
                defaultValue={localStorage.getItem("oto-mcc-urgency") || ""}
                onChange={(event) => localStorage.setItem("oto-mcc-urgency", event.target.value)}
              >
                <option value="">не указана</option>
                <option value="normal">обычная</option>
                <option value="urgent">срочная</option>
              </select>
            </label>
            <label>Примечание<input name="note" type="text" placeholder="необязательно" /></label>
            <button type="button" className="mcc-classify" onClick={onClassify} disabled={classifying || pending || trainPending || !draftText.trim() || !sessionName}>
              {classifying ? "Классификация…" : "Подсказать тип инженера"}
            </button>
            <button type="button" className="mcc-classify" onClick={onTrain} disabled={trainPending || pending || classifying || !sessionName}>
              {trainPending ? "Обучение…" : "Переобучить классификатор"}
            </button>
            {hint ? <p className="mcc-hint">{hint}</p> : null}
            <button type="submit" disabled={pending || !draftText.trim()} aria-busy={pending}>
              {pending ? "Запись…" : "Передать диспетчеру"}
            </button>
            {error ? <div className="auth-error">{error}</div> : null}
            {status ? <div className="auth-status">{status}</div> : null}
          </form>

          <section className="auth-panel mcc-filed">
            <div className="section-heading">
              <p className="section-label">Переданные замечания</p>
              <div className="mcc-list-actions">
                {items.some(isMccAssigned) ? (
                  <button
                    type="button"
                    className="ghost-copy"
                    onClick={() => {
                      clearAssignedMcc();
                      setItems(freshList());
                      setStatus("Назначенные убраны из локального списка.");
                    }}
                  >
                    Убрать назначенные
                  </button>
                ) : null}
                {items.length ? (
                  <button
                    type="button"
                    className="ghost-copy"
                    onClick={() => {
                      const blob = new Blob([JSON.stringify(items, null, 2)], { type: "application/json" });
                      const url = URL.createObjectURL(blob);
                      const link = document.createElement("a");
                      link.href = url;
                      link.download = `mcc-queue-${new Date().toISOString().slice(0, 10)}.json`;
                      document.body.appendChild(link);
                      link.click();
                      link.remove();
                      URL.revokeObjectURL(url);
                      setStatus("Локальная очередь ЦПУ сохранена в JSON.");
                    }}
                  >
                    Экспорт JSON
                  </button>
                ) : null}
                {items.length ? (
                  <button
                    type="button"
                    className="ghost-copy"
                    onClick={() => {
                      if (!window.confirm("Очистить весь локальный список ЦПУ?")) return;
                      clearAllMcc();
                      setItems(freshList());
                      setStatus("Локальный список очищен.");
                    }}
                  >
                    Очистить всё
                  </button>
                ) : null}
              </div>
            </div>
            <div className="mcc-inbox">
              {items.length ? items.map((item) => {
                const plane = String(item.planeUuid || "").trim();
                return (
                  <article className={item.urgency === "urgent" ? "auth-panel mcc-item urgent" : "auth-panel mcc-item"} key={item.id}>
                    <strong>{item.description || issueLabel(item.issue)}</strong>
                    <span>Борт {plane || "не указан"}</span>
                    <span>Срочность {urgencyCaption(item.urgency)}</span>
                    <span>{isMccAssigned(item) ? "назначена" : "не назначена"}</span>
                    <span>
                      {item.serverId ? `сервер ${item.status}` : "локально"}
                      {item.createdAt ? ` · ${relativeRu(item.createdAt, nowTick)}` : ""}
                    </span>
                    {item.serverId ? (
                      <button
                        type="button"
                        className="ghost-copy"
                        onClick={async () => {
                          const ok = await copyText(item.serverId);
                          setStatus(ok ? `Скопирован id ${String(item.serverId).slice(0, 8)}` : "Не удалось скопировать id");
                        }}
                      >
                        Копировать id
                      </button>
                    ) : null}
                  </article>
                );
              }) : <p className="mcc-empty">Пока ничего не передано.</p>}
            </div>
          </section>
        </section>
      </div>
    </main>
  );
}
