import test from "node:test";
import assert from "node:assert/strict";

import { emitClientLog, flushPendingClientLogs } from "../client_logic/client_logger.js";

test("emitClientLog queues entries until a runtime logger is installed", () => {
    const originalWindow = globalThis.window;
    const captured = [];
    try {
        globalThis.window = {};

        emitClientLog("bootstrap warning", "warn", new Error("boom"));
        emitClientLog("secondary", "info");

        assert.equal(Array.isArray(globalThis.window.__mgsPendingClientLogs), true);
        assert.equal(globalThis.window.__mgsPendingClientLogs.length, 2);

        globalThis.window.__mgsClientLog = (message, level) => {
            captured.push({ message, level });
        };
        flushPendingClientLogs();

        assert.deepEqual(captured, [
            { message: "bootstrap warning: boom", level: "warn" },
            { message: "secondary", level: "info" },
        ]);
        assert.deepEqual(globalThis.window.__mgsPendingClientLogs, []);
    } finally {
        globalThis.window = originalWindow;
    }
});
