import { lazy, Suspense } from "react";
import { Navigate, Route, Routes } from "react-router-dom";
import LoginPage from "./pages/LoginPage.jsx";

const DispatcherPage = lazy(() => import("./pages/DispatcherPage.jsx"));
const WorkerPage = lazy(() => import("./pages/WorkerPage.jsx"));

function ScreenFallback() {
  return (
    <main id="app-main" className="role-page" tabIndex={-1}>
      <div className="role-shell">
        <div className="role-hero">
          <p className="eyebrow">АЭРОФЛОТ · ОТО</p>
          <h1>Аэрофлот</h1>
          <p className="role-copy screen-loading">Загрузка экрана…</p>
        </div>
      </div>
    </main>
  );
}

export default function App() {
  return (
    <Suspense fallback={<ScreenFallback />}>
      <Routes>
        <Route path="/" element={<LoginPage />} />
        <Route path="/dispatcher" element={<DispatcherPage />} />
        <Route path="/worker" element={<WorkerPage />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </Suspense>
  );
}
