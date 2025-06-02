import * as flatbuffers from 'flatbuffers';
import { PickupType } from '../game-protocol/pickup-type.js';
import { WeaponType } from '../game-protocol/weapon-type.js';
export declare class Pickup {
    bb: flatbuffers.ByteBuffer | null;
    bb_pos: number;
    __init(i: number, bb: flatbuffers.ByteBuffer): Pickup;
    static getRootAsPickup(bb: flatbuffers.ByteBuffer, obj?: Pickup): Pickup;
    static getSizePrefixedRootAsPickup(bb: flatbuffers.ByteBuffer, obj?: Pickup): Pickup;
    id(): string | null;
    id(optionalEncoding: flatbuffers.Encoding): string | Uint8Array | null;
    x(): number;
    y(): number;
    pickupType(): PickupType;
    weaponType(): WeaponType;
    isActive(): boolean;
    static startPickup(builder: flatbuffers.Builder): void;
    static addId(builder: flatbuffers.Builder, idOffset: flatbuffers.Offset): void;
    static addX(builder: flatbuffers.Builder, x: number): void;
    static addY(builder: flatbuffers.Builder, y: number): void;
    static addPickupType(builder: flatbuffers.Builder, pickupType: PickupType): void;
    static addWeaponType(builder: flatbuffers.Builder, weaponType: WeaponType): void;
    static addIsActive(builder: flatbuffers.Builder, isActive: boolean): void;
    static endPickup(builder: flatbuffers.Builder): flatbuffers.Offset;
    static createPickup(builder: flatbuffers.Builder, idOffset: flatbuffers.Offset, x: number, y: number, pickupType: PickupType, weaponType: WeaponType, isActive: boolean): flatbuffers.Offset;
}
//# sourceMappingURL=pickup.d.ts.map