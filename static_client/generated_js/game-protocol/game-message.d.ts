import * as flatbuffers from 'flatbuffers';
import { MessagePayload } from '../game-protocol/message-payload.js';
import { MessageType } from '../game-protocol/message-type.js';
export declare class GameMessage {
    bb: flatbuffers.ByteBuffer | null;
    bb_pos: number;
    __init(i: number, bb: flatbuffers.ByteBuffer): GameMessage;
    static getRootAsGameMessage(bb: flatbuffers.ByteBuffer, obj?: GameMessage): GameMessage;
    static getSizePrefixedRootAsGameMessage(bb: flatbuffers.ByteBuffer, obj?: GameMessage): GameMessage;
    msgType(): MessageType;
    actualMessageType(): MessagePayload;
    actualMessage<T extends flatbuffers.Table>(obj: any): any | null;
    static startGameMessage(builder: flatbuffers.Builder): void;
    static addMsgType(builder: flatbuffers.Builder, msgType: MessageType): void;
    static addActualMessageType(builder: flatbuffers.Builder, actualMessageType: MessagePayload): void;
    static addActualMessage(builder: flatbuffers.Builder, actualMessageOffset: flatbuffers.Offset): void;
    static endGameMessage(builder: flatbuffers.Builder): flatbuffers.Offset;
    static finishGameMessageBuffer(builder: flatbuffers.Builder, offset: flatbuffers.Offset): void;
    static finishSizePrefixedGameMessageBuffer(builder: flatbuffers.Builder, offset: flatbuffers.Offset): void;
    static createGameMessage(builder: flatbuffers.Builder, msgType: MessageType, actualMessageType: MessagePayload, actualMessageOffset: flatbuffers.Offset): flatbuffers.Offset;
}
//# sourceMappingURL=game-message.d.ts.map