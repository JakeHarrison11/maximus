// Run with: node --test crates/maximus-backend-web/web/bridge.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

test("mounts newly uploaded files before queued requests without restarting the worker", async t => {
    let files = [{ path: "/workspace/old.mac", data: [1] }];
    const sent = [];
    class Worker {
        postMessage(message) { sent.push(message); }
        terminate() {}
    }
    const old = { window: globalThis.window, document: globalThis.document, Worker: globalThis.Worker };
    t.after(() => Object.assign(globalThis, old));
    globalThis.window = { maximusRuntimeFiles: async () => structuredClone(files) };
    globalThis.document = { baseURI: "https://example.test/", getElementById: () => null };
    globalThis.Worker = Worker;
    const source = await readFile(new URL("./bridge.js", import.meta.url), "utf8");
    const { MaximaEngine } = await import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);
    const engine = new MaximaEngine();
    await engine.initialization;
    files = [...files, { path: "/workspace/new.mac", data: [2] }];
    engine.submit("1", "vars", "");
    engine.submit("2", "eval", 'load("/workspace/new.mac")$');
    await engine.requestChain;
    engine.worker.onmessage({ data: { type: "ready" } });
    assert.deepEqual(sent.map(message => message.type), ["init", "request", "request"]);
    assert.deepEqual(sent[1].files, files);
    assert.equal(sent[2].files, undefined);
    assert.equal(sent[2].code, 'load("/workspace/new.mac")$');
    engine.dispose();
});
