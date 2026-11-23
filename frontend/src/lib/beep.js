// Короткий сигнал без ассетов; громкость низкая — для смены, не для колонок.
export function softBeep(hz = 880, ms = 180) {
  try {
    const ctx = new window.AudioContext();
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.frequency.value = hz;
    gain.gain.value = 0.04;
    osc.connect(gain);
    gain.connect(ctx.destination);
    osc.start();
    osc.stop(ctx.currentTime + ms / 1000);
    window.setTimeout(() => ctx.close().catch(() => {}), ms + 80);
  } catch {
    // звук опционален
  }
}
