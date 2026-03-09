const DEFAULT_PLAYLIST = [
  "music/Untitled.mp3",
  "music/Untitled (1).mp3",
  "music/Untitled (2).mp3",
  "music/Untitled (4).mp3",
  "music/Untitled (5).mp3",
  "music/Untitled (6).mp3",
  "music/Untitled (8).mp3",
  "music/Untitled (10).mp3",
  "music/Untitled (11).mp3",
  "music/cassete.mp3",
  "music/cassete (1).mp3"
];

const TRACK_CATEGORIES = Object.freeze({
  "music/Untitled.mp3": "ambient",
  "music/Untitled (1).mp3": "ambient",
  "music/Untitled (2).mp3": "ambient",
  "music/Untitled (4).mp3": "action",
  "music/Untitled (5).mp3": "action",
  "music/Untitled (6).mp3": "action",
  "music/Untitled (8).mp3": "action",
  "music/Untitled (10).mp3": "intense",
  "music/Untitled (11).mp3": "intense",
  "music/cassete.mp3": "ambient",
  "music/cassete (1).mp3": "ambient",
});

const CATEGORY_LABELS = Object.freeze({
  ambient: "Ambient",
  action: "Action",
  intense: "Intense",
});

const MUSIC_CROSSFADE_MS = 2000;
const MUSIC_DYNAMIC_POLL_MS = 2000;
const MUSIC_CATEGORY_HYSTERESIS_MS = 5000;
const MUSIC_MIN_TRACK_MS = 30000;
const MUSIC_MANUAL_OVERRIDE_MS = 30000;

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

function asFiniteNumber(value, fallback) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function toTrackName(path) {
  return String(path || "")
    .split("/")
    .pop()
    .replace(/\.mp3$/i, "") || "Unknown";
}

function nowMs() {
  return (typeof performance !== "undefined" && typeof performance.now === "function")
    ? performance.now()
    : Date.now();
}

export class MusicPlayer {
  constructor(options = {}) {
    this.audios = [this.createAudioElement(0), this.createAudioElement(1)];
    this.playlist = Array.isArray(options.playlist) && options.playlist.length > 0
      ? options.playlist.slice()
      : DEFAULT_PLAYLIST.slice();
    this.currentTrackIndex = 0;
    this.currentTrackPath = this.playlist[0] || null;
    this.currentTrackCategory = this.getTrackCategory(this.currentTrackPath);
    this.activeAudioIndex = 0;
    this.trackMix = [1, 0];
    this.isPlaying = false;
    this.isTransitioning = false;
    this.currentTrackStartedAt = 0;
    this.lastDynamicPollAt = 0;
    this.lastRequestedCategory = this.currentTrackCategory;
    this.categoryStableSinceAt = 0;
    this.manualOverrideUntil = 0;
    this.baseVolume = 0.3;
    this.duckMultiplier = 1;
    this.categoryPointers = new Map();
    this.crossfadeTimer = 0;
    this.log = typeof options.log === "function" ? options.log : () => {};
    this.getGameSettings = typeof options.getGameSettings === "function"
      ? options.getGameSettings
      : (() => ({ musicEnabled: false, musicVolume: 0.3 }));
    this.getAudioManager = typeof options.getAudioManager === "function"
      ? options.getAudioManager
      : (() => null);
    this.getLocalPlayerState = typeof options.getLocalPlayerState === "function"
      ? options.getLocalPlayerState
      : (() => null);
    this.progressIntervalMs = Math.max(50, Math.floor(asFiniteNumber(options.progressIntervalMs, 100)));
    this.destroyed = false;

    this.playerDiv = document.getElementById("musicPlayer");
    this.playPauseBtn = document.getElementById("playPauseBtn");
    this.playIcon = document.getElementById("playIcon");
    this.pauseIcon = document.getElementById("pauseIcon");
    this.prevBtn = document.getElementById("prevTrack");
    this.nextBtn = document.getElementById("nextTrack");
    this.volumeSlider = document.getElementById("volumeSlider");
    this.volumeValue = document.getElementById("volumeValue");
    this.progressBar = document.getElementById("progressBar");
    this.nowPlaying = document.getElementById("nowPlaying");
    this.closeBtn = document.getElementById("closeMusicPlayer");
    this.progressContainer = this.progressBar?.parentElement || null;

    if (
      !this.playerDiv ||
      !this.playPauseBtn ||
      !this.prevBtn ||
      !this.nextBtn ||
      !this.volumeSlider ||
      !this.progressBar ||
      !this.nowPlaying
    ) {
      this.log("Music player UI not found; module loaded without controls.", "warn");
      return;
    }

    this.setupEventListeners();
    this.volumeSlider.value = "30";
    this.updateVolume(30);
    this.primeActiveTrack(0);
    this.progressTimer = window.setInterval(() => {
      this.updateProgress();
      this.updateDynamicPlayback();
    }, this.progressIntervalMs);
  }

