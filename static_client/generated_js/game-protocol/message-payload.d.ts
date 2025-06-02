import { ChatMessage } from '../game-protocol/chat-message.js';
import { DeltaStateMessage } from '../game-protocol/delta-state-message.js';
import { InitialStateMessage } from '../game-protocol/initial-state-message.js';
import { MatchInfo } from '../game-protocol/match-info.js';
import { PlayerInput } from '../game-protocol/player-input.js';
import { WelcomeMessage } from '../game-protocol/welcome-message.js';
export declare enum MessagePayload {
    NONE = 0,
    WelcomeMessage = 1,
    InitialStateMessage = 2,
    DeltaStateMessage = 3,
    PlayerInput = 4,
    ChatMessage = 5,
    MatchInfo = 6
}
export declare function unionToMessagePayload(type: MessagePayload, accessor: (obj: ChatMessage | DeltaStateMessage | InitialStateMessage | MatchInfo | PlayerInput | WelcomeMessage) => ChatMessage | DeltaStateMessage | InitialStateMessage | MatchInfo | PlayerInput | WelcomeMessage | null): ChatMessage | DeltaStateMessage | InitialStateMessage | MatchInfo | PlayerInput | WelcomeMessage | null;
export declare function unionListToMessagePayload(type: MessagePayload, accessor: (index: number, obj: ChatMessage | DeltaStateMessage | InitialStateMessage | MatchInfo | PlayerInput | WelcomeMessage) => ChatMessage | DeltaStateMessage | InitialStateMessage | MatchInfo | PlayerInput | WelcomeMessage | null, index: number): ChatMessage | DeltaStateMessage | InitialStateMessage | MatchInfo | PlayerInput | WelcomeMessage | null;
//# sourceMappingURL=message-payload.d.ts.map