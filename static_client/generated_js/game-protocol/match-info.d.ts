import * as flatbuffers from 'flatbuffers';
import { GameModeType } from '../game-protocol/game-mode-type.js';
import { MatchStateType } from '../game-protocol/match-state-type.js';
import { TeamScoreEntry } from '../game-protocol/team-score-entry.js';
export declare class MatchInfo {
    bb: flatbuffers.ByteBuffer | null;
    bb_pos: number;
    __init(i: number, bb: flatbuffers.ByteBuffer): MatchInfo;
    static getRootAsMatchInfo(bb: flatbuffers.ByteBuffer, obj?: MatchInfo): MatchInfo;
    static getSizePrefixedRootAsMatchInfo(bb: flatbuffers.ByteBuffer, obj?: MatchInfo): MatchInfo;
    timeRemaining(): number;
    matchState(): MatchStateType;
    winnerId(): string | null;
    winnerId(optionalEncoding: flatbuffers.Encoding): string | Uint8Array | null;
    winnerName(): string | null;
    winnerName(optionalEncoding: flatbuffers.Encoding): string | Uint8Array | null;
    gameMode(): GameModeType;
    teamScores(index: number, obj?: TeamScoreEntry): TeamScoreEntry | null;
    teamScoresLength(): number;
    static startMatchInfo(builder: flatbuffers.Builder): void;
    static addTimeRemaining(builder: flatbuffers.Builder, timeRemaining: number): void;
    static addMatchState(builder: flatbuffers.Builder, matchState: MatchStateType): void;
    static addWinnerId(builder: flatbuffers.Builder, winnerIdOffset: flatbuffers.Offset): void;
    static addWinnerName(builder: flatbuffers.Builder, winnerNameOffset: flatbuffers.Offset): void;
    static addGameMode(builder: flatbuffers.Builder, gameMode: GameModeType): void;
    static addTeamScores(builder: flatbuffers.Builder, teamScoresOffset: flatbuffers.Offset): void;
    static createTeamScoresVector(builder: flatbuffers.Builder, data: flatbuffers.Offset[]): flatbuffers.Offset;
    static startTeamScoresVector(builder: flatbuffers.Builder, numElems: number): void;
    static endMatchInfo(builder: flatbuffers.Builder): flatbuffers.Offset;
    static createMatchInfo(builder: flatbuffers.Builder, timeRemaining: number, matchState: MatchStateType, winnerIdOffset: flatbuffers.Offset, winnerNameOffset: flatbuffers.Offset, gameMode: GameModeType, teamScoresOffset: flatbuffers.Offset): flatbuffers.Offset;
}
//# sourceMappingURL=match-info.d.ts.map