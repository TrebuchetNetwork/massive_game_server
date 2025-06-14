// automatically generated by the FlatBuffers compiler, do not modify
/* eslint-disable @typescript-eslint/no-unused-vars, @typescript-eslint/no-explicit-any, @typescript-eslint/no-non-null-assertion */
import * as flatbuffers from 'flatbuffers';
import { MessagePayload } from '../game-protocol/message-payload.js';
import { MessageType } from '../game-protocol/message-type.js';
export class GameMessage {
    constructor() {
        this.bb = null;
        this.bb_pos = 0;
    }
    __init(i, bb) {
        this.bb_pos = i;
        this.bb = bb;
        return this;
    }
    static getRootAsGameMessage(bb, obj) {
        return (obj || new GameMessage()).__init(bb.readInt32(bb.position()) + bb.position(), bb);
    }
    static getSizePrefixedRootAsGameMessage(bb, obj) {
        bb.setPosition(bb.position() + flatbuffers.SIZE_PREFIX_LENGTH);
        return (obj || new GameMessage()).__init(bb.readInt32(bb.position()) + bb.position(), bb);
    }
    msgType() {
        const offset = this.bb.__offset(this.bb_pos, 4);
        return offset ? this.bb.readInt8(this.bb_pos + offset) : MessageType.Welcome;
    }
    actualMessageType() {
        const offset = this.bb.__offset(this.bb_pos, 6);
        return offset ? this.bb.readUint8(this.bb_pos + offset) : MessagePayload.NONE;
    }
    actualMessage(obj) {
        const offset = this.bb.__offset(this.bb_pos, 8);
        return offset ? this.bb.__union(obj, this.bb_pos + offset) : null;
    }
    static startGameMessage(builder) {
        builder.startObject(3);
    }
    static addMsgType(builder, msgType) {
        builder.addFieldInt8(0, msgType, MessageType.Welcome);
    }
    static addActualMessageType(builder, actualMessageType) {
        builder.addFieldInt8(1, actualMessageType, MessagePayload.NONE);
    }
    static addActualMessage(builder, actualMessageOffset) {
        builder.addFieldOffset(2, actualMessageOffset, 0);
    }
    static endGameMessage(builder) {
        const offset = builder.endObject();
        return offset;
    }
    static finishGameMessageBuffer(builder, offset) {
        builder.finish(offset);
    }
    static finishSizePrefixedGameMessageBuffer(builder, offset) {
        builder.finish(offset, undefined, true);
    }
    static createGameMessage(builder, msgType, actualMessageType, actualMessageOffset) {
        GameMessage.startGameMessage(builder);
        GameMessage.addMsgType(builder, msgType);
        GameMessage.addActualMessageType(builder, actualMessageType);
        GameMessage.addActualMessage(builder, actualMessageOffset);
        return GameMessage.endGameMessage(builder);
    }
}
//# sourceMappingURL=game-message.js.map