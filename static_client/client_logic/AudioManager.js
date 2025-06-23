/**
 * AudioManager - Handles sound effects and audio playback using Web Audio API
 * Dependencies: GameProtocol
 */

export class AudioManager {
    constructor() {
        this.soundEnabled = true;
        this.globalVolume = 0.5;
        this.audioContext = null;
        try {
            this.audioContext = new (window.AudioContext || window.webkitAudioContext)();
        } catch (e) {
            console.warn("Web Audio API not supported.");
        }

        this.sounds = {
            pistolFire: { freq: [800, 600], duration: 0.05, type: 'triangle', vol: 0.3 },
            shotgunFire: { freq: [400, 200], duration: 0.15, type: 'sawtooth', vol: 0.5 },
            rifleFire: { freq: [700, 500], duration: 0.07, type: 'square', vol: 0.35 },
            sniperFire: { freq: [1000, 300], duration: 0.2, type: 'sine', vol: 0.6 },
            meleeSwing: { freq: [300, 500], duration: 0.1, type: 'sine', vol: 0.2 },
            bulletImpact: { freq: [200, 100], duration: 0.08, type: 'noise', vol: 0.25 },
            explosion: { freq: [300, 50], duration: 0.5, type: 'noise', vol: 0.7 },
            powerupCollect: { freq: [600, 1200], duration: 0.2, type: 'sine', vol: 0.4 },
            playerHit: { freq: [250, 150], duration: 0.1, type: 'sawtooth', vol: 0.3 },
            flagCapture: { freq: [800, 1000, 1200], duration: 0.4, type: 'square', vol: 0.5 },
            chatMessage: { freq: [1000, 1200], duration: 0.1, type: 'sine', vol: 0.2 },
            outOfAmmo: { freq: [150, 100], duration: 0.15, type: 'square', vol: 0.3 },
            reloadStart: { freq: [400, 300], duration: 0.1, type: 'sawtooth', vol: 0.25 },
            reloadNeeded: { freq: [200], duration: 0.1, type: 'sine', vol: 0.35 },
            flagGrabbed: { freq: [700, 900], duration: 0.2, type: 'triangle', vol: 0.45 },
            flagDropped: { freq: [600, 400], duration: 0.25, type: 'sawtooth', vol: 0.4 },
            flagReturned: { freq: [500, 800, 600], duration: 0.3, type: 'sine', vol: 0.5 },
        };
    }

    setGlobalVolume(volume) {
        this.globalVolume = Math.max(0, Math.min(1, volume));
    }

    setMuted(muted) {
        this.soundEnabled = !muted;
    }

    playWeaponSound(weaponType, position, isLocalPlayer) {
        let soundName;
        const GP = window.GP; // Reference to GameProtocol
        switch (weaponType) {
            case GP.WeaponType.Pistol: soundName = 'pistolFire'; break;
            case GP.WeaponType.Shotgun: soundName = 'shotgunFire'; break;
            case GP.WeaponType.Rifle: soundName = 'rifleFire'; break;
            case GP.WeaponType.Sniper: soundName = 'sniperFire'; break;
            case GP.WeaponType.Melee: soundName = 'meleeSwing'; break;
            default: return;
        }
        this.playSound(soundName, position, isLocalPlayer ? 1.0 : 0.7);
    }

