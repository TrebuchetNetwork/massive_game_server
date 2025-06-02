import * as flatbuffers from 'flatbuffers';
import { WeaponType } from '../game-protocol/weapon-type.js';
export declare class ProjectileState {
    bb: flatbuffers.ByteBuffer | null;
    bb_pos: number;
    __init(i: number, bb: flatbuffers.ByteBuffer): ProjectileState;
    static getRootAsProjectileState(bb: flatbuffers.ByteBuffer, obj?: ProjectileState): ProjectileState;
    static getSizePrefixedRootAsProjectileState(bb: flatbuffers.ByteBuffer, obj?: ProjectileState): ProjectileState;
    id(): string | null;
    id(optionalEncoding: flatbuffers.Encoding): string | Uint8Array | null;
    x(): number;
    y(): number;
    ownerId(): string | null;
    ownerId(optionalEncoding: flatbuffers.Encoding): string | Uint8Array | null;
    weaponType(): WeaponType;
    velocityX(): number;
    velocityY(): number;
    static startProjectileState(builder: flatbuffers.Builder): void;
    static addId(builder: flatbuffers.Builder, idOffset: flatbuffers.Offset): void;
    static addX(builder: flatbuffers.Builder, x: number): void;
    static addY(builder: flatbuffers.Builder, y: number): void;
    static addOwnerId(builder: flatbuffers.Builder, ownerIdOffset: flatbuffers.Offset): void;
    static addWeaponType(builder: flatbuffers.Builder, weaponType: WeaponType): void;
    static addVelocityX(builder: flatbuffers.Builder, velocityX: number): void;
    static addVelocityY(builder: flatbuffers.Builder, velocityY: number): void;
    static endProjectileState(builder: flatbuffers.Builder): flatbuffers.Offset;
    static createProjectileState(builder: flatbuffers.Builder, idOffset: flatbuffers.Offset, x: number, y: number, ownerIdOffset: flatbuffers.Offset, weaponType: WeaponType, velocityX: number, velocityY: number): flatbuffers.Offset;
}
//# sourceMappingURL=projectile-state.d.ts.map