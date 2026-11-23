import { useEffect, useState } from "react";

export default function NetBanner() {
  const [online, setOnline] = useState(() => (typeof navigator === "undefined" ? true : navigator.onLine));
  const [flashOk, setFlashOk] = useState(false);

  useEffect(() => {
    const on = () => {
      setOnline(true);
      setFlashOk(true);
    };
    const off = () => {
      setOnline(false);
      setFlashOk(false);
    };
    window.addEventListener("online", on);
    window.addEventListener("offline", off);
    return () => {
      window.removeEventListener("online", on);
      window.removeEventListener("offline", off);
    };
  }, []);

  useEffect(() => {
    if (!flashOk) return undefined;
    const timer = window.setTimeout(() => setFlashOk(false), 2200);
    return () => clearTimeout(timer);
  }, [flashOk]);

  if (!online) {
    return (
      <div className="net-banner" role="status">
        Нет сети. Показаны последние данные; геоточки копятся в очереди.
      </div>
    );
  }
  if (!flashOk) return null;
  return (
    <div className="net-banner ok" role="status">
      Сеть снова доступна.
    </div>
  );
}