    playSound(soundName, position = null, volumeMultiplier = 1.0) {
        if (!this.soundEnabled || !this.audioContext || !this.sounds[soundName]) return;
        if (this.audioContext.state === 'suspended') { 
            this.audioContext.resume().catch(e => console.warn("AudioContext resume failed:", e));
        }

        const soundProfile = this.sounds[soundName];
        // Ensure duration is positive and finite
        if (!Number.isFinite(soundProfile.duration) || soundProfile.duration <= 0) {
            console.warn(`Invalid duration for sound: ${soundName}`, soundProfile.duration);
            return;
        }

        const baseVolume = (soundProfile.vol !== undefined ? soundProfile.vol : 0.5) * this.globalVolume * volumeMultiplier;
        if (baseVolume <= 0.001) return;

        let finalVolume = baseVolume;
        let pannerNode = null;

        // 3D spatial audio calculation if position is provided
        if (position && window.localPlayerState && window.app && window.gameScene) {
            const viewCenter = { x: window.app.screen.width / 2, y: window.app.screen.height / 2 };
            const soundWorldPos = window.gameScene.toGlobal(position);
            const dx = soundWorldPos.x - viewCenter.x;
            const dy = soundWorldPos.y - viewCenter.y;
            const distance = Math.sqrt(dx * dx + dy * dy);
            const maxAudibleDistance = 800;

            if (distance > maxAudibleDistance) return;
            finalVolume *= Math.max(0, 1 - (distance / maxAudibleDistance));
            if (finalVolume <= 0.001) return;

            pannerNode = this.audioContext.createStereoPanner();
            pannerNode.pan.value = Math.max(-1, Math.min(1, dx / (window.app.screen.width / 2)));
        }
        
        this._playTone(soundProfile, finalVolume, pannerNode);
    }

    _playTone(profile, volume, pannerNode) {
        const now = this.audioContext.currentTime;
        const gainNode = this.audioContext.createGain();
        gainNode.gain.setValueAtTime(volume, now);
        
        // Ensure duration is valid for ramp
        if (Number.isFinite(profile.duration) && profile.duration > 0) {
            gainNode.gain.exponentialRampToValueAtTime(0.001, now + profile.duration);
        } else {
            gainNode.gain.setValueAtTime(0.001, now + 0.01); // Fallback quick fade
        }

        if (pannerNode) {
            gainNode.connect(pannerNode);
            pannerNode.connect(this.audioContext.destination);
        } else {
            gainNode.connect(this.audioContext.destination);
        }

        if (profile.type === 'noise') {
            const bufferSize = this.audioContext.sampleRate * profile.duration;
            const buffer = this.audioContext.createBuffer(1, bufferSize, this.audioContext.sampleRate);
            const output = buffer.getChannelData(0);
            for (let i = 0; i < bufferSize; i++) output[i] = Math.random() * 2 - 1;

            const source = this.audioContext.createBufferSource();
            source.buffer = buffer;
            source.connect(gainNode);
            source.start(now);
            source.stop(now + profile.duration);
        } else {
            const oscillator = this.audioContext.createOscillator();
            oscillator.type = profile.type || 'sine';

            if (Array.isArray(profile.freq)) {
                if (Number.isFinite(profile.freq[0])) {
                   oscillator.frequency.setValueAtTime(profile.freq[0], now);
                } else {
                    console.warn("Invalid profile.freq[0]", profile);
                    return; // Don't play if initial frequency is bad
                }
                
                // Check for freq[1] and valid duration for the first ramp
                if (profile.freq.length > 1 && Number.isFinite(profile.freq[1]) && Number.isFinite(profile.duration) && profile.duration > 0) {
                    oscillator.frequency.linearRampToValueAtTime(profile.freq[1], now + profile.duration * 0.8);
                }
                // Check for freq[2] and valid duration for the second ramp
                if (profile.freq.length > 2 && Number.isFinite(profile.freq[2]) && Number.isFinite(profile.duration) && profile.duration > 0) {
                    oscillator.frequency.linearRampToValueAtTime(profile.freq[2], now + profile.duration);
                }
            } else if (Number.isFinite(profile.freq)) {
                oscillator.frequency.setValueAtTime(profile.freq, now);
            } else {
                console.warn("Invalid profile.freq", profile);
                return; // Don't play if frequency is bad
            }
            
            oscillator.connect(gainNode);
            oscillator.start(now);
            // Ensure duration is positive for stop time
            oscillator.stop(now + (Number.isFinite(profile.duration) && profile.duration > 0 ? profile.duration : 0.01));
        }
    }

    // Resume audio context on user interaction
    resumeContext() {
        if (this.audioContext && this.audioContext.state === 'suspended') {
            return this.audioContext.resume();
        }
        return Promise.resolve();
    }
}
