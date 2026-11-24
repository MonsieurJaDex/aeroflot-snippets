export default function PasswordField({
  name = "password",
  autoComplete = "current-password",
  value,
  onChange,
  required = true,
  show,
  onToggle,
}) {
  return (
    <label className="password-field">
      Пароль
      <span className="password-wrap">
        <input
          type={show ? "text" : "password"}
          name={name}
          required={required}
          autoComplete={autoComplete}
          value={value}
          onChange={onChange}
        />
        <button
          className="password-toggle"
          type="button"
          onClick={onToggle}
          aria-pressed={show}
        >
          {show ? "Скрыть" : "Показать"}
        </button>
      </span>
    </label>
  );
}
