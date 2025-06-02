import * as flatbuffers from 'flatbuffers';
import { Vec2 } from '../game-protocol/vec2.js';
import { WeaponType } from '../game-protocol/weapon-type.js';
export declare class KillFeedEntry {
    bb: flatbuffers.ByteBuffer | null;
    bb_pos: number;
    __init(i: number, bb: flatbuffers.ByteBuffer): KillFeedEntry;
    static getRootAsKillFeedEntry(bb: flatbuffers.ByteBuffer, obj?: KillFeedEntry): KillFeedEntry;
    static getSizePrefixedRootAsKillFeedEntry(bb: flatbuffers.ByteBuffer, obj?: KillFeedEntry): KillFeedEntry;
    killerName(): string | null;
    killerName(optionalEncoding: flatbuffers.Encoding): string | Uint8Array | null;
    victimName(): string | null;
    victimName(optionalEncoding: flatbuffers.Encoding): string | Uint8Array | null;
    weapon(): WeaponType;
    timestamp(): number;
    killerPosition(obj?: Vec2): Vec2 | null;
    victimPosition(obj?: Vec2): Vec2 | null;
    isHeadshot(): boolean;
    static startKillFeedEntry(builder: flatbuffers.Builder): void;
    static addKillerName(builder: flatbuffers.Builder, killerNameOffset: flatbuffers.Offset): void;
    static addVictimName(builder: flatbuffers.Builder, victimNameOffset: flatbuffers.Offset): void;
    static addWeapon(builder: flatbuffers.Builder, weapon: WeaponType): void;
    static addTimestamp(builder: flatbuffers.Builder, timestamp: number): void;
    static addKillerPosition(builder: flatbuffers.Builder, killerPositionOffset: flatbuffers.Offset): void;
    static addVictimPosition(builder: flatbuffers.Builder, victimPositionOffset: flatbuffers.Offset): void;
    static addIsHeadshot(builder: flatbuffers.Builder, isHeadshot: boolean): void;
    static endKillFeedEntry(builder: flatbuffers.Builder): flatbuffers.Offset;
}
//# sourceMappingURL=kill-feed-entry.d.ts.map