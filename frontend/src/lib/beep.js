// Короткий сигнал без ассетов; громкость низкая — для смены, не для колонок.
let sharedCtx = null;
let lastAt = 0;

function ctx() {
  if (!sharedCtx || sharedCtx.state === "closed") {
    sharedCtx = new window.AudioContext();
  }
  if (sharedCtx.state === "suspended") sharedCtx.resume().catch(() => {});
  return sharedCtx;
}

export function softBeep(hz = 880, ms = 180) {
  const now = Date.now();
  // антидребезг: не чаще чем раз в 80 мс
  if (now - lastAt < 80) return;
  lastAt = now;
  try {
    const audio = ctx();
    const osc = audio.createOscillator();
    const gain = audio.createGain();
    osc.frequency.value = hz;
    gain.gain.value = 0.04;
    osc.connect(gain);
    gain.connect(audio.destination);
    osc.start();
    osc.stop(audio.currentTime + ms / 1000);
  } catch {
    // звук опционален
  }
}
