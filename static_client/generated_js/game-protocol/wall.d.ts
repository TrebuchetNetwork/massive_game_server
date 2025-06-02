import * as flatbuffers from 'flatbuffers';
export declare class Wall {
    bb: flatbuffers.ByteBuffer | null;
    bb_pos: number;
    __init(i: number, bb: flatbuffers.ByteBuffer): Wall;
    static getRootAsWall(bb: flatbuffers.ByteBuffer, obj?: Wall): Wall;
    static getSizePrefixedRootAsWall(bb: flatbuffers.ByteBuffer, obj?: Wall): Wall;
    id(): string | null;
    id(optionalEncoding: flatbuffers.Encoding): string | Uint8Array | null;
    x(): number;
    y(): number;
    width(): number;
    height(): number;
    isDestructible(): boolean;
    currentHealth(): number;
    maxHealth(): number;
    static startWall(builder: flatbuffers.Builder): void;
    static addId(builder: flatbuffers.Builder, idOffset: flatbuffers.Offset): void;
    static addX(builder: flatbuffers.Builder, x: number): void;
    static addY(builder: flatbuffers.Builder, y: number): void;
    static addWidth(builder: flatbuffers.Builder, width: number): void;
    static addHeight(builder: flatbuffers.Builder, height: number): void;
    static addIsDestructible(builder: flatbuffers.Builder, isDestructible: boolean): void;
    static addCurrentHealth(builder: flatbuffers.Builder, currentHealth: number): void;
    static addMaxHealth(builder: flatbuffers.Builder, maxHealth: number): void;
    static endWall(builder: flatbuffers.Builder): flatbuffers.Offset;
    static createWall(builder: flatbuffers.Builder, idOffset: flatbuffers.Offset, x: number, y: number, width: number, height: number, isDestructible: boolean, currentHealth: number, maxHealth: number): flatbuffers.Offset;
}
//# sourceMappingURL=wall.d.ts.map