  createAudioElement(index) {
    const audio = new Audio();
    audio.preload = "auto";
    audio.dataset.channel = String(index);
    audio.volume = 0;
    return audio;
  }

  setupEventListeners() {
    this.onPlayPauseClick = () => this.togglePlayPause();
    this.onPrevClick = () => this.previousTrack();
    this.onNextClick = () => this.nextTrack();
    this.onVolumeInput = (event) => this.updateVolume(event?.target?.value);
    this.onCloseClick = () => this.hide();
    this.onProgressClick = (event) => this.seekToProgress(event);

    this.playPauseBtn.addEventListener("click", this.onPlayPauseClick);
    this.prevBtn.addEventListener("click", this.onPrevClick);
    this.nextBtn.addEventListener("click", this.onNextClick);
    this.volumeSlider.addEventListener("input", this.onVolumeInput);
    if (this.closeBtn) {
      this.closeBtn.addEventListener("click", this.onCloseClick);
    }

    this.onAudioEnded = this.audios.map((_, index) => () => this.handleAudioEnded(index));
    this.audios.forEach((audio, index) => {
      audio.addEventListener("ended", this.onAudioEnded[index]);
    });

    if (this.progressContainer) {
      this.progressContainer.addEventListener("click", this.onProgressClick);
    }
  }

  seekToProgress(event) {
    const rect = this.progressContainer?.getBoundingClientRect();
    if (!rect || rect.width <= 0) return;
    const activeAudio = this.getActiveAudio();
    if (!activeAudio?.duration) return;
    const x = asFiniteNumber(event?.clientX, rect.left) - rect.left;
    const percentage = clamp(x / rect.width, 0, 1);
    activeAudio.currentTime = percentage * activeAudio.duration;
  }

  getActiveAudio() {
    return this.audios[this.activeAudioIndex] || null;
  }

  getStandbyAudio() {
    return this.audios[1 - this.activeAudioIndex] || null;
  }

  getTrackCategory(path) {
    return TRACK_CATEGORIES[path] || "action";
  }

  setAudioSource(audio, trackPath) {
    if (!audio || !trackPath) return;
    if (audio.dataset.trackPath === trackPath) return;
    audio.src = trackPath;
    audio.dataset.trackPath = trackPath;
  }

  getCategoryLabel(category) {
    return CATEGORY_LABELS[category] || "Action";
  }

  syncPlaybackIcons() {
    if (this.playIcon) {
      this.playIcon.classList.toggle("hidden", this.isPlaying);
    }
    if (this.pauseIcon) {
      this.pauseIcon.classList.toggle("hidden", !this.isPlaying);
    }
  }

  setNowPlaying(trackPath) {
    if (!this.nowPlaying) return;
    const category = this.getCategoryLabel(this.getTrackCategory(trackPath));
    this.nowPlaying.textContent = `Track: ${toTrackName(trackPath)} • ${category}`;
  }

  applyAudioMixVolumes() {
    const baseVolume = this.baseVolume * this.duckMultiplier;
    this.audios.forEach((audio, index) => {
      if (!audio) return;
      const mix = clamp(asFiniteNumber(this.trackMix[index], index === this.activeAudioIndex ? 1 : 0), 0, 1);
      audio.volume = clamp(baseVolume * mix, 0, 1);
    });
  }

