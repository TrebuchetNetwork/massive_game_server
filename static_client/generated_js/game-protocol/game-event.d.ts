import * as flatbuffers from 'flatbuffers';
import { GameEventType } from '../game-protocol/game-event-type.js';
import { Vec2 } from '../game-protocol/vec2.js';
import { WeaponType } from '../game-protocol/weapon-type.js';
export declare class GameEvent {
    bb: flatbuffers.ByteBuffer | null;
    bb_pos: number;
    __init(i: number, bb: flatbuffers.ByteBuffer): GameEvent;
    static getRootAsGameEvent(bb: flatbuffers.ByteBuffer, obj?: GameEvent): GameEvent;
    static getSizePrefixedRootAsGameEvent(bb: flatbuffers.ByteBuffer, obj?: GameEvent): GameEvent;
    eventType(): GameEventType;
    position(obj?: Vec2): Vec2 | null;
    instigatorId(): string | null;
    instigatorId(optionalEncoding: flatbuffers.Encoding): string | Uint8Array | null;
    targetId(): string | null;
    targetId(optionalEncoding: flatbuffers.Encoding): string | Uint8Array | null;
    weaponType(): WeaponType;
    value(): number;
    static startGameEvent(builder: flatbuffers.Builder): void;
    static addEventType(builder: flatbuffers.Builder, eventType: GameEventType): void;
    static addPosition(builder: flatbuffers.Builder, positionOffset: flatbuffers.Offset): void;
    static addInstigatorId(builder: flatbuffers.Builder, instigatorIdOffset: flatbuffers.Offset): void;
    static addTargetId(builder: flatbuffers.Builder, targetIdOffset: flatbuffers.Offset): void;
    static addWeaponType(builder: flatbuffers.Builder, weaponType: WeaponType): void;
    static addValue(builder: flatbuffers.Builder, value: number): void;
    static endGameEvent(builder: flatbuffers.Builder): flatbuffers.Offset;
}
//# sourceMappingURL=game-event.d.ts.map