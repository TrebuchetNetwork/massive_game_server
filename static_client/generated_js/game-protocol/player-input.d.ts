import * as flatbuffers from 'flatbuffers';
export declare class PlayerInput {
    bb: flatbuffers.ByteBuffer | null;
    bb_pos: number;
    __init(i: number, bb: flatbuffers.ByteBuffer): PlayerInput;
    static getRootAsPlayerInput(bb: flatbuffers.ByteBuffer, obj?: PlayerInput): PlayerInput;
    static getSizePrefixedRootAsPlayerInput(bb: flatbuffers.ByteBuffer, obj?: PlayerInput): PlayerInput;
    timestamp(): bigint;
    sequence(): number;
    moveForward(): boolean;
    moveBackward(): boolean;
    moveLeft(): boolean;
    moveRight(): boolean;
    shooting(): boolean;
    reload(): boolean;
    rotation(): number;
    meleeAttack(): boolean;
    changeWeaponSlot(): number;
    useAbilitySlot(): number;
    static startPlayerInput(builder: flatbuffers.Builder): void;
    static addTimestamp(builder: flatbuffers.Builder, timestamp: bigint): void;
    static addSequence(builder: flatbuffers.Builder, sequence: number): void;
    static addMoveForward(builder: flatbuffers.Builder, moveForward: boolean): void;
    static addMoveBackward(builder: flatbuffers.Builder, moveBackward: boolean): void;
    static addMoveLeft(builder: flatbuffers.Builder, moveLeft: boolean): void;
    static addMoveRight(builder: flatbuffers.Builder, moveRight: boolean): void;
    static addShooting(builder: flatbuffers.Builder, shooting: boolean): void;
    static addReload(builder: flatbuffers.Builder, reload: boolean): void;
    static addRotation(builder: flatbuffers.Builder, rotation: number): void;
    static addMeleeAttack(builder: flatbuffers.Builder, meleeAttack: boolean): void;
    static addChangeWeaponSlot(builder: flatbuffers.Builder, changeWeaponSlot: number): void;
    static addUseAbilitySlot(builder: flatbuffers.Builder, useAbilitySlot: number): void;
    static endPlayerInput(builder: flatbuffers.Builder): flatbuffers.Offset;
    static createPlayerInput(builder: flatbuffers.Builder, timestamp: bigint, sequence: number, moveForward: boolean, moveBackward: boolean, moveLeft: boolean, moveRight: boolean, shooting: boolean, reload: boolean, rotation: number, meleeAttack: boolean, changeWeaponSlot: number, useAbilitySlot: number): flatbuffers.Offset;
}
//# sourceMappingURL=player-input.d.ts.map