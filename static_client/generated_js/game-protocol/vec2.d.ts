import * as flatbuffers from 'flatbuffers';
export declare class Vec2 {
    bb: flatbuffers.ByteBuffer | null;
    bb_pos: number;
    __init(i: number, bb: flatbuffers.ByteBuffer): Vec2;
    static getRootAsVec2(bb: flatbuffers.ByteBuffer, obj?: Vec2): Vec2;
    static getSizePrefixedRootAsVec2(bb: flatbuffers.ByteBuffer, obj?: Vec2): Vec2;
    x(): number;
    y(): number;
    static startVec2(builder: flatbuffers.Builder): void;
    static addX(builder: flatbuffers.Builder, x: number): void;
    static addY(builder: flatbuffers.Builder, y: number): void;
    static endVec2(builder: flatbuffers.Builder): flatbuffers.Offset;
    static createVec2(builder: flatbuffers.Builder, x: number, y: number): flatbuffers.Offset;
}
//# sourceMappingURL=vec2.d.ts.map