import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { AIRCRAFT_ISSUES, engineerTypeLabel, issueLabel } from "../lib/catalog.js";
import NetBanner from "../components/NetBanner.jsx";
import PasswordField from "../components/PasswordField.jsx";
import { copyText } from "../lib/copyText.js";
import { relativeRu } from "../lib/format.js";
import { classifyRemark, createRemark, trainClassifier } from "../lib/mccApi.js";
import { addMcc, clearAllMcc, clearAssignedMcc, countOpenMcc, isMccAssigned, listMcc, MCC_QUEUE_KEY } from "../lib/mccQueue.js";
import { clearSession, getSession, login } from "../lib/session.js";

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

const EMAIL_RE = /^[^\s@]+@[a-z0-9](?:[a-z0-9-]*[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]*[a-z0-9])?)+$/i;

function remarkBody({ description, planeUuid, ata, note, urgency }) {
  const body = { text: description, source: "mcc" };
  if (planeUuid) body.plane_uuid = planeUuid;
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
    if (draft.planeUuid && !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(draft.planeUuid)) {
      setError("UUID борта выглядит неверно. Оставьте пустым или вставьте полный uuid.");
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
      const created = await createRemark(remarkBody(draft));
      addMcc({ ...draft, serverId: created?.id || null, serverStatus: created?.status || "crew_remark" });
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
      setStatus("Замечание записано на сервере. Инженера не назначали.");
      setItems(freshList());
    } catch (err) {
      setError(readError(err));
    } finally {
      setPending(false);
    }
  }

  async function onAuth(event) {
    event.preventDefault();
    setError("");
    if (!EMAIL_RE.test(authEmail.trim())) {
      setError("Почта: имя@сервис.домен, например ivan@mail.ru");
      setAuthPending(false);
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
      setError(readError(err));
    } finally {
      setAuthPending(false);
    }
  }

  function onLogout() {
    clearSession();
    setSessionName("");
    setAuthPassword("");
    setShowPassword(false);
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
            <form className="auth-panel auth-form" autoComplete="off" onSubmit={onAuth}>
              <p className="section-label">Вход для записи на сервер</p>
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
              {pending ? "Запись…" : "Передать начальнику смены"}
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
              {items.length ? items.map((item) => (
                <div className={item.urgency === "urgent" ? "mcc-item urgent" : "mcc-item"} key={item.id}>
                  <strong>{issueLabel(item.issue)}{item.urgency === "urgent" ? " · срочно" : ""}</strong>
                  <span>
                    {String(item.planeUuid || "").slice(0, 8)} · {isMccAssigned(item) ? "назначена" : "не назначена"}
                    {item.serverId ? ` · сервер ${item.status}` : " · локально"}
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
                </div>
              )) : <p className="mcc-empty">Пока ничего не передано.</p>}
            </div>
          </section>
        </section>
      </div>
    </main>
  );
}
