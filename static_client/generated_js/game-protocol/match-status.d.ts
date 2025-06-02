import * as flatbuffers from 'flatbuffers';
import { MatchStateType } from '../game-protocol/match-state-type.js';
import { Team } from '../game-protocol/team.js';
export declare class MatchStatus {
    bb: flatbuffers.ByteBuffer | null;
    bb_pos: number;
    __init(i: number, bb: flatbuffers.ByteBuffer): MatchStatus;
    static getRootAsMatchStatus(bb: flatbuffers.ByteBuffer, obj?: MatchStatus): MatchStatus;
    static getSizePrefixedRootAsMatchStatus(bb: flatbuffers.ByteBuffer, obj?: MatchStatus): MatchStatus;
    state(): MatchStateType;
    timeRemainingSeconds(): number;
    team1Score(): number;
    team2Score(): number;
    winningTeam(): Team;
    static startMatchStatus(builder: flatbuffers.Builder): void;
    static addState(builder: flatbuffers.Builder, state: MatchStateType): void;
    static addTimeRemainingSeconds(builder: flatbuffers.Builder, timeRemainingSeconds: number): void;
    static addTeam1Score(builder: flatbuffers.Builder, team1Score: number): void;
    static addTeam2Score(builder: flatbuffers.Builder, team2Score: number): void;
    static addWinningTeam(builder: flatbuffers.Builder, winningTeam: Team): void;
    static endMatchStatus(builder: flatbuffers.Builder): flatbuffers.Offset;
    static createMatchStatus(builder: flatbuffers.Builder, state: MatchStateType, timeRemainingSeconds: number, team1Score: number, team2Score: number, winningTeam: Team): flatbuffers.Offset;
}
//# sourceMappingURL=match-status.d.ts.map