import test from "node:test";
import assert from "node:assert/strict";

import {
    applyConnectionStatusUi,
    normalizeConnectionErrorDetail,
} from "../client_logic/connection_status.js";

function createClassList(initial = []) {
    const values = new Set(initial);
    return {
        add(...classes) {
            classes.forEach((c) => values.add(c));
        },
        remove(...classes) {
            classes.forEach((c) => values.delete(c));
        },
        contains(className) {
            return values.has(className);
        },
    };
}

function createElement(initialClasses = []) {
    return {
        classList: createClassList(initialClasses),
        textContent: "",
    };
}

test("normalizeConnectionErrorDetail uses provided detail or fallback", () => {
    assert.equal(normalizeConnectionErrorDetail("Socket dropped"), "Socket dropped");
    assert.equal(
        normalizeConnectionErrorDetail(""),
        "Connection lost. Click Connect to retry."
    );
});

test("applyConnectionStatusUi hides panel in playing state", () => {
    const div = createElement();
    const title = createElement();
    const detail = createElement();
    const statusUpdates = [];

    const result = applyConnectionStatusUi({
        statusKey: "playing",
        detailText: "Connected",
        connectionStatusDiv: div,
        connectionStatusTitle: title,
        connectionStatusDetail: detail,
        connectionStatusTitles: { playing: "In Match" },
        lastConnectionStatusKey: "connecting",
        lastConnectionDetail: "Trying",
        onStatusChange: (statusKey, text) => statusUpdates.push({ statusKey, text }),
    });

    assert.equal(div.classList.contains("hidden"), true);
    assert.deepEqual(statusUpdates, [{ statusKey: "playing", text: "Connected" }]);
    assert.equal(result.lastConnectionStatusKey, "playing");
    assert.equal(result.lastConnectionDetail, "Connected");
});

test("applyConnectionStatusUi no-ops when status and detail are unchanged", () => {
    const div = createElement(["connection-status--waiting"]);
    const title = createElement();
    const detail = createElement();

    const result = applyConnectionStatusUi({
        statusKey: "waiting",
        detailText: "Queued",
        connectionStatusDiv: div,
        connectionStatusTitle: title,
        connectionStatusDetail: detail,
        connectionStatusTitles: { waiting: "Waiting" },
        lastConnectionStatusKey: "waiting",
        lastConnectionDetail: "Queued",
    });

    assert.equal(title.textContent, "");
    assert.equal(detail.textContent, "");
    assert.equal(result.lastConnectionStatusKey, "waiting");
    assert.equal(result.lastConnectionDetail, "Queued");
});

test("applyConnectionStatusUi maps negotiating to connecting style class", () => {
    const div = createElement(["hidden", "connection-status--idle"]);
    const title = createElement();
    const detail = createElement();

    const result = applyConnectionStatusUi({
        statusKey: "negotiating",
        detailText: "Setting up peer connection",
        connectionStatusDiv: div,
        connectionStatusTitle: title,
        connectionStatusDetail: detail,
        connectionStatusTitles: { negotiating: "Negotiating" },
        lastConnectionStatusKey: "idle",
        lastConnectionDetail: "",
    });

    assert.equal(div.classList.contains("hidden"), false);
    assert.equal(div.classList.contains("connection-status--connecting"), true);
    assert.equal(title.textContent, "Negotiating");
    assert.equal(detail.textContent, "Setting up peer connection");
    assert.equal(result.lastConnectionStatusKey, "negotiating");
});