  primeActiveTrack(index, restart = false) {
    if (!this.playlist.length) return;
    const normalizedIndex = ((Math.floor(index) % this.playlist.length) + this.playlist.length) % this.playlist.length;
    const trackPath = this.playlist[normalizedIndex];
    const activeAudio = this.getActiveAudio();
    this.currentTrackIndex = normalizedIndex;
    this.currentTrackPath = trackPath;
    this.currentTrackCategory = this.getTrackCategory(trackPath);
    if (activeAudio) {
      this.setAudioSource(activeAudio, trackPath);
      if (restart) {
        try {
          activeAudio.currentTime = 0;
        } catch (_) {}
      }
    }
    this.trackMix = this.trackMix.map((_, idx) => (idx === this.activeAudioIndex ? 1 : 0));
    this.setNowPlaying(trackPath);
    this.applyAudioMixVolumes();
  }

  loadTrack(index, options = {}) {
    if (!this.playlist.length) return;
    const normalizedIndex = ((Math.floor(index) % this.playlist.length) + this.playlist.length) % this.playlist.length;
    const trackPath = this.playlist[normalizedIndex];
    const manual = !!options.manual;
    const immediate = !!options.immediate;
    const shouldCrossfade = !!this.isPlaying && !immediate;

    if (manual) {
      this.manualOverrideUntil = nowMs() + MUSIC_MANUAL_OVERRIDE_MS;
    }

    if (!shouldCrossfade) {
      this.cancelCrossfade();
      this.primeActiveTrack(normalizedIndex, !!options.restart);
      if (this.isPlaying) {
        this.play();
      }
      return;
    }

    const standbyAudio = this.getStandbyAudio();
    if (!standbyAudio) {
      this.primeActiveTrack(normalizedIndex, true);
      this.play();
      return;
    }

    this.setAudioSource(standbyAudio, trackPath);
    try {
      standbyAudio.currentTime = 0;
    } catch (_) {}
    standbyAudio.volume = 0;
    standbyAudio.play()
      .then(() => {
        this.startCrossfade(normalizedIndex, trackPath);
      })
      .catch((error) => {
        this.log(`Music crossfade failed: ${error?.message || error}`, "warn");
        this.primeActiveTrack(normalizedIndex, true);
        this.play();
      });
  }

  play() {
    if (!this.playlist.length) return;
    const activeAudio = this.getActiveAudio();
    if (!activeAudio) return;
    if (!activeAudio.dataset.trackPath) {
      this.primeActiveTrack(this.currentTrackIndex || 0);
    }
    activeAudio.play()
      .then(() => {
        this.isPlaying = true;
        if (!this.currentTrackStartedAt) {
          this.currentTrackStartedAt = nowMs();
        }
        this.applyAudioMixVolumes();
        this.syncPlaybackIcons();
      })
      .catch((error) => {
        this.log(`Audio play failed: ${error?.message || error}`, "warn");
      });
  }

  pause() {
    this.cancelCrossfade();
    this.audios.forEach((audio) => audio.pause());
    this.isPlaying = false;
    this.syncPlaybackIcons();
  }

  togglePlayPause() {
    if (this.isPlaying) {
      this.pause();
    } else {
      this.play();
    }
  }

  previousTrack() {
    this.loadTrack(this.currentTrackIndex - 1, { manual: true, restart: true });
  }

  nextTrack() {
    this.loadTrack(this.currentTrackIndex + 1, { manual: true, restart: true });
  }

  updateVolume(rawValue) {
    const volumePercent = clamp(asFiniteNumber(rawValue, 30), 0, 100);
    this.baseVolume = volumePercent / 100;
    this.applyAudioMixVolumes();
    if (this.volumeSlider) {
      this.volumeSlider.value = String(volumePercent);
      this.volumeSlider.style.background =
        `linear-gradient(to right, #6366F1 0%, #6366F1 ${volumePercent}%, #374151 ${volumePercent}%, #374151 100%)`;
    }
    if (this.volumeValue) {
      this.volumeValue.textContent = `${Math.round(volumePercent)}%`;
    }
  }

