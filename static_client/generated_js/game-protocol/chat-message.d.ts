import * as flatbuffers from 'flatbuffers';
export declare class ChatMessage {
    bb: flatbuffers.ByteBuffer | null;
    bb_pos: number;
    __init(i: number, bb: flatbuffers.ByteBuffer): ChatMessage;
    static getRootAsChatMessage(bb: flatbuffers.ByteBuffer, obj?: ChatMessage): ChatMessage;
    static getSizePrefixedRootAsChatMessage(bb: flatbuffers.ByteBuffer, obj?: ChatMessage): ChatMessage;
    seq(): bigint;
    playerId(): string | null;
    playerId(optionalEncoding: flatbuffers.Encoding): string | Uint8Array | null;
    username(): string | null;
    username(optionalEncoding: flatbuffers.Encoding): string | Uint8Array | null;
    message(): string | null;
    message(optionalEncoding: flatbuffers.Encoding): string | Uint8Array | null;
    timestamp(): bigint;
    static startChatMessage(builder: flatbuffers.Builder): void;
    static addSeq(builder: flatbuffers.Builder, seq: bigint): void;
    static addPlayerId(builder: flatbuffers.Builder, playerIdOffset: flatbuffers.Offset): void;
    static addUsername(builder: flatbuffers.Builder, usernameOffset: flatbuffers.Offset): void;
    static addMessage(builder: flatbuffers.Builder, messageOffset: flatbuffers.Offset): void;
    static addTimestamp(builder: flatbuffers.Builder, timestamp: bigint): void;
    static endChatMessage(builder: flatbuffers.Builder): flatbuffers.Offset;
    static createChatMessage(builder: flatbuffers.Builder, seq: bigint, playerIdOffset: flatbuffers.Offset, usernameOffset: flatbuffers.Offset, messageOffset: flatbuffers.Offset, timestamp: bigint): flatbuffers.Offset;
}
//# sourceMappingURL=chat-message.d.ts.map