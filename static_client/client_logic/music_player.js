const DEFAULT_PLAYLIST = [
  "music/Untitled.mp3",
  "music/Untitled (1).mp3",
  "music/Untitled (2).mp3",
  "music/Untitled (3).mp3",
  "music/Untitled (4).mp3",
  "music/Untitled (5).mp3",
  "music/Untitled (6).mp3",
  "music/Untitled (7).mp3",
  "music/Untitled (8).mp3",
  "music/Untitled (9).mp3",
  "music/Untitled (10).mp3",
  "music/Untitled (11).mp3",
  "music/Untitled (12).mp3",
  "music/Untitled (13).mp3",
  "music/cassete.mp3",
  "music/cassete (1).mp3"
];

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

export class MusicPlayer {
  constructor(options = {}) {
    this.audio = new Audio();
    this.playlist = Array.isArray(options.playlist) && options.playlist.length > 0
      ? options.playlist.slice()
      : DEFAULT_PLAYLIST.slice();
    this.currentTrackIndex = 0;
    this.isPlaying = false;
    this.log = typeof options.log === "function" ? options.log : () => {};
    this.getGameSettings = typeof options.getGameSettings === "function"
      ? options.getGameSettings
      : (() => ({ musicEnabled: false, musicVolume: 0.3 }));
    this.progressIntervalMs = Math.max(50, Math.floor(asFiniteNumber(options.progressIntervalMs, 100)));

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
    this.audio.volume = 0.3;
    this.volumeSlider.value = "30";
    this.updateVolume(30);
    this.loadTrack(0);
    this.progressTimer = window.setInterval(() => this.updateProgress(), this.progressIntervalMs);
  }

  setupEventListeners() {
    this.playPauseBtn.addEventListener("click", () => this.togglePlayPause());
    this.prevBtn.addEventListener("click", () => this.previousTrack());
    this.nextBtn.addEventListener("click", () => this.nextTrack());
    this.volumeSlider.addEventListener("input", (event) => this.updateVolume(event?.target?.value));
    if (this.closeBtn) {
      this.closeBtn.addEventListener("click", () => this.hide());
    }

    this.audio.addEventListener("ended", () => this.nextTrack());
    if (this.progressContainer) {
      this.progressContainer.addEventListener("click", (event) => {
        const rect = this.progressContainer.getBoundingClientRect();
        if (!rect || rect.width <= 0) return;
        const x = asFiniteNumber(event?.clientX, rect.left) - rect.left;
        const percentage = clamp(x / rect.width, 0, 1);
        if (this.audio.duration) {
          this.audio.currentTime = percentage * this.audio.duration;
        }
      });
    }
  }

  syncPlaybackIcons() {
    if (this.playIcon) {
      this.playIcon.classList.toggle("hidden", this.isPlaying);
    }
    if (this.pauseIcon) {
      this.pauseIcon.classList.toggle("hidden", !this.isPlaying);
    }
  }

  loadTrack(index) {
    if (!this.playlist.length) return;
    const normalizedIndex = ((Math.floor(index) % this.playlist.length) + this.playlist.length) % this.playlist.length;
    this.currentTrackIndex = normalizedIndex;
    this.audio.src = this.playlist[normalizedIndex];
    this.nowPlaying.textContent = `Track: ${toTrackName(this.playlist[normalizedIndex])}`;
    if (this.isPlaying) {
      this.play();
    }
  }

  play() {
    this.audio.play()
      .then(() => {
        this.isPlaying = true;
        this.syncPlaybackIcons();
      })
      .catch((error) => {
        this.log(`Audio play failed: ${error?.message || error}`, "warn");
      });
  }

  pause() {
    this.audio.pause();
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
    this.loadTrack(this.currentTrackIndex - 1);
  }

  nextTrack() {
    this.loadTrack(this.currentTrackIndex + 1);
  }

  updateVolume(rawValue) {
    const volumePercent = clamp(asFiniteNumber(rawValue, 30), 0, 100);
    const volume = volumePercent / 100;
    this.audio.volume = volume;
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
    if (!this.audio.duration || this.audio.duration <= 0) {
      this.progressBar.style.width = "0%";
      return;
    }
    const percentage = clamp((this.audio.currentTime / this.audio.duration) * 100, 0, 100);
    this.progressBar.style.width = `${percentage}%`;
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
    if (this.progressTimer) {
      window.clearInterval(this.progressTimer);
      this.progressTimer = 0;
    }
    this.pause();
    this.audio.src = "";
  }
}
