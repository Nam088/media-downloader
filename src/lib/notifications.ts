/**
 * Programmatic Web Audio chime when a download or queue completes.
 */
export function playCompletionChime(): void {
  try {
    const AudioCtx =
      window.AudioContext ||
      (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
    if (!AudioCtx) return;
    const ctx = new AudioCtx();
    const now = ctx.currentTime;

    // Tone 1: E5 (659.25 Hz)
    const osc1 = ctx.createOscillator();
    const gain1 = ctx.createGain();
    osc1.type = "sine";
    osc1.frequency.setValueAtTime(659.25, now);
    gain1.gain.setValueAtTime(0.12, now);
    gain1.gain.exponentialRampToValueAtTime(0.001, now + 0.15);
    osc1.connect(gain1);
    gain1.connect(ctx.destination);
    osc1.start(now);
    osc1.stop(now + 0.15);

    // Tone 2: B5 (987.77 Hz)
    const osc2 = ctx.createOscillator();
    const gain2 = ctx.createGain();
    osc2.type = "sine";
    osc2.frequency.setValueAtTime(987.77, now + 0.12);
    gain2.gain.setValueAtTime(0.15, now + 0.12);
    gain2.gain.exponentialRampToValueAtTime(0.001, now + 0.35);
    osc2.connect(gain2);
    gain2.connect(ctx.destination);
    osc2.start(now + 0.12);
    osc2.stop(now + 0.35);
  } catch {
    // Ignore audio context errors in headless/unsupported environments
  }
}

/**
 * Sends a system desktop notification when queue completes or a download finishes.
 */
export function sendQueueCompletionNotification(totalJobs: number, title?: string, body?: string): void {
  playCompletionChime();

  if (typeof window === "undefined" || !("Notification" in window)) return;

  const defaultTitle = "Media Downloader";
  const defaultBody =
    totalJobs > 1
      ? `🎉 Đã hoàn thành ${totalJobs} lượt tải xuống trong hàng chờ!`
      : `🎉 Đã hoàn thành tải xuống!`;

  const finalTitle = title || defaultTitle;
  const finalBody = body || defaultBody;

  if (Notification.permission === "granted") {
    try {
      new Notification(finalTitle, { body: finalBody });
    } catch {
      // Fallback
    }
  } else if (Notification.permission !== "denied") {
    void Notification.requestPermission().then((permission) => {
      if (permission === "granted") {
        try {
          new Notification(finalTitle, { body: finalBody });
        } catch {
          // Fallback
        }
      }
    });
  }
}
