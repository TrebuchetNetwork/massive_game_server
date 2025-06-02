import * as flatbuffers from 'flatbuffers';
export declare class TeamScoreEntry {
    bb: flatbuffers.ByteBuffer | null;
    bb_pos: number;
    __init(i: number, bb: flatbuffers.ByteBuffer): TeamScoreEntry;
    static getRootAsTeamScoreEntry(bb: flatbuffers.ByteBuffer, obj?: TeamScoreEntry): TeamScoreEntry;
    static getSizePrefixedRootAsTeamScoreEntry(bb: flatbuffers.ByteBuffer, obj?: TeamScoreEntry): TeamScoreEntry;
    teamId(): number;
    score(): number;
    static startTeamScoreEntry(builder: flatbuffers.Builder): void;
    static addTeamId(builder: flatbuffers.Builder, teamId: number): void;
    static addScore(builder: flatbuffers.Builder, score: number): void;
    static endTeamScoreEntry(builder: flatbuffers.Builder): flatbuffers.Offset;
    static createTeamScoreEntry(builder: flatbuffers.Builder, teamId: number, score: number): flatbuffers.Offset;
}
//# sourceMappingURL=team-score-entry.d.ts.map