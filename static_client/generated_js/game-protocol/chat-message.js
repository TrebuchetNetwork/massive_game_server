// automatically generated by the FlatBuffers compiler, do not modify
/* eslint-disable @typescript-eslint/no-unused-vars, @typescript-eslint/no-explicit-any, @typescript-eslint/no-non-null-assertion */
import * as flatbuffers from 'flatbuffers';
export class ChatMessage {
    constructor() {
        this.bb = null;
        this.bb_pos = 0;
    }
    __init(i, bb) {
        this.bb_pos = i;
        this.bb = bb;
        return this;
    }
    static getRootAsChatMessage(bb, obj) {
        return (obj || new ChatMessage()).__init(bb.readInt32(bb.position()) + bb.position(), bb);
    }
    static getSizePrefixedRootAsChatMessage(bb, obj) {
        bb.setPosition(bb.position() + flatbuffers.SIZE_PREFIX_LENGTH);
        return (obj || new ChatMessage()).__init(bb.readInt32(bb.position()) + bb.position(), bb);
    }
    seq() {
        const offset = this.bb.__offset(this.bb_pos, 4);
        return offset ? this.bb.readUint64(this.bb_pos + offset) : BigInt('0');
    }
    playerId(optionalEncoding) {
        const offset = this.bb.__offset(this.bb_pos, 6);
        return offset ? this.bb.__string(this.bb_pos + offset, optionalEncoding) : null;
    }
    username(optionalEncoding) {
        const offset = this.bb.__offset(this.bb_pos, 8);
        return offset ? this.bb.__string(this.bb_pos + offset, optionalEncoding) : null;
    }
    message(optionalEncoding) {
        const offset = this.bb.__offset(this.bb_pos, 10);
        return offset ? this.bb.__string(this.bb_pos + offset, optionalEncoding) : null;
    }
    timestamp() {
        const offset = this.bb.__offset(this.bb_pos, 12);
        return offset ? this.bb.readUint64(this.bb_pos + offset) : BigInt('0');
    }
    static startChatMessage(builder) {
        builder.startObject(5);
    }
    static addSeq(builder, seq) {
        builder.addFieldInt64(0, seq, BigInt('0'));
    }
    static addPlayerId(builder, playerIdOffset) {
        builder.addFieldOffset(1, playerIdOffset, 0);
    }
    static addUsername(builder, usernameOffset) {
        builder.addFieldOffset(2, usernameOffset, 0);
    }
    static addMessage(builder, messageOffset) {
        builder.addFieldOffset(3, messageOffset, 0);
    }
    static addTimestamp(builder, timestamp) {
        builder.addFieldInt64(4, timestamp, BigInt('0'));
    }
    static endChatMessage(builder) {
        const offset = builder.endObject();
        return offset;
    }
    static createChatMessage(builder, seq, playerIdOffset, usernameOffset, messageOffset, timestamp) {
        ChatMessage.startChatMessage(builder);
        ChatMessage.addSeq(builder, seq);
        ChatMessage.addPlayerId(builder, playerIdOffset);
        ChatMessage.addUsername(builder, usernameOffset);
        ChatMessage.addMessage(builder, messageOffset);
        ChatMessage.addTimestamp(builder, timestamp);
        return ChatMessage.endChatMessage(builder);
    }
}
//# sourceMappingURL=chat-message.js.map