  updateProgress() {
    if (!this.progressBar) return;
    const activeAudio = this.getActiveAudio();
    if (!activeAudio?.duration || activeAudio.duration <= 0) {
      this.progressBar.style.width = "0%";
      return;
    }
    const percentage = clamp((activeAudio.currentTime / activeAudio.duration) * 100, 0, 100);
    this.progressBar.style.width = `${percentage}%`;
  }

  getCombatEnergy() {
    const audioManager = this.getAudioManager();
    const rawEnergy = Number(audioManager?.ambientCombatEnergy) || 0;
    return clamp(rawEnergy / 1.5, 0, 1);
  }

  getTargetCategory() {
    const localPlayerState = this.getLocalPlayerState();
    if (localPlayerState && localPlayerState.alive === false) {
      return "ambient";
    }
    const energy = this.getCombatEnergy();
    if (energy >= 0.72) return "intense";
    if (energy >= 0.28) return "action";
    return "ambient";
  }

  getDuckMultiplier() {
    const localPlayerState = this.getLocalPlayerState();
    if (localPlayerState && localPlayerState.alive === false) {
      return 1;
    }
    const energy = this.getCombatEnergy();
    return clamp(1 - energy * 0.32, 0.66, 1);
  }

  getCandidateTrackIndexes(category) {
    const matches = [];
    for (let i = 0; i < this.playlist.length; i += 1) {
      if (this.getTrackCategory(this.playlist[i]) === category) {
        matches.push(i);
      }
    }
    return matches.length > 0 ? matches : this.playlist.map((_, index) => index);
  }

  pickTrackForCategory(category, allowCurrent = false) {
    const candidates = this.getCandidateTrackIndexes(category);
    if (!candidates.length) return this.currentTrackIndex || 0;
    const cursor = this.categoryPointers.get(category) || 0;
    for (let offset = 0; offset < candidates.length; offset += 1) {
      const candidate = candidates[(cursor + offset) % candidates.length];
      if (allowCurrent || candidate !== this.currentTrackIndex || candidates.length === 1) {
        this.categoryPointers.set(category, (cursor + offset + 1) % candidates.length);
        return candidate;
      }
    }
    return candidates[0];
  }

  updateDynamicPlayback() {
    const settings = this.getGameSettings() || {};
    // Playback state should continue adapting even when the player UI is hidden.
    if (!settings.musicEnabled || !this.playerDiv) {
      return;
    }

    const targetDuck = this.getDuckMultiplier();
    this.duckMultiplier += (targetDuck - this.duckMultiplier) * 0.18;
    this.applyAudioMixVolumes();

    if (!this.isPlaying || this.isTransitioning) return;

    const now = nowMs();
    if ((now - this.lastDynamicPollAt) < MUSIC_DYNAMIC_POLL_MS) {
      return;
    }
    this.lastDynamicPollAt = now;

    const requestedCategory = this.getTargetCategory();
    if (requestedCategory !== this.lastRequestedCategory) {
      this.lastRequestedCategory = requestedCategory;
      this.categoryStableSinceAt = now;
      return;
    }

    if (now < this.manualOverrideUntil) return;
    if (requestedCategory === this.currentTrackCategory) return;
    if ((now - this.categoryStableSinceAt) < MUSIC_CATEGORY_HYSTERESIS_MS) return;
    if (this.currentTrackStartedAt && (now - this.currentTrackStartedAt) < MUSIC_MIN_TRACK_MS) return;

    this.loadTrack(this.pickTrackForCategory(requestedCategory), { restart: true });
  }

