import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import NetBanner from "../components/NetBanner.jsx";
import { ENGINEER_TYPES } from "../lib/catalog.js";
import { clearSession, getApiBase, getSession, login, register, setApiBase } from "../lib/session.js";

const ROLE_LABELS = {
  Mcc: "ЦПУ",
  Dispatcher: "Начальник смены",
  Engineer: "Инженер",
};

function homeForRole(role) {
  if (role === "Dispatcher") return "/dispatcher";
  if (role === "Engineer") return "/worker";
  if (role === "Mcc") return "/mcc";
  return "/";
}

export default function LoginPage() {
  const navigate = useNavigate();
  const [apiBase, setApiBaseInput] = useState(getApiBase);
  const [role, setRole] = useState(null);
  const [mode, setMode] = useState("login");
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");
  const [pending, setPending] = useState(false);
  const [savedEmail, setSavedEmail] = useState(() => localStorage.getItem("oto-last-email") || "");
  const [showPassword, setShowPassword] = useState(false);

  useEffect(() => {
    document.title = "Аэрофлот ОТО · вход";
    return () => { document.title = "Аэрофлот ОТО"; };
  }, []);

  useEffect(() => {
    const session = getSession();
    if (session?.role === "Dispatcher" || session?.role === "Engineer") {
      navigate(homeForRole(session.role), { replace: true });
    }
  }, [navigate]);

  function openRole(nextRole) {
    if (nextRole === "Mcc") {
      navigate("/mcc");
      return;
    }
    setRole(nextRole);
    setError("");
    setStatus(`Роль: ${ROLE_LABELS[nextRole]}`);
    setMode("login");
  }

  async function onLogin(event) {
    event.preventDefault();
    setError("");
    setPending(true);
    const form = new FormData(event.currentTarget);
    try {
      const email = String(form.get("email") || "").trim();
      const session = await login(email, String(form.get("password") || ""));
      if (session.role !== role) {
        clearSession();
        setError(`Этот аккаунт зарегистрирован как ${ROLE_LABELS[session.role] || session.role}.`);
        return;
      }
      localStorage.setItem("oto-last-email", email);
      setSavedEmail(email);
      navigate(homeForRole(session.role));
    } catch (err) {
      setError(err.message);
    } finally {
      setPending(false);
    }
  }

  async function onRegister(event) {
    event.preventDefault();
    setError("");
    setPending(true);
    const form = new FormData(event.currentTarget);
    try {
      const email = String(form.get("email") || "").trim();
      const session = await register({
        name: String(form.get("name") || "").trim(),
        email,
        password: String(form.get("password") || ""),
        userRole: role,
        engineerType: role === "Engineer" ? String(form.get("skill") || "") : undefined,
      });
      localStorage.setItem("oto-last-email", email);
      setSavedEmail(email);
      navigate(homeForRole(session.role));
    } catch (err) {
      setError(err.message);
    } finally {
      setPending(false);
    }
  }

  return (
    <main className="role-page">
      <NetBanner />
      <div className="role-shell">
        <div className="role-hero">
          <p className="eyebrow">ОТО · ДИСПЕТЧЕРИЗАЦИЯ</p>
          <h1>Аэрофлот</h1>
          <p className="role-copy">ЦПУ фиксирует замечание, начальник смены назначает, инженер выполняет. Вход по роли.</p>
        </div>

        <details className="api-config">
          <summary>Адрес сервера API</summary>
          <div className="api-config-row">
            <input value={apiBase} onChange={(event) => setApiBaseInput(event.target.value)} placeholder="http://127.0.0.1:3001" />
            <button type="button" onClick={() => setApiBase(apiBase.trim())}>Сохранить</button>
            <button
              type="button"
              onClick={async () => {
                const base = apiBase.trim().replace(/\/$/, "") || getApiBase();
                setApiBase(base);
                setStatus("Проверка сервера…");
                try {
                  const res = await fetch(`${base}/api/aircraft`);
                  setStatus(res.ok || res.status === 401
                    ? `Сервер отвечает (${res.status}).`
                    : `Сервер ответил ${res.status}.`);
                } catch {
                  setStatus("Сервер недоступен по этому адресу.");
                }
              }}
            >
              Проверить
            </button>
          </div>
        </details>

        {role ? (
          <section className="auth-panel">
            <div className="auth-card-head">
              <div>
                <div className="auth-role-badge">{ROLE_LABELS[role]}</div>
                <div className="auth-card-title">Доступ к системе</div>
              </div>
              <button className="auth-close" type="button" onClick={() => setRole(null)}>Назад</button>
            </div>
            <div className="auth-tabs">
              <button className={mode === "login" ? "auth-tab active" : "auth-tab"} type="button" onClick={() => setMode("login")}>Вход</button>
              <button className={mode === "register" ? "auth-tab active" : "auth-tab"} type="button" onClick={() => setMode("register")}>Регистрация</button>
            </div>
            {mode === "login" ? (
              <form className="auth-form" onSubmit={onLogin}>
                <label>Email<input type="email" name="email" required autoComplete="username" defaultValue={savedEmail} /></label>
                <label>Пароль
                  <input type={showPassword ? "text" : "password"} name="password" required autoComplete="current-password" />
                </label>
                <label className="layer-row">
                  <input type="checkbox" checked={showPassword} onChange={(event) => setShowPassword(event.target.checked)} />
                  Показать пароль
                </label>
                <button type="submit" disabled={pending}>{pending ? "Вход…" : "Войти"}</button>
              </form>
            ) : (
              <form className="auth-form" onSubmit={onRegister}>
                <label>Имя<input type="text" name="name" required /></label>
                <label>Email<input type="email" name="email" required autoComplete="username" /></label>
                <label>Пароль
                  <input type={showPassword ? "text" : "password"} name="password" required autoComplete="new-password" />
                </label>
                <label className="layer-row">
                  <input type="checkbox" checked={showPassword} onChange={(event) => setShowPassword(event.target.checked)} />
                  Показать пароль
                </label>
                {role === "Engineer" ? (
                  <label>Специализация
                    <select name="skill" defaultValue={ENGINEER_TYPES[0][0]}>
                      {ENGINEER_TYPES.map(([value, label]) => <option key={value} value={value}>{label}</option>)}
                    </select>
                  </label>
                ) : null}
                <button type="submit" disabled={pending}>{pending ? "Регистрация…" : "Зарегистрироваться"}</button>
              </form>
            )}
            {error ? <div className="auth-error">{error}</div> : null}
            <div className="auth-status">{status}</div>
          </section>
        ) : (
          <div className="role-grid">
            <button className="role-card" type="button" onClick={() => openRole("Mcc")}>
              <span className="role-card-number">01</span>
              <h2>ЦПУ</h2>
              <p>Фиксирует замечание экипажа или сотрудника и передаёт его начальнику смены. Исполнителя не назначает.</p>
              <span>Передать замечание</span>
            </button>
            <button className="role-card" type="button" onClick={() => openRole("Dispatcher")}>
              <span className="role-card-number">02</span>
              <h2>Начальник смены</h2>
              <p>Принимает заявку от ЦПУ, назначает инженера и подтверждает снятие после статуса «в работе».</p>
              <span>Войти как начальник смены</span>
            </button>
            <button className="role-card" type="button" onClick={() => openRole("Engineer")}>
              <span className="role-card-number">03</span>
              <h2>Инженер</h2>
              <p>Принимает или отклоняет заявку, меняет статус, идёт по маршруту, ставит AOG и категорию MEL.</p>
              <span>Войти как инженер</span>
            </button>
          </div>
        )}
      </div>
    </main>
  );
}
