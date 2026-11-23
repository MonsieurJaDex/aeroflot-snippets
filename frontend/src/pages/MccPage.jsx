import { useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";
import { AIRCRAFT_ISSUES, engineerTypeLabel, issueLabel } from "../lib/catalog.js";
import StatusChain from "../components/StatusChain.jsx";
import { classifyRemark, createRemark } from "../lib/mccApi.js";
import { addMcc, isMccAssigned, listMcc, MCC_QUEUE_KEY } from "../lib/mccQueue.js";
import { getSession } from "../lib/session.js";

const ISSUES = AIRCRAFT_ISSUES.filter(([value]) => value !== "other");

function freshList() {
  return listMcc();
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

function remarkBody({ description, planeUuid, ata, note, urgency }) {
  const body = { text: description, source: "mcc" };
  if (planeUuid) body.plane_uuid = planeUuid;
  if (ata) body.ata_chapter = ata;
  if (note) body.note = note;
  if (urgency) body.urgency = urgency;
  return body;
}

export default function MccPage() {
  const formRef = useRef(null);
  const [items, setItems] = useState(freshList);
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");
  const [hint, setHint] = useState("");
  const [pending, setPending] = useState(false);
  const [classifying, setClassifying] = useState(false);

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
    setError("");
    setStatus("");
    const draft = readForm();
    if (!draft.planeUuid || !draft.description) return;
    if (!signedIn()) {
      addMcc(draft);
      event.currentTarget.reset();
      setHint("");
      setError("Нужен вход, чтобы записать замечание на сервер. Оно осталось в локальной очереди.");
      setItems(freshList());
      return;
    }
    setPending(true);
    try {
      const created = await createRemark(remarkBody(draft));
      addMcc({ ...draft, serverId: created?.id || null, serverStatus: created?.status || "crew_remark" });
      event.currentTarget.reset();
      setHint("");
      setStatus("Замечание записано на сервере. Инженера не назначали.");
      setItems(freshList());
    } catch (err) {
      setError(readError(err));
    } finally {
      setPending(false);
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
      const name = engineerTypeLabel(data.engineer_type) || data.engineer_type;
      setHint(`${name}, уверенность ${confidenceLabel(data.confidence)}. Подсказка, не назначение.`);
    } catch (err) {
      setHint("");
      setError(readError(err));
    } finally {
      setClassifying(false);
    }
  }

  return (
    <main className="role-page">
      <div className="role-shell">
        <div className="role-hero">
          <p className="eyebrow">ЦПУ / MCC</p>
          <h1>Фиксация замечания</h1>
          <p className="role-copy">ЦПУ передаёт замечание начальнику смены. Исполнителя здесь не назначают.</p>
        </div>
        <p className="mcc-back"><Link className="role-link" to="/">К ролям</Link></p>
        <StatusChain current={items.some((item) => !isMccAssigned(item)) ? "не назначен" : items.length ? "назначен" : "замечание"} />
        <form className="auth-panel auth-form" ref={formRef} onSubmit={onSubmit}>
          <label>UUID воздушного судна<input name="plane" type="text" required placeholder="uuid борта из списка начальника смены" /></label>
          <label>Тип неисправности
            <select name="issue" required defaultValue={ISSUES[0][0]}>
              {ISSUES.map(([value, label]) => <option key={value} value={value}>{label}</option>)}
            </select>
          </label>
          <label>Описание<textarea name="description" rows={3} required placeholder="Что сообщил экипаж или сотрудник" /></label>
          <label>Глава ATA<input name="ata" type="text" placeholder="необязательно, например 28" /></label>
          <label>Срочность
            <select name="urgency" defaultValue="">
              <option value="">не указана</option>
              <option value="normal">обычная</option>
              <option value="urgent">срочная</option>
            </select>
          </label>
          <label>Примечание<input name="note" type="text" placeholder="необязательно" /></label>
          <button type="button" className="mcc-classify" onClick={onClassify} disabled={classifying || pending}>
            {classifying ? "Классификация…" : "Подсказать тип инженера"}
          </button>
          {hint ? <p className="mcc-hint">{hint}</p> : null}
          <button type="submit" disabled={pending}>{pending ? "Запись…" : "Передать начальнику смены"}</button>
          {error ? <div className="auth-error">{error}</div> : null}
          <div className="auth-status">{status}</div>
        </form>
        <section className="auth-panel mcc-filed">
          <p className="section-label">Переданные замечания</p>
          <div className="mcc-inbox">
            {items.length ? items.map((item) => (
              <div className="mcc-item" key={item.id}>
                <strong>{issueLabel(item.issue)}</strong>
                <span>
                  {String(item.planeUuid || "").slice(0, 8)} · {isMccAssigned(item) ? "назначена" : "не назначена"}
                  {item.serverId ? ` · сервер ${item.status}` : " · локально"}
                </span>
              </div>
            )) : <p className="mcc-empty">Пока ничего не передано.</p>}
          </div>
        </section>
      </div>
    </main>
  );
}
