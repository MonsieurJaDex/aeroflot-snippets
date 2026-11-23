import { Navigate, Route, Routes } from "react-router-dom";
import LoginPage from "./pages/LoginPage.jsx";
import MccPage from "./pages/MccPage.jsx";
import DispatcherPage from "./pages/DispatcherPage.jsx";
import WorkerPage from "./pages/WorkerPage.jsx";

export default function App() {
  return (
    <Routes>
      <Route path="/" element={<LoginPage />} />
      <Route path="/mcc" element={<MccPage />} />
      <Route path="/dispatcher" element={<DispatcherPage />} />
      <Route path="/worker" element={<WorkerPage />} />
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
