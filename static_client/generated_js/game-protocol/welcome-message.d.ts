import * as flatbuffers from 'flatbuffers';
export declare class WelcomeMessage {
    bb: flatbuffers.ByteBuffer | null;
    bb_pos: number;
    __init(i: number, bb: flatbuffers.ByteBuffer): WelcomeMessage;
    static getRootAsWelcomeMessage(bb: flatbuffers.ByteBuffer, obj?: WelcomeMessage): WelcomeMessage;
    static getSizePrefixedRootAsWelcomeMessage(bb: flatbuffers.ByteBuffer, obj?: WelcomeMessage): WelcomeMessage;
    playerId(): string | null;
    playerId(optionalEncoding: flatbuffers.Encoding): string | Uint8Array | null;
    message(): string | null;
    message(optionalEncoding: flatbuffers.Encoding): string | Uint8Array | null;
    serverTickRate(): number;
    static startWelcomeMessage(builder: flatbuffers.Builder): void;
    static addPlayerId(builder: flatbuffers.Builder, playerIdOffset: flatbuffers.Offset): void;
    static addMessage(builder: flatbuffers.Builder, messageOffset: flatbuffers.Offset): void;
    static addServerTickRate(builder: flatbuffers.Builder, serverTickRate: number): void;
    static endWelcomeMessage(builder: flatbuffers.Builder): flatbuffers.Offset;
    static createWelcomeMessage(builder: flatbuffers.Builder, playerIdOffset: flatbuffers.Offset, messageOffset: flatbuffers.Offset, serverTickRate: number): flatbuffers.Offset;
}
//# sourceMappingURL=welcome-message.d.ts.map