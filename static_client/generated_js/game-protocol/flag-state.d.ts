import * as flatbuffers from 'flatbuffers';
import { FlagStatus } from '../game-protocol/flag-status.js';
import { Vec2 } from '../game-protocol/vec2.js';
export declare class FlagState {
    bb: flatbuffers.ByteBuffer | null;
    bb_pos: number;
    __init(i: number, bb: flatbuffers.ByteBuffer): FlagState;
    static getRootAsFlagState(bb: flatbuffers.ByteBuffer, obj?: FlagState): FlagState;
    static getSizePrefixedRootAsFlagState(bb: flatbuffers.ByteBuffer, obj?: FlagState): FlagState;
    teamId(): number;
    status(): FlagStatus;
    position(obj?: Vec2): Vec2 | null;
    carrierId(): string | null;
    carrierId(optionalEncoding: flatbuffers.Encoding): string | Uint8Array | null;
    respawnTimer(): number;
    static startFlagState(builder: flatbuffers.Builder): void;
    static addTeamId(builder: flatbuffers.Builder, teamId: number): void;
    static addStatus(builder: flatbuffers.Builder, status: FlagStatus): void;
    static addPosition(builder: flatbuffers.Builder, positionOffset: flatbuffers.Offset): void;
    static addCarrierId(builder: flatbuffers.Builder, carrierIdOffset: flatbuffers.Offset): void;
    static addRespawnTimer(builder: flatbuffers.Builder, respawnTimer: number): void;
    static endFlagState(builder: flatbuffers.Builder): flatbuffers.Offset;
}
//# sourceMappingURL=flag-state.d.ts.map