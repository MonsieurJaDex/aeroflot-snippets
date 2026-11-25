import { useEffect, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import NetBanner from "../components/NetBanner.jsx";
import PasswordField from "../components/PasswordField.jsx";
import { ENGINEER_TYPES } from "../lib/catalog.js";
import { clearSession, getApiBase, getSession, login, register, setApiBase } from "../lib/session.js";

const EMAIL_RE = /^[^\s@]+@[a-z0-9](?:[a-z0-9-]*[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]*[a-z0-9])?)+$/i;

function validEmail(value) {
  const email = String(value || "").trim();
  if (!email) return false;
  return EMAIL_RE.test(email);
}

const ROLE_LABELS = {
  Dispatcher: "Диспетчер",
  Engineer: "Инженер",
};

function homeForRole(role) {
  if (role === "Dispatcher") return "/dispatcher";
  if (role === "Engineer") return "/worker";
  return "/";
}

function humanAuthError(message) {
  const text = String(message || "");
  if (/email already registered|already exists/i.test(text)) {
    return "Эта почта уже есть. Откройте вкладку «Вход» и войдите, повторно регистрироваться не нужно.";
  }
  if (/invalid email or password/i.test(text)) return "Неверная почта или пароль.";
  if (/requires engineer_type/i.test(text)) return "Для инженера выберите специализацию.";
  if (/timed?\s?out|не успел ответить/i.test(text)) return text;
  return text;
}

