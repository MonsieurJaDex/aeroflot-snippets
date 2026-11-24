import { Component } from "react";

export default class ErrorBoundary extends Component {
  constructor(props) {
    super(props);
    this.state = { message: "" };
  }

  static getDerivedStateFromError(error) {
    return { message: error?.message || "Ошибка интерфейса" };
  }

  componentDidCatch(error) {
    console.error("[ui]", error);
  }

  render() {
    if (this.state.message) {
      return (
        <main id="app-main" className="role-page" tabIndex={-1}>
          <div className="role-shell">
            <div className="role-hero">
              <p className="eyebrow">СБОЙ ЭКРАНА</p>
              <h1>Аэрофлот</h1>
              <p className="role-copy">{this.state.message}</p>
            </div>
            <div className="error-actions">
              <button type="button" className="role-link" onClick={() => this.setState({ message: "" })}>
                Попробовать снова
              </button>
              <button type="button" className="role-link" onClick={() => window.location.reload()}>
                Обновить экран
              </button>
              <button type="button" className="role-link" onClick={() => window.location.assign("/")}>
                На вход
              </button>
            </div>
          </div>
        </main>
      );
    }
    return this.props.children;
  }
}