  startCrossfade(index, trackPath) {
    this.cancelCrossfade();
    const incomingIndex = 1 - this.activeAudioIndex;
    const outgoingIndex = this.activeAudioIndex;
    const startedAt = nowMs();
    this.isTransitioning = true;
    this.currentTrackIndex = index;
    this.currentTrackPath = trackPath;
    this.currentTrackCategory = this.getTrackCategory(trackPath);
    this.setNowPlaying(trackPath);

    this.crossfadeTimer = window.setInterval(() => {
      const progress = clamp((nowMs() - startedAt) / MUSIC_CROSSFADE_MS, 0, 1);
      this.trackMix[outgoingIndex] = 1 - progress;
      this.trackMix[incomingIndex] = progress;
      this.applyAudioMixVolumes();
      if (progress >= 1) {
        const outgoingAudio = this.audios[outgoingIndex];
        if (outgoingAudio) {
          outgoingAudio.pause();
          try {
            outgoingAudio.currentTime = 0;
          } catch (_) {}
        }
        this.activeAudioIndex = incomingIndex;
        this.trackMix = this.trackMix.map((_, idx) => (idx === this.activeAudioIndex ? 1 : 0));
        this.currentTrackStartedAt = nowMs();
        this.isTransitioning = false;
        this.cancelCrossfade();
        this.applyAudioMixVolumes();
      }
    }, 50);
  }

  cancelCrossfade() {
    if (this.crossfadeTimer) {
      window.clearInterval(this.crossfadeTimer);
      this.crossfadeTimer = 0;
    }
    this.isTransitioning = false;
  }

  handleAudioEnded(index) {
    if (!this.isPlaying) return;
    if (this.isTransitioning && index !== this.activeAudioIndex) return;
    if (!this.playlist.length) return;

    this.cancelCrossfade();
    this.activeAudioIndex = index;
    this.trackMix = this.trackMix.map((_, idx) => (idx === this.activeAudioIndex ? 1 : 0));
    const nextCategory = this.getTargetCategory();
    const nextIndex = this.pickTrackForCategory(nextCategory, false);
    this.currentTrackStartedAt = 0;
    this.loadTrack(nextIndex, { immediate: true, restart: true });
  }

  show() {
    if (!this.playerDiv) return;
    this.playerDiv.classList.remove("hidden");
    const settings = this.getGameSettings() || {};
    const musicEnabled = !!settings.musicEnabled;
    const musicVolume = clamp(asFiniteNumber(settings.musicVolume, 0.3), 0, 1);
    this.setVolume(musicVolume);
    if (musicEnabled) {
      if (!this.isPlaying) {
        this.play();
      }
    } else {
      this.pause();
    }
  }

  hide() {
    if (!this.playerDiv) return;
    this.playerDiv.classList.add("hidden");
  }

  setEnabled(enabled) {
    if (!this.playerDiv) return;
    if (enabled && this.playerDiv.classList.contains("hidden")) {
      return;
    }
    if (enabled) {
      if (!this.isPlaying) {
        this.play();
      }
    } else {
      this.pause();
    }
  }

  setVolume(volume) {
    const normalized = clamp(asFiniteNumber(volume, 0.3), 0, 1);
    this.updateVolume(normalized * 100);
  }

  destroy() {
    if (this.destroyed) return;
    this.destroyed = true;
    this.cancelCrossfade();
    if (this.progressTimer) {
      window.clearInterval(this.progressTimer);
      this.progressTimer = 0;
    }
    if (this.playPauseBtn && this.onPlayPauseClick) {
      this.playPauseBtn.removeEventListener("click", this.onPlayPauseClick);
    }
    if (this.prevBtn && this.onPrevClick) {
      this.prevBtn.removeEventListener("click", this.onPrevClick);
    }
    if (this.nextBtn && this.onNextClick) {
      this.nextBtn.removeEventListener("click", this.onNextClick);
    }
    if (this.volumeSlider && this.onVolumeInput) {
      this.volumeSlider.removeEventListener("input", this.onVolumeInput);
    }
    if (this.closeBtn && this.onCloseClick) {
      this.closeBtn.removeEventListener("click", this.onCloseClick);
    }
    if (this.progressContainer && this.onProgressClick) {
      this.progressContainer.removeEventListener("click", this.onProgressClick);
    }
    this.audios.forEach((audio, index) => {
      if (audio && this.onAudioEnded?.[index]) {
        audio.removeEventListener("ended", this.onAudioEnded[index]);
      }
    });
    this.pause();
    this.categoryPointers.clear();
    this.audios.forEach((audio) => {
      audio.src = "";
      audio.load();
    });
    this.onAudioEnded = [];
  }
}