export default function LoginPage() {
  const navigate = useNavigate();
  const location = useLocation();
  const [apiBase, setApiBaseInput] = useState(getApiBase);
  const [role, setRole] = useState(null);
  const [mode, setMode] = useState("login");
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");
  const [pending, setPending] = useState(false);
  const [email, setEmail] = useState(() => (localStorage.getItem("oto-remember") === "1" ? localStorage.getItem("oto-remember-email") || "" : ""));
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [skill, setSkill] = useState(ENGINEER_TYPES[0][0]);
  const [showPassword, setShowPassword] = useState(false);
  const [remember, setRemember] = useState(() => localStorage.getItem("oto-remember") === "1");
  const panelRef = useRef(null);

  useEffect(() => {
    if (!role) return;
    const panel = panelRef.current;
    if (!panel) return;
    const errorNode = error ? panel.querySelector(".auth-error") : null;
    if (errorNode) {
      errorNode.scrollIntoView({ block: "nearest", inline: "nearest" });
      return;
    }
    if (mode === "register") {
      panel.querySelector(".auth-form button[type=submit]")?.scrollIntoView({ block: "nearest", inline: "nearest" });
      return;
    }
    panel.scrollIntoView({ block: "start", inline: "nearest" });
  }, [role, mode, error]);

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

  useEffect(() => {
    if (location.state?.resetRoles) {
      localStorage.removeItem("oto-last-role");
      setRole(null);
      setMode("login");
      setError("");
      setStatus("");
      navigate("/", { replace: true, state: {} });
    }
  }, [location.state, navigate]);

  function prefetchRole(nextRole) {
    if (nextRole === "Dispatcher") import("./DispatcherPage.jsx");
    if (nextRole === "Engineer") import("./WorkerPage.jsx");
  }

  function resetSecrets() {
    setPassword("");
    setShowPassword(false);
  }

  function openRole(nextRole) {
    prefetchRole(nextRole);
    localStorage.setItem("oto-last-role", nextRole);
    setRole(nextRole);
    setMode("login");
    setName("");
    resetSecrets();
    setError("");
    setStatus("");
  }

  function backToRoles() {
    localStorage.removeItem("oto-last-role");
    setRole(null);
    setMode("login");
    setName("");
    resetSecrets();
    setError("");
    setStatus("");
  }

  function switchMode(next) {
    setMode(next);
    setName("");
    resetSecrets();
    setError("");
  }

  function storeRemember(nextEmail) {
    if (remember) {
      localStorage.setItem("oto-remember", "1");
      localStorage.setItem("oto-remember-email", nextEmail);
    } else {
      localStorage.removeItem("oto-remember");
      localStorage.removeItem("oto-remember-email");
    }
  }

  function formProblem() {
    if (mode === "register" && !name.trim()) return "Введите имя.";
    if (!validEmail(email)) return "Почта: имя@сервис.домен, например ivan@mail.ru";
    if (!password) return "Введите пароль.";
    return "";
  }

  async function onLogin(event) {
    event.preventDefault();
    setError("");
    const problem = formProblem();
    if (problem) {
      setError(problem);
      return;
    }
    const nextEmail = email.trim().toLowerCase();
    setPending(true);
    try {
      const session = await login(nextEmail, password);
      if (session.role !== role) {
        clearSession();
        setError(`Этот аккаунт зарегистрирован как ${ROLE_LABELS[session.role] || session.role}. Откройте карточку «${ROLE_LABELS[session.role] || "другой роли"}».`);
        return;
      }
      storeRemember(nextEmail);
      navigate(homeForRole(session.role));
    } catch (err) {
      setError(humanAuthError(err.message));
    } finally {
      setPending(false);
    }
  }

  async function onRegister(event) {
    event.preventDefault();
    setError("");
    const problem = formProblem();
    if (problem) {
      setError(problem);
      return;
    }
    const nextEmail = email.trim().toLowerCase();
    setPending(true);
    try {
      const session = await register({
        name: name.trim(),
        email: nextEmail,
        password,
        userRole: role,
        engineerType: role === "Engineer" ? skill : undefined,
      });
      if (session?.role !== "Dispatcher" && session?.role !== "Engineer") {
        clearSession();
        setError("Сервер не подтвердил роль. Повторите регистрацию.");
        return;
      }
      storeRemember(nextEmail);
      navigate(homeForRole(session.role));
    } catch (err) {
      setError(humanAuthError(err.message));
    } finally {
      setPending(false);
    }
  }

  return (
    <main id="app-main" className={role ? "role-page gate gate-open" : "role-page gate"} tabIndex={-1}>
      <NetBanner />
      <div className="gate-split">
        <aside className="gate-brand">
          <img className="gate-mark" src="/aeroflot-mark.png" alt="Аэрофлот" />
          <p className="gate-lead">Диспетчер назначает инженера, инженер выходит к борту.</p>
          <ol className="gate-steps">
            <li><span>01</span>Диспетчер выбирает инженера</li>
            <li><span>02</span>Инженер идёт по маршруту</li>
          </ol>
        </aside>
        <div className="gate-stage">

        {["localhost", "127.0.0.1", "::1"].includes(window.location.hostname) ? <details className="api-config">
          <summary>Адрес сервера API</summary>
          <div className="api-config-row">
            <input value={apiBase} onChange={(event) => setApiBaseInput(event.target.value)} placeholder="http://127.0.0.1:3001" />
            <button
              type="button"
              onClick={() => {
                const next = apiBase.trim();
                setApiBase(next);
                setStatus(`API: ${next.replace(/\/$/, "") || getApiBase()}`);
              }}
            >
              Сохранить
            </button>
            <button
              type="button"
              onClick={async () => {
                const base = apiBase.trim().replace(/\/$/, "") || getApiBase();
                setApiBase(base);
                setStatus("Проверка сервера…");
                try {
                  const res = await fetch(`${base}/api/aircraft`);
                  const ok = res.ok || res.status === 401;
                  setStatus(ok ? `Сервер отвечает (${res.status}).` : `Сервер ответил ${res.status}.`);
                } catch {
                  setStatus("Сервер недоступен по этому адресу.");
                }
              }}
            >
              Проверить
            </button>
          </div>
        </details> : null}

        {role ? (
          <section className="auth-panel" ref={panelRef}>
            <div className="auth-card-head">
              <div>
                <div className="auth-role-badge">{ROLE_LABELS[role]}</div>
                <div className="auth-card-title">Доступ к системе</div>
              </div>
              <button className="auth-close" type="button" onClick={backToRoles}>Назад</button>
            </div>
            <div className="auth-tabs">
              <button className={mode === "login" ? "auth-tab active" : "auth-tab"} type="button" onClick={() => switchMode("login")}>Вход</button>
              <button className={mode === "register" ? "auth-tab active" : "auth-tab"} type="button" onClick={() => switchMode("register")}>Регистрация</button>
            </div>
            {mode === "login" ? (
              <form className="auth-form" key={`login-${role}`} autoComplete="off" noValidate onSubmit={onLogin}>
                <label>Email
                  <input type="email" name="email" inputMode="email" autoCapitalize="none" autoCorrect="off" spellCheck={false} autoComplete="username" autoFocus value={email} onChange={(event) => setEmail(event.target.value)} />
                </label>
                <PasswordField
                  autoComplete="current-password"
                  value={password}
                  onChange={(event) => setPassword(event.target.value)}
                  show={showPassword}
                  onToggle={() => setShowPassword((prev) => !prev)}
                />
                <label className="remember-row">
                  <input type="checkbox" checked={remember} onChange={(event) => setRemember(event.target.checked)} />
                  Запомнить почту
                </label>
                {error ? <div className="auth-error" role="alert">{error}</div> : null}
                <button type="submit" disabled={pending} aria-busy={pending}>{pending ? "Вход…" : "Войти"}</button>
              </form>
            ) : (
              <form className="auth-form" key={`register-${role}`} autoComplete="off" noValidate onSubmit={onRegister}>
                <label>Имя
                  <input type="text" name="full-name" autoComplete="name" autoFocus value={name} onChange={(event) => setName(event.target.value)} />
                </label>
                <label>Email
                  <input type="email" name="email" inputMode="email" autoCapitalize="none" autoCorrect="off" spellCheck={false} autoComplete="username" value={email} onChange={(event) => setEmail(event.target.value)} />
                </label>
                <PasswordField
                  autoComplete="new-password"
                  value={password}
                  onChange={(event) => setPassword(event.target.value)}
                  show={showPassword}
                  onToggle={() => setShowPassword((prev) => !prev)}
                />
                {role === "Engineer" ? (
                  <label>Специализация
                    <select name="skill" value={skill} onChange={(event) => setSkill(event.target.value)}>
                      {ENGINEER_TYPES.map(([value, label]) => <option key={value} value={value}>{label}</option>)}
                    </select>
                  </label>
                ) : null}
                <label className="remember-row">
                  <input type="checkbox" checked={remember} onChange={(event) => setRemember(event.target.checked)} />
                  Запомнить почту
                </label>
                {error ? <div className="auth-error" role="alert">{error}</div> : null}
                <button type="submit" disabled={pending}>{pending ? "Регистрация…" : "Зарегистрироваться"}</button>
              </form>
            )}
            {status ? <div className="auth-status">{status}</div> : null}
          </section>
        ) : (
          <div className="role-grid">
            <button className="role-card" type="button" onMouseEnter={() => prefetchRole("Dispatcher")} onFocus={() => prefetchRole("Dispatcher")} onClick={() => openRole("Dispatcher")}>
              <span className="role-card-body">
                <span className="role-card-number">01</span>
                <h2>Диспетчер</h2>
                <p>Назначает инженера и снимает его только после статуса «в работе».</p>
                <span>Войти</span>
              </span>
            </button>
            <button className="role-card" type="button" onMouseEnter={() => prefetchRole("Engineer")} onFocus={() => prefetchRole("Engineer")} onClick={() => openRole("Engineer")}>
              <span className="role-card-body">
                <span className="role-card-number">02</span>
                <h2>Инженер</h2>
                <p>Принимает заявку, идёт по маршруту, ставит MEL или AOG.</p>
                <span>Войти</span>
              </span>
            </button>
          </div>
        )}
        </div>
      </div>
    </main>
  );
}

