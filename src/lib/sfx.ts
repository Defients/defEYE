import startRecWebcamUrl from "../../media/start-rec-webcam.mp3";
import startRecScreenUrl from "../../media/start-rec-screen.mp3";
import sentinelUrl from "../../media/sentinel.mp3";
import timeLapseUrl from "../../media/time-lapse.mp3";
import buttonClickHeaderUrl from "../../media/button-click-header.mp3";
import buttonClickElseUrl from "../../media/button-click-else.mp3";
import stealthOffUrl from "../../media/stealth-off.mp3";
import stealthOnUrl from "../../media/stealth-on.mp3";
import terminateUrl from "../../media/terminate.mp3";
import stopRecWebcamUrl from "../../media/stop-rec-webcam.mp3";
import stopRecScreenUrl from "../../media/stop-rec-screen.mp3";
import trashUrl from "../../media/trash.mp3";
import sfxToggleUrl from "../../media/sfx-toggle.mp3";
import capturePrimaryUrl from "../../media/capture-primary.mp3";
import captureAllUrl from "../../media/capture-all.mp3";

const SFX_KEY = "defeye-sfx-enabled";

let enabled = localStorage.getItem(SFX_KEY) !== "false";

const volumes: Record<string, number> = {
  "start-rec-webcam": 0.15,
  "start-rec-screen": 0.15,
  "stop-rec-webcam": 0.13,
  "stop-rec-screen": 0.13,
  "time-lapse": 0.16,
  "button-click-header": 0.26,
  sentinel: 0.17,
  trash: 0.07,
  "sfx-toggle": 0.15,
  "capture-primary": 0.10,
  "capture-all": 0.13,
};

const sounds: Record<string, HTMLAudioElement> = {
  "start-rec-webcam": new Audio(startRecWebcamUrl),
  "start-rec-screen": new Audio(startRecScreenUrl),
  "stop-rec-webcam": new Audio(stopRecWebcamUrl),
  "stop-rec-screen": new Audio(stopRecScreenUrl),
  sentinel: new Audio(sentinelUrl),
  "time-lapse": new Audio(timeLapseUrl),
  "button-click-header": new Audio(buttonClickHeaderUrl),
  "button-click-else": new Audio(buttonClickElseUrl),
  "stealth-off": new Audio(stealthOffUrl),
  "stealth-on": new Audio(stealthOnUrl),
  terminate: new Audio(terminateUrl),
  trash: new Audio(trashUrl),
  "sfx-toggle": new Audio(sfxToggleUrl),
  "capture-primary": new Audio(capturePrimaryUrl),
  "capture-all": new Audio(captureAllUrl),
};

export function playSfx(name: string): void {
  if (!enabled) return;
  const audio = sounds[name];
  if (audio) {
    audio.currentTime = 0;
    audio.volume = volumes[name] ?? 1;
    void audio.play().catch(() => undefined);
  }
}

export function isSfxEnabled(): boolean {
  return enabled;
}

export function setSfxEnabled(value: boolean): void {
  enabled = value;
  localStorage.setItem(SFX_KEY, String(value));
}

export function toggleSfx(): boolean {
  setSfxEnabled(!enabled);
  return enabled;
}
