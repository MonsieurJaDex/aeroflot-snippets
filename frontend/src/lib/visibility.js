export function whenVisible(callback, ms) {
  let timer = 0;
  function tick() {
    if (document.visibilityState === "visible") callback();
  }
  function arm() {
    window.clearInterval(timer);
    if (document.visibilityState === "visible") {
      timer = window.setInterval(tick, ms);
      tick();
    }
  }
  arm();
  document.addEventListener("visibilitychange", arm);
  return () => {
    window.clearInterval(timer);
    document.removeEventListener("visibilitychange", arm);
  };
}
