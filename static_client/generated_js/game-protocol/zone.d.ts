import * as flatbuffers from 'flatbuffers';
import { ZoneType } from '../game-protocol/zone-type.js';
export declare class Zone {
    bb: flatbuffers.ByteBuffer | null;
    bb_pos: number;
    __init(i: number, bb: flatbuffers.ByteBuffer): Zone;
    static getRootAsZone(bb: flatbuffers.ByteBuffer, obj?: Zone): Zone;
    static getSizePrefixedRootAsZone(bb: flatbuffers.ByteBuffer, obj?: Zone): Zone;
    id(): string | null;
    id(optionalEncoding: flatbuffers.Encoding): string | Uint8Array | null;
    x(): number;
    y(): number;
    width(): number;
    height(): number;
    zoneType(): ZoneType;
    direction(): number;
    static startZone(builder: flatbuffers.Builder): void;
    static addId(builder: flatbuffers.Builder, idOffset: flatbuffers.Offset): void;
    static addX(builder: flatbuffers.Builder, x: number): void;
    static addY(builder: flatbuffers.Builder, y: number): void;
    static addWidth(builder: flatbuffers.Builder, width: number): void;
    static addHeight(builder: flatbuffers.Builder, height: number): void;
    static addZoneType(builder: flatbuffers.Builder, zoneType: ZoneType): void;
    static addDirection(builder: flatbuffers.Builder, direction: number): void;
    static endZone(builder: flatbuffers.Builder): flatbuffers.Offset;
    static createZone(builder: flatbuffers.Builder, idOffset: flatbuffers.Offset, x: number, y: number, width: number, height: number, zoneType: ZoneType, direction: number): flatbuffers.Offset;
}
//# sourceMappingURL=zone.d.ts.map