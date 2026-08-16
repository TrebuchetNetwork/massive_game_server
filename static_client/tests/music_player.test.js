import test from "node:test";
import assert from "node:assert/strict";

import { MusicPlayer } from "../client_logic/music_player.js";

class FakeClassList {
  constructor() {
    this.tokens = new Set();
  }

  add(token) {
    this.tokens.add(token);
  }

  remove(token) {
    this.tokens.delete(token);
  }

  contains(token) {
    return this.tokens.has(token);
  }

  toggle(token, force) {
    if (force === undefined) {
      if (this.tokens.has(token)) {
        this.tokens.delete(token);
        return false;
      }
      this.tokens.add(token);
      return true;
    }
    if (force) {
      this.tokens.add(token);
      return true;
    }
    this.tokens.delete(token);
    return false;
  }
}

class FakeElement {
  constructor(id) {
    this.id = id;
    this.listeners = new Map();
    this.classList = new FakeClassList();
    this.style = {};
    this.textContent = "";
    this.value = "";
    this.parentElement = null;
  }

  addEventListener(type, handler) {
    const handlers = this.listeners.get(type) || new Set();
    handlers.add(handler);
    this.listeners.set(type, handlers);
  }

  removeEventListener(type, handler) {
    const handlers = this.listeners.get(type);
    if (!handlers) return;
    handlers.delete(handler);
    if (handlers.size === 0) {
      this.listeners.delete(type);
    }
  }

  listenerCount(type) {
    return this.listeners.get(type)?.size || 0;
  }

  getBoundingClientRect() {
    return { left: 0, width: 100 };
  }
}

class FakeAudio {
  constructor() {
    this.dataset = {};
    this.volume = 0;
    this.currentTime = 0;
    this.duration = 120;
    this.listeners = new Map();
    this.loadCount = 0;
    this.pauseCount = 0;
    this.playCount = 0;
    this.src = "";
  }

  addEventListener(type, handler) {
    const handlers = this.listeners.get(type) || new Set();
    handlers.add(handler);
    this.listeners.set(type, handlers);
  }

  removeEventListener(type, handler) {
    const handlers = this.listeners.get(type);
    if (!handlers) return;
    handlers.delete(handler);
    if (handlers.size === 0) {
      this.listeners.delete(type);
    }
  }

  listenerCount(type) {
    return this.listeners.get(type)?.size || 0;
  }

  play() {
    this.playCount += 1;
    return Promise.resolve();
  }

  pause() {
    this.pauseCount += 1;
  }

  load() {
    this.loadCount += 1;
  }
}

function installFakeDom() {
  const elements = new Map();
  const createElement = (id) => {
    const element = new FakeElement(id);
    elements.set(id, element);
    return element;
  };

  createElement("musicPlayer");
  createElement("playPauseBtn");
  createElement("playIcon");
  createElement("pauseIcon");
  createElement("prevTrack");
  createElement("nextTrack");
  createElement("volumeSlider");
  createElement("volumeValue");
  createElement("progressBar");
  createElement("nowPlaying");
  createElement("closeMusicPlayer");

  const progressContainer = new FakeElement("progressContainer");
  elements.get("progressBar").parentElement = progressContainer;

  const originalWindow = globalThis.window;
  const originalDocument = globalThis.document;
  const originalAudio = globalThis.Audio;

  let nextTimerId = 1;
  const activeTimers = new Set();
  globalThis.window = {
    setInterval(callback, _ms) {
      const id = nextTimerId++;
      activeTimers.add(id);
      return id;
    },
    clearInterval(id) {
      activeTimers.delete(id);
    },
  };
  globalThis.document = {
    getElementById(id) {
      return elements.get(id) || null;
    },
  };
  globalThis.Audio = FakeAudio;

  return {
    elements,
    progressContainer,
    getActiveTimerCount: () => activeTimers.size,
    restore() {
      globalThis.window = originalWindow;
      globalThis.document = originalDocument;
      globalThis.Audio = originalAudio;
    },
  };
}

test("MusicPlayer destroy is idempotent and releases listeners, timers, and category state", () => {
  const dom = installFakeDom();
  try {
    const player = new MusicPlayer({
      log: () => {},
      getGameSettings: () => ({ musicEnabled: true, musicVolume: 0.3 }),
    });

    assert.equal(dom.getActiveTimerCount(), 1);
    assert.equal(dom.elements.get("playPauseBtn").listenerCount("click"), 1);
    assert.equal(dom.progressContainer.listenerCount("click"), 1);

    player.categoryPointers.set("ambient", 3);
    assert.equal(player.audios[0].listenerCount("ended"), 1);
    assert.equal(player.audios[1].listenerCount("ended"), 1);

    player.destroy();

    assert.equal(player.destroyed, true);
    assert.equal(dom.getActiveTimerCount(), 0);
    assert.equal(dom.elements.get("playPauseBtn").listenerCount("click"), 0);
    assert.equal(dom.progressContainer.listenerCount("click"), 0);
    assert.equal(player.categoryPointers.size, 0);
    assert.deepEqual(player.onAudioEnded, []);
    assert.equal(player.audios[0].listenerCount("ended"), 0);
    assert.equal(player.audios[1].listenerCount("ended"), 0);
    assert.equal(player.audios[0].loadCount, 1);
    assert.equal(player.audios[1].loadCount, 1);

    player.destroy();

    assert.equal(dom.getActiveTimerCount(), 0);
    assert.equal(player.audios[0].loadCount, 1);
    assert.equal(player.audios[1].loadCount, 1);
  } finally {
    dom.restore();
  }
});

test("MusicPlayer shows track titles instead of raw filenames in the now-playing line", () => {
  const dom = installFakeDom();
  try {
    const player = new MusicPlayer({
      playlist: [
        { title: "Neon Circuit", file: "music/neon-circuit.mp3" },
        { title: "Arena Drift", file: "music/arena-drift.mp3" },
      ],
      log: () => {},
      getGameSettings: () => ({ musicEnabled: false, musicVolume: 0.3 }),
    });
    try {
      const nowPlaying = dom.elements.get("nowPlaying");
      assert.match(nowPlaying.textContent, /^Neon Circuit • /);
      assert.ok(!nowPlaying.textContent.includes(".mp3"));
      assert.ok(!nowPlaying.textContent.includes("neon-circuit"));

      player.nextTrack();
      assert.match(nowPlaying.textContent, /^Arena Drift • /);
      assert.ok(!nowPlaying.textContent.includes(".mp3"));

      // Bare string entries still work and fall back to a derived name.
      const fallback = new MusicPlayer({
        playlist: ["music/some-file.mp3"],
        log: () => {},
        getGameSettings: () => ({ musicEnabled: false, musicVolume: 0.3 }),
      });
      try {
        assert.match(dom.elements.get("nowPlaying").textContent, /^some-file • /);
      } finally {
        fallback.destroy();
      }
    } finally {
      player.destroy();
    }
  } finally {
    dom.restore();
  }
});
